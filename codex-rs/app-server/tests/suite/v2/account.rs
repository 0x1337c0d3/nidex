//! Account-related integration tests for the v2 protocol.
//! These tests cover the account/logout and account/get endpoints.

use anyhow::Result;
use app_test_support::McpProcess;
use app_test_support::to_response;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::LogoutAccountResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_core::auth::AuthCredentialsStoreMode;
use codex_core::auth::login_with_api_key;
use pretty_assertions::assert_eq;
use std::path::Path;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn create_config_toml(codex_home: &Path) -> std::io::Result<()> {
    let config_toml = codex_home.join("config.toml");
    std::fs::write(
        config_toml,
        r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "http://127.0.0.1:0/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#,
    )
}

#[tokio::test]
async fn logout_account_removes_auth_and_notifies() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path())?;

    login_with_api_key(
        codex_home.path(),
        "sk-test-key",
        AuthCredentialsStoreMode::File,
    )?;
    assert!(codex_home.path().join("auth.json").exists());

    let mut mcp = McpProcess::new_with_env(codex_home.path(), &[("OPENAI_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let id = mcp.send_logout_account_request().await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(id)),
    )
    .await??;
    let _ok: LogoutAccountResponse = to_response(resp)?;

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountUpdated(payload) = parsed else {
        anyhow::bail!("unexpected notification: {parsed:?}");
    };
    assert_eq!(payload.auth_mode, None);

    assert!(
        !codex_home.path().join("auth.json").exists(),
        "auth.json should be deleted after logout"
    );

    let get_id = mcp
        .send_get_account_request(GetAccountParams {
            refresh_token: false,
        })
        .await?;
    let get_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(get_id)),
    )
    .await??;
    let account: GetAccountResponse = to_response(get_resp)?;
    assert_eq!(account.account, None);
    Ok(())
}

#[tokio::test]
async fn get_account_no_auth() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path())?;

    let mut mcp = McpProcess::new_with_env(codex_home.path(), &[("OPENAI_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let get_id = mcp
        .send_get_account_request(GetAccountParams {
            refresh_token: false,
        })
        .await?;
    let get_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(get_id)),
    )
    .await??;
    let account: GetAccountResponse = to_response(get_resp)?;
    assert_eq!(
        account,
        GetAccountResponse {
            account: None,
            requires_openai_auth: false,
        }
    );

    Ok(())
}
