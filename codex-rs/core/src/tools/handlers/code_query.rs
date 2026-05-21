use async_trait::async_trait;
use codex_code_nav::Lang;
use codex_code_nav::run_query;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct CodeQueryHandler;

#[derive(Deserialize)]
struct CodeQueryArgs {
    query: String,
    lang: String,
    path: Option<String>,
}

#[async_trait]
impl ToolHandler for CodeQueryHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "code_query handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: CodeQueryArgs = parse_arguments(&arguments)?;

        let lang = Lang::from_str(&args.lang).ok_or_else(|| {
            FunctionCallError::RespondToModel(format!(
                "unknown language {:?}; supported: bash, c, cpp, go, javascript, python, rust, typescript",
                args.lang
            ))
        })?;

        let search_path = turn.resolve_path(args.path);
        let query_str = args.query.clone();

        let matches =
            tokio::task::spawn_blocking(move || run_query(&query_str, lang, &search_path))
                .await
                .map_err(|err| {
                    FunctionCallError::RespondToModel(format!("code_query task panicked: {err}"))
                })?
                .map_err(|err| {
                    FunctionCallError::RespondToModel(format!("code_query failed: {err}"))
                })?;

        if matches.is_empty() {
            return Ok(ToolOutput::Function {
                content: "No matches found.".to_string(),
                content_items: None,
                success: Some(false),
            });
        }

        let content = serde_json::to_string_pretty(&matches).map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to serialize matches: {err}"))
        })?;

        Ok(ToolOutput::Function {
            content,
            content_items: None,
            success: Some(true),
        })
    }
}
