# API-Key-Only Auth Refactor Plan

## Goal

Remove all ChatGPT/OAuth authentication paths, leaving only API key auth.
Currently auth supports three modes - API key, ChatGPT (OAuth tokens), and
ChatGPT auth tokens (external provider). Since only API key auth is used,
everything else is dead code.

---

## Overview

Two major removals:

1. **codex-login crate** ("login/") - Device code + local server OAuth flow (~1,000 lines)
2. **ChatGPT/OAuth branches in "core/src/auth.rs"** (~1,500 lines of dead code)

Plus ripple effects across the CLI, TUI, app-server, and test files.

---

## Phase 1: Remove codex-login crate

### 1a. Remove the crate

- Delete "login/" directory entirely (src, tests, Cargo.toml)
- Remove "login" from workspace members in root "Cargo.toml"
- Remove "codex-login" from [workspace.dependencies] in root "Cargo.toml"

### 1b. Update "cli/Cargo.toml"

- Remove "codex-login" from [dependencies]

### 1c. Simplify "cli/src/login.rs"

Keep:
- login_with_api_key()
- read_api_key_from_stdin()
- run_login_with_api_key()
- run_login_status()
- run_logout()
- safe_format_key()
- load_config_or_exit() helper

Delete:
- run_login_with_chatgpt()
- run_login_with_device_code()
- run_login_with_device_code_fallback_to_browser()
- print_login_server_start()
- Import of codex_login::*
- CHATGPT_LOGIN_DISABLED_MESSAGE, API_KEY_LOGIN_DISABLED_MESSAGE, LOGIN_SUCCESS_MESSAGE

### 1d. Simplify "cli/src/main.rs"

- Default "codex login" (no flags) -> call run_login_with_api_key + read_api_key_from_stdin()
- Remove --device-auth, --experimental_issuer, --experimental_client-id from LoginCommand
- Remove use_device_code field
- Remove run_login_with_chatgpt import
- Remove run_login_with_device_code import
- Clean up LoginCommand to only have --with-api-key, --api-key (deprecated), action (status)

### 1e. Simplify TUI onboarding

"tui/src/onboarding/auth.rs":
- Remove ChatGPT-related renders (render_continue_in_browser, render_chatgpt_success_message, render_chatgpt_success)
- Remove start_chatgpt_login() method
- Remove start_device_code_login() method
- Remove handle_existing_chatgpt_login()
- Remove ContinueInBrowserState, ContinueWithDeviceCodeState structs
- Remove SignInOption::ChatGpt, SignInOption::DeviceCode variants
- Remove SignInState variants: ChatGptContinueInBrowser, ChatGptDeviceCode, ChatGptSuccessMessage, ChatGptSuccess
- Remove is_chatgpt_login_allowed() check
- Simplify render_pick_mode to only show API key option
- Remove imports from codex-login
- Remove tests that test ChatGPT flow (keep API key tests)

"tui/src/onboarding/auth/headless_chatgpt_login.rs":
- Delete the entire file

"tui/src/onboarding/onboarding_screen.rs":
- Remove references to deleted auth states

"tui/Cargo.toml":
- Remove codex-login dependency

### 1f. Remove references in TUI status

"tui/src/status/helpers.rs":
- Remove ChatgptAuthTokens-related code

---

## Phase 2: Simplify core/src/auth.rs

### 2a. Remove enum variants and types

- Remove CodexAuth::Chatgpt(ChatgptAuth) variant
- Remove CodexAuth::ChatgptAuthTokens(ChatgptAuthTokens) variant
- Remove ChatgptAuth struct
- Remove ChatgptAuthTokens struct
- Remove ChatgptAuthState struct
- Remove AuthMode::Chatgpt variant (keep AuthMode::ApiKey only)

### 2b. Remove methods from CodexAuth

Remove:
- internal_auth_mode()
- is_chatgpt_auth()
- is_external_chatgpt_tokens()
- get_token_data()
- get_token() - simplify to just return api_key
- get_account_id()
- get_account_email()
- account_plan_type()
- get_current_auth_json()
- get_current_token_data()
- create_dummy_chatgpt_auth_for_testing()

Simplify:
- from_auth_dot_json() - remove non-API-key branches
- from_auth_storage() - remove ephemeral/external auth loading

### 2c. Remove token refresh infrastructure

Remove:
- update_tokens() function
- try_refresh_token() function
- classify_refresh_token_failure() function
- extract_refresh_token_error_code() function
- RefreshRequest struct
- RefreshResponse struct
- CLIENT_ID constant
- refresh_token_endpoint() function
- REFRESH_TOKEN_URL constant
- REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR constant
- TOKEN_REFRESH_INTERVAL constant
- All REFRESH_TOKEN_*_MESSAGE constants

