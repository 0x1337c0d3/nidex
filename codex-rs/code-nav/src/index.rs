use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;
use ignore::WalkBuilder;
use sqlx::ConnectOptions;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::sqlite::SqliteSynchronous;

use crate::language::Lang;
use crate::symbols::Symbol;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Mtime as (secs, nanos) since UNIX_EPOCH, stored as i64 for SQLite.
type Mtime = (i64, i64);

pub struct NavIndex {
    pool: SqlitePool,
}

impl NavIndex {
    /// Open (or create) the index at `<root>/.codex-nav/index.db`.
    /// If the DB is corrupted, deletes it and retries once automatically.
    pub async fn open(root: &Path) -> Result<Self> {
        let db_dir = root.join(".codex-nav");
        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("create {}", db_dir.display()))?;
        let db_path = db_dir.join("index.db");
        match open_pool(&db_path).await {
            Ok(pool) => Ok(Self { pool }),
            Err(err) => {
                tracing::warn!("code-nav index open failed ({err:#}), resetting and retrying");
                let _ = std::fs::remove_file(&db_path);
                let pool = open_pool(&db_path).await?;
                Ok(Self { pool })
            }
        }
    }

    /// Delete the existing DB and reopen fresh. Used by `code_nav_init` with `reset: true`.
    pub async fn reset(root: &Path) -> Result<Self> {
        let db_path = root.join(".codex-nav").join("index.db");
        let _ = std::fs::remove_file(&db_path);
        Self::open(root).await
    }

    /// Run a full incremental index update for `path`.
    ///
    /// Scans for changed files, re-parses stale ones, updates the index, and
    /// prunes entries for deleted files. Safe to call from a background task.
    pub async fn warm(path: &Path, lang_filter: Option<Lang>) -> Result<()> {
        let root = find_project_root(path);
        let index = Self::open(&root).await?;
        let prefix = path.to_string_lossy().into_owned();
        let cached = index.get_cached_mtimes(&prefix).await?;

        let path_owned = path.to_owned();
        let (stale, existing) = tokio::task::spawn_blocking(move || {
            scan_for_changes(&path_owned, lang_filter, &cached)
        })
        .await
        .context("scan task panicked")??;

        let parsed =
            tokio::task::spawn_blocking(move || crate::symbols::run_symbols_for_files(&stale))
                .await
                .context("parse task panicked")??;

        for (file, mtime, syms) in &parsed {
            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
            if let Some(lang) = Lang::from_extension(ext) {
                index.update_file(file, lang, *mtime, syms).await?;
            }
        }

        index.remove_deleted_files(&existing).await?;
        Ok(())
    }

    /// Return cached `{path → mtime}` for all indexed files whose path starts with `prefix`.
    pub async fn get_cached_mtimes(&self, prefix: &str) -> Result<HashMap<PathBuf, Mtime>> {
        let pattern = format!("{prefix}%");
        let rows = sqlx::query(
            "SELECT path, mtime_secs, mtime_nanos FROM indexed_files WHERE path LIKE ?",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .context("get_cached_mtimes")?;

        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            use sqlx::Row;
            let path: String = row.get("path");
            let secs: i64 = row.get("mtime_secs");
            let nanos: i64 = row.get("mtime_nanos");
            map.insert(PathBuf::from(path), (secs, nanos));
        }
        Ok(map)
    }

    /// Return all cached symbols for files whose path starts with `prefix`.
    pub async fn get_symbols_for_prefix(&self, prefix: &str) -> Result<Vec<Symbol>> {
        let pattern = format!("{prefix}%");
        let rows = sqlx::query(
            "SELECT s.name, s.kind, s.file_path, s.line \
             FROM symbols s \
             JOIN indexed_files f ON s.file_path = f.path \
             WHERE f.path LIKE ? \
             ORDER BY f.path, s.line",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .context("get_symbols_for_prefix")?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            use sqlx::Row;
            let name: String = row.get("name");
            let kind_str: String = row.get("kind");
            let file: String = row.get("file_path");
            let line: i64 = row.get("line");
            out.push(Symbol {
                name,
                kind: crate::symbols::SymbolKind::from_str(&kind_str),
                file,
                line: line as usize,
            });
        }
        Ok(out)
    }

    /// Replace the cached symbols for a single file.
    pub async fn update_file(
        &self,
        file: &Path,
        lang: Lang,
        mtime: SystemTime,
        symbols: &[Symbol],
    ) -> Result<()> {
        let path_str = file.to_string_lossy().into_owned();
        let lang_str = lang.as_str().to_string();
        let dur = mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        let mtime_secs = dur.as_secs() as i64;
        let mtime_nanos = dur.subsec_nanos() as i64;

        let mut tx = self.pool.begin().await.context("begin transaction")?;

        sqlx::query("DELETE FROM indexed_files WHERE path = ?")
            .bind(&path_str)
            .execute(&mut *tx)
            .await
            .context("delete indexed_file")?;

        sqlx::query(
            "INSERT INTO indexed_files (path, lang, mtime_secs, mtime_nanos) VALUES (?, ?, ?, ?)",
        )
        .bind(&path_str)
        .bind(&lang_str)
        .bind(mtime_secs)
        .bind(mtime_nanos)
        .execute(&mut *tx)
        .await
        .context("insert indexed_file")?;

        for sym in symbols {
            sqlx::query(
                "INSERT INTO symbols (file_path, name, kind, line) VALUES (?, ?, ?, ?)",
            )
            .bind(&path_str)
            .bind(&sym.name)
            .bind(sym.kind.as_str())
            .bind(sym.line as i64)
            .execute(&mut *tx)
            .await
            .context("insert symbol")?;
        }

        tx.commit().await.context("commit transaction")?;
        Ok(())
    }

    /// Remove indexed_files rows whose paths are no longer on disk.
    /// ON DELETE CASCADE removes their symbols automatically.
    pub async fn remove_deleted_files(&self, existing_paths: &HashSet<String>) -> Result<()> {
        let all_rows =
            sqlx::query("SELECT path FROM indexed_files")
                .fetch_all(&self.pool)
                .await
                .context("list indexed files")?;

        for row in all_rows {
            use sqlx::Row;
            let path: String = row.get("path");
            if !existing_paths.contains(&path) {
                sqlx::query("DELETE FROM indexed_files WHERE path = ?")
                    .bind(&path)
                    .execute(&self.pool)
                    .await
                    .context("delete stale file")?;
            }
        }
        Ok(())
    }
}

