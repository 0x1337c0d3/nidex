use async_trait::async_trait;
use codex_code_nav::NavIndex;
use codex_code_nav::find_project_root;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct CodeNavInitHandler;

#[derive(Deserialize)]
struct CodeNavInitArgs {
    #[serde(default)]
    reset: bool,
}

#[async_trait]
impl ToolHandler for CodeNavInitHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "code_nav_init handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: CodeNavInitArgs = parse_arguments(&arguments)?;
        let cwd = turn.resolve_path(None);
        let root = find_project_root(&cwd);

        if args.reset {
            NavIndex::reset(&root).await.map_err(|e| {
                FunctionCallError::RespondToModel(format!("code_nav_init reset failed: {e}"))
            })?;
        }

        NavIndex::warm(&cwd, None).await.map_err(|e| {
            FunctionCallError::RespondToModel(format!("code_nav_init warm failed: {e}"))
        })?;

        let msg = if args.reset {
            "Index reset and rebuilt successfully.".to_string()
        } else {
            "Index is up to date.".to_string()
        };

        Ok(ToolOutput::Function {
            content: msg,
            content_items: None,
            success: Some(true),
        })
    }
}
