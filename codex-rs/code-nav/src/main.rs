use std::path::PathBuf;

use anyhow::Result;
use codex_code_nav::Lang;
use codex_code_nav::NavIndex;
use codex_code_nav::find_project_root;
use codex_code_nav::run_query;
use codex_code_nav::run_symbols_for_files;
use codex_code_nav::scan_for_changes;

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  codex-code-nav symbols <path> [lang]");
    eprintln!("  codex-code-nav query <lang> <query> <path>");
    eprintln!();
    eprintln!("Languages: bash, c, cpp, go, javascript, python, rust, typescript");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  codex-code-nav symbols src/main.rs");
    eprintln!("  codex-code-nav symbols src/ rust");
    eprintln!("  codex-code-nav query rust '(function_item name: (identifier) @name)' src/");
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("symbols") => {
            let path: PathBuf = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let lang = args.get(3).and_then(|s| Lang::from_str(s));

            let root = find_project_root(&path);
            let index = NavIndex::open(&root).await?;
            let prefix = path.to_string_lossy().into_owned();
            let cached = index.get_cached_mtimes(&prefix).await?;

            let path_clone = path.clone();
            let (stale, existing) =
                tokio::task::spawn_blocking(move || scan_for_changes(&path_clone, lang, &cached))
                    .await?
                    ?;

            let parsed = tokio::task::spawn_blocking(move || run_symbols_for_files(&stale))
                .await?
                ?;

            for (file, mtime, syms) in &parsed {
                let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
                if let Some(file_lang) = Lang::from_extension(ext) {
                    index.update_file(file, file_lang, *mtime, syms).await?;
                }
            }
            index.remove_deleted_files(&existing).await?;

            let symbols = index.get_symbols_for_prefix(&prefix).await?;
            println!("{}", serde_json::to_string_pretty(&symbols)?);
        }
        Some("query") => {
            let lang_str = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let query_str = args.get(3).map(|s| s.as_str()).unwrap_or("");
            let path: PathBuf = args
                .get(4)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));

            let lang = Lang::from_str(lang_str)
                .ok_or_else(|| anyhow::anyhow!("unknown language: {lang_str}"))?;
            let matches = run_query(query_str, lang, &path)?;
            println!("{}", serde_json::to_string_pretty(&matches)?);
        }
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}
