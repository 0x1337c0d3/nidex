# Project Rename Plan: OpenAI Codex → Nidex

**Date:** 2026-05-03
**Scope:** Full rebranding of the codex-rs Rust workspace and all references

---

## Summary

The project currently references "Codex" and "OpenAI" in ~1,466 locations across Rust code, TOML files, documentation, tests, and configuration. This plan systematically renames all occurrences while maintaining functional equivalence.

---

## Scope Overview

| Category | Count | Examples |
|----------|-------|----------|
| Rust crates (workspace) | ~48 | `codex-cli` → `nidex-cli` |
| Binary names | ~8 | `codex` → `nidex` |
| Crate path dependencies | 246+ | `codex-core` workspace deps |
| Code identifiers | ~1,466 | `CodexError`, `CODEX_HOME`, `OPENAI_BASE_URL` |
| External packages (npm) | 2 | `@openai/codex`, `@openai/codex-responses-api-proxy` |
| URL references | 100+ | api.openai.com, github.com/openai, developers.openai.com |
| Documentation | 10+ files | README.md, docs/*.md |

---

## Phase 1: Core Package Renames

### 1.1 Workspace Cargo.toml

**File:** `Cargo.toml`

Changes:
```toml
# Members list - add:
# NOTE: No workspace members need renaming; directory names stay the same.
# Only the published crate names change.

# Workspace dependencies - prefix changes:
codex-ansi-escape → nidex-ansi-escape
codex-api → nidex-api
codex-app-server → nidex-app-server
codex-app-server-protocol → nidex-app-server-protocol
# ... (all 48 crates)
```

### 1.2 All Crate Cargo.toml Files

**Files:** Each crate's `Cargo.toml` (48 crates)

Pattern:
- `name = "codex-*"` → `name = "nidex-*"`
- `name = "codex_*"` → `name = "nidex_*"`

Key crates requiring special handling:
- `cli/Cargo.toml`: binary name `codex` → `nidex`
- `cli/Cargo.toml`: lib name `codex_cli` → `nidex_cli`
- `windows-sandbox-rs/Cargo.toml`: 4 binaries (`codex-command-runner`, `codex-windows-sandbox`, etc.)
- `exec-server/Cargo.toml`: 3 binaries (`codex-exec-server`, `codex-exec-mcp-server`, `codex-execve-wrapper`)

### 1.3 Rename Internal Path Dependencies

After crate name changes, update all `codex-*` → `nidex-*` references in:
- `[dependencies]` sections
- `[dev-dependencies]` sections
- `workspace = true` references

---

## Phase 2: Binary and Executable Renames

### 2.1 CLI Binary

**Files:** `cli/Cargo.toml`, `cli/src/main.rs`

Changes:
```toml
[[bin]]
name = "codex" → name = "nidex"
path = "src/main.rs"
```

CLI entry point updates in `cli/src/main.rs`:
- Binary name display
- Help text references
- Error messages

### 2.2 Windows Sandbox Binaries

**Files:** `windows-sandbox-rs/Cargo.toml`

```toml
# Current:
name = "codex-command-runner"
name = "codex-windows-sandbox-setup"
name = "codex-windows-sandbox"

# New:
name = "nidex-command-runner"
name = "nidex-windows-sandbox-setup"
name = "nidex-windows-sandbox"
```

### 2.3 Exec Server Binaries

**Files:** `exec-server/Cargo.toml`

```toml
# Current:
name = "codex-exec-server"
name = "codex-exec-mcp-server"
name = "codex-execve-wrapper"

# New:
name = "nidex-exec-server"
name = "nidex-exec-mcp-server"
name = "nidex-execve-wrapper"
```

---

## Phase 3: Environment Variables and Config Keys

### 3.1 Core Environment Variables

| Current | New | Location |
|---------|-----|----------|
| `CODEX_HOME` | `NIDEX_HOME` | `utils/home-dir/src/lib.rs`, `rmcp-client/src/oauth.rs`, `core/config/mod.rs` |
| `CODEX_ESCALATE_SOCKET` | `NIDEX_ESCALATE_SOCKET` | `exec-server/README.md`, exec-server code |
| `OPENAI_BASE_URL` | `NIDEX_BASE_URL` | `codex-api/`, `core/`, `tui/`, tests |
| `OPENAI_API_KEY` | `NIDEX_API_KEY` | Config handling |
| `CODEX_TELEMETRY_ENABLED` | `NIDEX_TELEMETRY_ENABLED` | Config |

### 3.2 Config Schema

**Files:** `core/config.schema.json`, `core/src/config/mod.rs`

- `base_url` default: `https://api.openai.com/v1` → `https://api.nidex.ai/v1`
- All `codex.*` config keys → `nidex.*` (if namespace used)

### 3.3 Keyring Service Name

**File:** `rmcp-client/src/oauth.rs`

```rust
const KEYRING_SERVICE: &str = "Codex MCP Credentials";
// → "Nidex MCP Credentials"
```

---

## Phase 4: API Endpoints and URLs

### 4.1 OpenAI API URLs

| Current | New | Notes |
|---------|-----|-------|
| `https://api.openai.com/v1` | `https://api.nidex.ai/v1` | Default base_url |
| `https://auth.openai.com/oauth/token` | `https://auth.nidex.ai/oauth/token` | Token refresh endpoint |
| `https://api.openai.com/auth` | `https://api.nidex.ai/auth` | Auth headers |

**Files:** `core/src/auth.rs`, `codex-api/src/requests/chat.rs`, `core/src/config/mod.rs`, `login/src/server.rs`, and ~20 more.

### 4.2 Domain References

| Pattern | Replacement | Scope |
|---------|-------------|-------|
| `*.openai.com` | `*.nidex.ai` | Config, tests, network-proxy |
| `api.openai.com` | `api.nidex.ai` | Runtime code |
| `chat.openai.com` | `chat.nidex.ai` | MCP server logic |
| `platform.openai.com` | `platform.nidex.ai` | Login flow |

**Files:** `network-proxy/src/runtime.rs`, `login/src/server.rs`, `core/src/mcp/mod.rs`, `core/src/client_common.rs`, `protocol/src/`

### 4.3 Documentation URLs

| Current | New |
|---------|-----|
| `https://github.com/openai/codex` | `https://github.com/nidex/nidex` |
| `https://developers.openai.com/codex/*` | `https://developers.nidex.ai/nidex/*` |
| `https://github.com/openai/codex/issues/*` | `https://github.com/nidex/nidex/issues/*` |
| `https://github.com/openai/codex/pull/*` | `https://github.com/nidex/nidex/pull/*` |

**Files:** All `.md` files, Rust doc comments, test fixtures.

---

## Phase 5: NPM Package Renames

### 5.1 Package Names

| Current | New |
|---------|-----|
| `@openai/codex` | `@nidex/nidex` |
| `@openai/codex-responses-api-proxy` | `@nidex/nidex-responses-api-proxy` |

**Files:** 
- `responses-api-proxy/npm/package.json`
- `responses-api-proxy/npm/README.md`
- Installation instructions in `tui/src/update_action.rs`

### 5.2 Package URLs

**File:** `responses-api-proxy/npm/package.json`

```json
{
  "repository": {
    "url": "git+https://github.com/nidex/nidex.git"
  }
}
```

---

## Phase 6: Code Identifiers

### 6.1 Type and Struct Names

| Current | New | Location |
|---------|-----|----------|
| `CodexError` | `NidexError` | Various crates |
| `CodexResult<T>` | `NidexResult<T>` | Various crates |
| `CodexAuth` | `NidexAuth` | auth modules |
| `CodexSession` | `NidexSession` | core/session |
| `CodexClient` | `NidexClient` | client crates |
| `CodexTui` | `NidexTui` | tui crate |
| `CodexMcpServer` | `NidexMcpServer` | mcp-server |
| `TokenData` fields | Update `api.openai.com/auth` → `api.nidex.ai/auth` | `core/src/token_data.rs` |

### 6.2 Module Names

Search for `pub mod codex`, `pub use codex_`, `use codex_` patterns.

### 6.3 Constants

| Current | New | Location |
|---------|-----|----------|
| `DEFAULT_OPENAI_BASE_URL` | `DEFAULT_NIDEX_BASE_URL` | `tui/src/chatwidget.rs` |
| `KEYRING_SERVICE = "Codex MCP Credentials"` | `"Nidex MCP Credentials"` | `rmcp-client/src/oauth.rs` |

---

## Phase 7: Documentation Updates

### 7.1 Root Documentation

**Files:** `README.md`, `AGENTS.md`, `CLAUDE.md`, `plan.md`

- Project name references
- Installation instructions (`npm i -g @openai/codex` → `npm i -g @nidex/nidex`)
- GitHub URLs
- Brew installation references

### 7.2 Docs Directory

**Files:** `docs/*.md`

Update all references to:
- Project name
- GitHub URLs
- External links
- Configuration examples

### 7.3 In-Code Documentation

Rust doc comments with Codex/OpenAI references:
- `cli/src/main.rs`
- `exec-server/README.md`
- `responses-api-proxy/README.md`
- `network-proxy/README.md`
- Various `/// See https://...` comments

---

## Phase 8: Test Fixtures and Test Data

### 8.1 Auth Fixtures

**Files:** `app-server/tests/common/auth_fixtures.rs`, `core/tests/common/responses.rs`, `login/tests/suite/*`, `core/tests/suite/client.rs`

Update:
- Auth endpoint URLs
- Token refresh URLs
- Response body URLs

### 8.2 Network Tests

**Files:** `network-proxy/src/runtime.rs`, `linux-sandbox/tests/suite/landlock.rs`

Update:
- `openai.com` references in blocked-host tests
- Domain allow/deny patterns

### 8.3 Test Binaries

**File:** `rmcp-client/tests/resources.rs`

- URI templates: `memo://codex/*` → `memo://nidex/*`
- Test binary names: `codex-test` → `nidex-test`
- Resource names containing "Codex"

---

## Phase 9: Third-Party Skill References

### 9.1 Skill Installer Sample

**File:** `core/src/skills/assets/samples/skill-installer/SKILL.md`

URLs:
- `https://github.com/openai/skills/tree/main/skills/.curated` → `https://github.com/nidex/skills/tree/main/skills/.curated`
- `https://github.com/openai/skills/tree/main/skills/.system`
- `https://github.com/openai/skills/tree/main/skills/.experimental`

---

## Execution Order

1. **Phase 5 (NPM)**: Rename npm packages first if there's a registry impact
2. **Phase 1 (Workspace)**: Update root `Cargo.toml` workspace members and workspace dependencies
3. **Phase 2 (Binaries)**: Update binary names in `Cargo.toml` files
4. **Phase 3 (Environment)**: Update env vars and config defaults
5. **Phase 4 (URLs)**: Update API endpoints and domains
6. **Phase 6 (Code IDs)**: Rename types, constants, modules
7. **Phase 7 (Docs)**: Update all documentation
8. **Phase 8 (Tests)**: Update test fixtures and assertions
9. **Phase 9 (Skills)**: Update skill references

---

## Risk Mitigation

### High-Risk Areas
1. **Auth flow**: Changing `api.openai.com` → `api.nidex.ai` requires corresponding backend support
2. **Token refresh**: `auth.openai.com` → `auth.nidex.ai` requires OAuth provider update
3. **NPM packages**: Package name changes affect users' installation scripts

### Recommended Approach
- Use search-and-replace with careful verification
- Run `cargo check` frequently during implementation
- Test auth flow end-to-end after changes
- Provide migration guide for users (CODEX_HOME → NIDEX_HOME)

---

## Verification Checklist

- [ ] `cargo build` passes for all crates
- [ ] `cargo test` passes (all integration tests)
- [ ] CLI binary `nidex --help` works
- [ ] Auth flow (login/logout) functional
- [ ] MCP server starts correctly
- [ ] TUI launches without errors
- [ ] Config schema validates
- [ ] No remaining `codex-*` crate references
- [ ] No remaining `openai.com` in runtime paths (tests excluded)

---

## Post-Rename Cleanup

1. Update `.gitignore` if any new temp files created
2. Update any CI/CD configurations referencing old names
3. Update any shell completions or CLI helpers
4. Tag release as `v1.0.0-nidex` or similar