### 2d. Remove UnauthorizedRecovery state machine

Remove:
- UnauthorizedRecovery struct and all methods
- UnauthorizedRecoveryStep enum
- ReloadOutcome enum
- UnauthorizedRecoveryMode enum

### 2e. Remove ExternalAuthRefresher trait

Remove:
- ExternalAuthRefresher trait
- ExternalAuthTokens struct
- ExternalAuthRefreshContext struct
- ExternalAuthRefreshReason enum

### 2f. Remove from AuthManager

Remove:
- refresh_token()
- refresh_if_stale()
- refresh_external_auth()
- refresh_tokens()
- reload_if_account_id_matches()
- set_external_auth_refresher()
- has_external_auth_refresher()
- is_external_auth_active()
- set_forced_chatgpt_workspace_id()
- forced_chatgpt_workspace_id()
- unauthorized_recovery()
- get_internal_auth_mode()

Simplify:
- auth() - remove refresh_if_stale call
- reload() - simplify

Keep:
- new(), shared(), auth_cached(), auth(), reload(), logout(), get_auth_mode()
- from_auth_for_testing(), from_auth_for_testing_with_home()

### 2g. Clean up public re-exports in core/src/lib.rs

- Remove re-exports that no longer exist

### 2h. Remove ChatgptAuth impl block

- Remove entire impl ChatgptAuth { ... } block

### 2i. Simplify load_auth() private function

- Remove ephemeral storage path
- Simplify to: env var -> file storage -> None

### 2j. Simplify enforce_login_restrictions()

- Remove workspace/account-id checking logic
- Keep as a no-op or remove entirely

### 2k. Simplify AuthDotJson methods

- from_external_tokens() - remove
- from_external_token_strings() - remove
- resolved_mode() - simplify
- storage_mode() - remove (unused)

---

## Phase 3: Fix ripple effects in core/src/client.rs

- Remove UnauthorizedRecovery import
- Remove RefreshTokenError import
- Simplify handle_unauthorized() - just return Err(map_unauthorized_status(status))
- Remove auth_recovery loop from all stream methods
- Remove is_chatgpt_auth check in responses_request_compression()

---

## Phase 4: Fix ripple effects in other core files

| File | Action |
|------|--------|
| core/src/codex.rs | Remove enforce_login_restrictions() call |
| core/src/analytics_client.rs | Remove is_chatgpt_auth() check |
| core/src/codex_delegate.rs | Remove is_chatgpt_auth() check |
| core/src/error.rs | Remove RefreshTokenFailedError, RefreshTokenFailedReason |
| core/src/env.rs | Check for ChatGPT auth references |
| core/src/model_provider_info.rs | Remove unused auth_mode variable |

---

## Phase 5: Update test files

| File | Action |
|------|--------|
| core/tests/suite/auth_refresh.rs | Delete entirely |
| core/src/auth.rs tests | Remove ChatGPT auth path tests |
| core/src/auth/storage.rs tests | Keep all storage tests |
| login/tests/ | Delete (goes with login crate) |

---

## Phase 6: Update tui/src/chatwidget.rs

- Remove account_plan_type() call (line ~808)
- Handle simplification of what gets displayed

---

## Phase 7: Update app-server references

| File | Action |
|------|--------|
| app-server/src/codex_message_processor.rs | Remove ChatgptAuthTokens references |
| app-server/tests/common/auth_fixtures.rs | Remove ChatGPT auth fixtures |
| app-server/tests/suite/auth.rs | Remove ChatGPT-specific tests |
| app-server/tests/suite/v2/account.rs | Clean up |
| app-server/src/message_processor.rs | Remove ExternalAuth-related code |

---

## Phase 8: Final cleanup

- Run cargo check, cargo test to verify
- Run cargo clippy to catch any missed dead code
- Remove codex-login from Cargo.lock (will happen on next cargo update)
- Update any remaining docs/references

---

## Estimated scope

| Metric | Count |
|--------|-------|
| Files modified | ~20-25 |
| Files deleted | ~5 |
| Lines removed (net) | ~2,500-3,000 |
| crates removed | 1 (codex-login) |

---

## Order of execution

Phase 1 (login crate) -> Phase 2 (core/auth.rs) -> Phases 3-7 (parallel once Phase 2 is stable) -> Phase 5-6 -> Phase 8
