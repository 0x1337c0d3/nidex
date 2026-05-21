use codex_code_nav::Lang;
use codex_code_nav::NavIndex;
use codex_code_nav::find_project_root;
use codex_code_nav::run_symbols_for_files;
use codex_code_nav::scan_for_changes;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct CodeSymbolsHandler;

#[derive(Deserialize)]
struct CodeSymbolsArgs {
    path: Option<String>,
    lang: Option<String>,
}

#[async_trait::async_trait]
impl ToolHandler for CodeSymbolsHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "code_symbols handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: CodeSymbolsArgs = parse_arguments(&arguments)?;

        let search_path = turn.resolve_path(args.path);
        let lang_filter = args.lang.as_deref().and_then(Lang::from_str);

        // Step 1: find project root (git root or fallback to search path).
        let root = find_project_root(&search_path);

        // Step 2: open (or create) the index.
        let index = NavIndex::open(&root).await.map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to open code-nav index: {e}"))
        })?;

        // Step 3: get cached mtimes for the search prefix.
        let prefix = search_path.to_string_lossy().into_owned();
        let cached_mtimes = index
            .get_cached_mtimes(&prefix)
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("failed to read index mtimes: {e}"))
            })?;

        // Step 4 & 5: walk the directory and parse stale files (sync CPU work).
        let search_path_clone = search_path.clone();
        let (freshly_parsed, existing_paths) =
            tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                let (stale, existing) =
                    scan_for_changes(&search_path_clone, lang_filter, &cached_mtimes)?;
                let parsed = run_symbols_for_files(&stale)?;
                Ok((parsed, existing))
            })
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("code_symbols task panicked: {e}"))
            })?
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("code_symbols scan failed: {e}"))
            })?;

        // Step 6: update index for each re-parsed file.
        for (file_path, mtime, symbols) in &freshly_parsed {
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if let Some(lang) = Lang::from_extension(ext) {
                index
                    .update_file(file_path, lang, *mtime, symbols)
                    .await
                    .map_err(|e| {
                        FunctionCallError::RespondToModel(format!("index update failed: {e}"))
                    })?;
            }
        }

        // Step 7: prune deleted files.
        index
            .remove_deleted_files(&existing_paths)
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("index prune failed: {e}"))
            })?;

        // Step 8: return all symbols from the index for the search prefix.
        let symbols = index
            .get_symbols_for_prefix(&prefix)
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("failed to query index: {e}"))
            })?;

        if symbols.is_empty() {
            return Ok(ToolOutput::Function {
                content: "No symbols found.".to_string(),
                content_items: None,
                success: Some(false),
            });
        }

        let content = serde_json::to_string_pretty(&symbols).map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to serialize symbols: {e}"))
        })?;

        Ok(ToolOutput::Function {
            content,
            content_items: None,
            success: Some(true),
        })
    }
}
