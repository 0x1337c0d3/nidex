# ACP Compliance Plan

Reference schema: https://agentclientprotocol.com/protocol/schema.md

## Background

codex-app-server speaks JSON-RPC 2.0 and already implements the core ACP
happy path (`initialize` → `session/new` → `session/prompt` / `session/cancel`).
However three critical gaps prevent a standard ACP client (e.g. Zed) from working
correctly. This document tracks the fixes in priority order.

---

## Gap 1 — `initialize` request params mismatch  *(high priority)*

### Problem

ACP `initialize` request params:
```json
{
  "protocolVersion": "2025-05-12",   // required string
  "clientCapabilities": { ... },     // optional, field name differs
  "clientInfo": { ... }              // optional
}
```

Codex `v1::InitializeParams` (`app-server-protocol/src/protocol/v1.rs:34`):
- Has no `protocolVersion` field — the server ignores whatever the client sends.
- Uses `capabilities` instead of `clientCapabilities`.
- Treats `client_info` as **required** whereas ACP makes it optional.

### Fix

1. Add `protocol_version: Option<String>` to `InitializeParams` (tolerate absence for
   legacy clients).
2. Rename `capabilities` → `client_capabilities`, keeping a `#[serde(alias)]` for
   backwards compatibility.
3. Make `client_info` optional (`Option<ClientInfo>`).
4. In `message_processor.rs` `handle_initialize`, echo back the client's
   `protocolVersion` in the response (or use the server's supported version if absent).

**Files touched:**
- `app-server-protocol/src/protocol/v1.rs`
- `app-server/src/message_processor.rs`

---

## Gap 2 — `session/new` params mismatch  *(high priority)*

### Problem

ACP `session/new` request params:
```json
{
  "cwd": "/path/to/workspace",   // required string
  "mcpServers": []               // required array
}
```

Codex maps `session/new` to `v2::ThreadStartParams`
(`app-server-protocol/src/protocol/common.rs:182`) which has many custom fields
and no `mcpServers`. An ACP-compliant client sending only `{ cwd, mcpServers }`
will fail to deserialize or silently lose the `cwd`.

### Fix

1. Create a new `AcpSessionNewParams` struct in `v1.rs` with `cwd: String` and
   `mcp_servers: Vec<serde_json::Value>` (opaque for now).
2. Change the `SessionNew` variant in `client_request_definitions!` to use
   `AcpSessionNewParams` instead of `v2::ThreadStartParams`.
3. In `codex_message_processor.rs`, map `AcpSessionNewParams` → `ThreadStartParams`,
   forwarding `cwd` and ignoring/storing `mcp_servers` for future use.

**Files touched:**
- `app-server-protocol/src/protocol/v1.rs`
- `app-server-protocol/src/protocol/common.rs`
- `app-server/src/codex_message_processor.rs`

---

## Gap 3 — `session/update` notification not implemented  *(high priority)*

### Problem

ACP clients expect a `session/update` notification (server→client) for all
real-time progress. The `SessionUpdate` payload is a tagged union:

```
SessionUpdate =
  | { type: "agentMessage", role: "assistant", content: ContentBlock[] }
  | { type: "toolCall", toolCall: ToolCallUpdate }
  | { type: "plan", plan: Plan }
  | { type: "error", error: string }
```

Codex uses entirely custom notification names (`turn/started`,
`item/agentMessage/delta`, `item/commandExecution/outputDelta`, etc.) that
ACP clients don't understand.

### Fix

1. Define `SessionUpdatePayload` enum in `v1.rs` covering the four variants above.
2. Add a `SessionUpdate` variant to `ServerNotification` with wire name
   `"session/update"`.
3. In the turn event pipeline (`codex_message_processor.rs`), translate outgoing
   codex-specific events into `session/update` notifications in addition to (or
   instead of) the existing ones. Initially, map:
   - `AgentMessageDelta` → `session/update { type: "agentMessage", ... }`
   - `CommandExecutionRequestApproval` → `session/update { type: "toolCall", ... }`
   - `TurnCompleted` with error → `session/update { type: "error", ... }`
4. Keep existing notifications emitting so legacy clients aren't broken (dual-emit).

**Files touched:**
- `app-server-protocol/src/protocol/v1.rs`
- `app-server-protocol/src/protocol/common.rs`
- `app-server/src/codex_message_processor.rs`

---

## Gap 4 — `session/request_permission` not implemented  *(medium priority)*

### Problem

ACP requires the server to call `session/request_permission` (a server→client
**request**, expecting a response) when it needs user approval:

```json
{
  "sessionId": "...",
  "toolCall": { "id": "...", "name": "...", "input": {...} },
  "options": [
    { "label": "Allow", "kind": "allow_once" },
    { "label": "Deny",  "kind": "reject_once" }
  ]
}
```

Response: `{ "outcome": { "type": "option", "kind": "allow_once" } }`

Codex uses the non-standard `ApplyPatchApproval` and `ExecCommandApproval`
server→client requests instead.

### Fix

1. Define `AcpRequestPermissionParams` and `AcpRequestPermissionResponse` types
   in `v1.rs`.
2. Add a `SessionRequestPermission` variant to `ServerRequest` with wire name
   `"session/request_permission"`.
3. In `codex_message_processor.rs`, when an approval is required for a turn
   started via `session/prompt`, use `session/request_permission` instead of
   (or in addition to) the legacy approval requests.
4. Map the ACP `PermissionOptionKind` (allow_once / allow_always / reject_once /
   reject_always) back to the internal `ReviewDecision`.

**Files touched:**
- `app-server-protocol/src/protocol/v1.rs`
- `app-server-protocol/src/protocol/common.rs`
- `app-server/src/codex_message_processor.rs`

---

## Gap 5 — Optional session lifecycle methods  *(low priority)*

The following ACP methods are optional if not advertised in `agentCapabilities`:

| Method | Notes |
|--------|-------|
| `session/load` | Resume with history replay; map to `thread/resume` |
| `session/close` | Terminate session; map to dropping the thread |
| `session/list` | Enumerate sessions; map to `thread/list` |
| `session/resume` | Resume without history replay |
| `session/set_config_option` | Runtime config changes |
| `session/set_mode` | Switch agent mode (ask/code/architect) |
| `authenticate` | Pre-session auth handshake |

The server currently correctly advertises `loadSession: false` so these can be
added incrementally. Implement each one, then flip the corresponding capability
flag to `true`.

---

## Gap 6 — Terminal methods  *(low priority)*

ACP defines five terminal methods (`terminal/create`, `terminal/output`,
`terminal/kill`, `terminal/wait_for_exit`, `terminal/release`) that the server
can call on the client for delegated command execution. These are only required
if `clientCapabilities.terminal = true`.

The server currently never reads `clientCapabilities`, so it never knows whether
the client supports terminal delegation.

### Fix (future)

1. Parse `clientCapabilities.terminal` from the `initialize` request.
2. If true, implement the five terminal server→client requests.
3. Route command execution through client-side terminals when the capability is
   present.

---

## Implementation Order

```
Gap 1  →  Gap 2  →  Gap 3  →  Gap 4  →  Gap 5  →  Gap 6
(params)  (new)    (update)   (perm)    (lifecycle) (terminal)
```

Gaps 1–3 together unblock end-to-end testing with a real ACP client.
Gap 4 unblocks approval flows. Gaps 5–6 are stretch goals.

---

## Testing Strategy

- Add / extend tests in `app-server/tests/suite/v2/initialize.rs` for the
  updated `initialize` request/response shapes.
- Add a new test file `app-server/tests/suite/v2/session_acp.rs` covering
  `session/new`, `session/prompt`, and `session/update` end-to-end.
- Use the existing mock model server infrastructure to drive turns and assert
  that `session/update` notifications are emitted with correct payloads.