/// Walk `path`, compare actual mtimes against `cached_mtimes`, and return:
/// - `stale`: files that need re-indexing `(path, lang, actual_mtime)`
/// - `existing`: canonical string paths of every file found (for pruning)
///
/// Runs synchronously; call inside `tokio::task::spawn_blocking`.
pub fn scan_for_changes(
    path: &Path,
    lang_filter: Option<Lang>,
    cached_mtimes: &HashMap<PathBuf, Mtime>,
) -> Result<(Vec<(PathBuf, Lang, SystemTime)>, HashSet<String>)> {
    let mut stale: Vec<(PathBuf, Lang, SystemTime)> = Vec::new();
    let mut existing: HashSet<String> = HashSet::new();

    let walk = WalkBuilder::new(path).build();
    for entry in walk.flatten() {
        if !entry.file_type().map_or(false, |t| t.is_file()) {
            continue;
        }
        let file_path = entry.path().to_owned();
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang = match Lang::from_extension(ext) {
            Some(l) => l,
            None => continue,
        };
        if let Some(filter) = lang_filter {
            if lang != filter {
                continue;
            }
        }

        let key = file_path.to_string_lossy().into_owned();
        existing.insert(key);

        let actual_mtime = std::fs::metadata(&file_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let actual_dur = actual_mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        let actual_secs = actual_dur.as_secs() as i64;
        let actual_nanos = actual_dur.subsec_nanos() as i64;

        let is_fresh = cached_mtimes
            .get(&file_path)
            .map_or(false, |&(s, n)| s == actual_secs && n == actual_nanos);

        if !is_fresh {
            stale.push((file_path, lang, actual_mtime));
        }
    }

    Ok((stale, existing))
}

/// Find the project root by walking up from `path` looking for `.git/`.
/// Falls back to `path` itself if no `.git/` is found.
pub fn find_project_root(path: &Path) -> PathBuf {
    let base = if path.is_file() {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    let mut dir: &Path = &base;
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return base,
        }
    }
}

async fn open_pool(path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .log_statements(log::LevelFilter::Off);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("open sqlite pool")?;
    MIGRATOR.run(&pool).await.context("run migrations")?;
    Ok(pool)
}
