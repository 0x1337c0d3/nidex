# OpenMonoAgent vs. codex-rs — Comparison & Port Analysis

> Generated 2026-05-11 as input to the Rust migration.
> codex-rs source: `../nidex/codex-rs` (OpenAI's production Rust agent).
> Reference: `RUST_MIGRATION_PLAN.md` at the project root.

---

## Scale & Structure

| Dimension | codex-rs | OpenMonoAgent |
|-----------|----------|---------------|
| Files | 1,139 (Rust) | 164 (C#) |
| Crates / Projects | ~30 Cargo crates | 2 (Cli + Tests) |
| Entry points | 16 `main.rs` binaries | 1 `Program.cs` |
| TUI | ratatui + crossterm | Custom ANSI painter |
| LLM providers | OpenAI Responses API, OpenAI-compat | Anthropic, OpenAI-compat, Ollama, local-llama |
| MCP | Full client **and** server | Client only |
| LSP | No | Yes (`LspClient` + `LspServerManager`) |

codex-rs is a production system; OpenMonoAgent is a clean, well-structured reference implementation. That's the ideal combination for a port.

---

## Architecture — The Core Loop

### codex-rs (actor model)

```
App → Codex::submit(Op) → Submission queue (async_channel)
  → Session::submission_loop (dedicated task)
  → new_turn_with_sub_id() → run_turn() → run_sampling_request()
  → ToolRouter → exec approval / patch approval / request_user_input
  → events emitted back via tx_event (Sender<Event>)
```

The `Session` is an actor. All external input arrives as `Op` messages; all output leaves as `Event` messages. The TUI never touches session internals directly.

### OpenMonoAgent (procedural async loop)

```
ConversationLoop::RunTurnAsync()
  → ILlmClient::StreamChatAsync()
  → accumulate tool calls
  → PermissionEngine::CheckCapabilitiesAsync()
  → ToolDispatcher::ExecuteToolCallsAsync()  // parallel via Task.WhenAll
  → loop until stop_reason = end_turn
```

Simpler to follow, but output events and TUI are more coupled through `IOutputSink`.

### Decision for the port

Adopt the codex-rs actor model: `mpsc` submission queue → session task → `mpsc` event channel to TUI. The `IOutputSink` / `IRenderer` calls scattered through `ConversationLoop` become `tx_output.send(OutputEvent::...)` calls, which the TUI drains separately. This is already called out in `RUST_MIGRATION_PLAN.md` Phase 7 — it's the right call.

---

## What codex-rs Does Better (adopt these)

### `run_sampling_request()` pattern

codex-rs cleanly separates LLM streaming, tool-call accumulation, and the iteration loop into `run_sampling_request()`. Much more explicit than OpenMono's single monolithic `RunTurnAsync`. Study `core/src/codex.rs:3634` before implementing Phase 5.

### `ExecPolicy` / `execpolicy` crate

codex-rs has fine-grained sandbox rules per command (Linux landlock/seccomp, Windows sandbox), not just a binary allow/deny. The `BashParser` + `SanityCheck` in OpenMono is a good start but the codex-rs exec model goes deeper. Worth studying before porting `BashTool`.

### `apply-patch` as a standalone binary

Patch application is tested independently from the session. Good model for the Rust port — pull `ApplyPatchTool` logic into its own crate with unit tests.

### `ContextManager` + rollout persistence

codex-rs persists `RolloutItem` (the full conversation record) separately from the in-memory history. OpenMono's `SessionManager` only snapshots `SessionState`. The rollout pattern is more robust for crash recovery. Consider a richer persistence design in Phase 5.

### Ghost snapshots

`maybe_start_ghost_snapshot` — periodic background snapshots during long turns. OpenMono has no equivalent. Worth adding to the port.

### ratatui TUI already built

`tui/src/app.rs` in codex-rs (~2700 lines) is the most directly useful reference for `openmono-tui`. It handles:
- Multi-thread management
- Model migration prompts
- External editor integration
- Event replay for thread switching
- Approval overlays (exec + patch)

Read it before writing a single line of ratatui code.

---

## What OpenMonoAgent Has That codex-rs Lacks (keep these)

### `DoomLoopDetector`

Explicit cycle detection: normalize + SHA-256 hash last N tool-call argument sets, detect repeats. codex-rs has an iteration cap but not this structured check. Port it — it catches model misbehavior before it wastes tokens.

### `ToolResultCache`

LRU + TTL cache keyed by `sha256(tool_name + normalized_json_input)`. Validates resource state (file mtime/hash) before serving a cached result. codex-rs doesn't have this. Port it — it measurably reduces redundant file reads in long sessions.

### `ArtifactStore`

Content-addressed storage for large tool outputs (SHA-256 keyed, stored under `<data-dir>/<session-id>/artifacts/`). Large content is replaced in the message with a truncated preview + artifact reference. codex-rs handles large outputs via context compaction; this is orthogonal and more precise.

### `CursorStore`

Pagination cursors for large tool results (`FileReadTool`, `GrepTool`). codex-rs has no equivalent. Keeps context window clean for files that span many pages.

### `TurnJournal`

NDJSON append-only audit log with per-tool-call granularity: `TurnStarted`, `ToolCallReceived`, `SchemaValidated/Rejected`, `SanityChecked/Rejected`, `PermissionDecided`, `ToolStarted/Completed/Crashed`, `TurnFinished`. codex-rs has `state_db` but not this level of per-call audit. Invaluable for debugging stuck turns.

### `Checkpointer` (distinct from `Compactor`)

OpenMono has both: `Checkpointer` saves a named restore point (useful for `/checkpoint` command); `Compactor` compresses old context into a summary. codex-rs only has compaction. Port both — they serve different user needs.

### `Playbooks`, `Memory`, `Hooks`

codex-rs has "skills" (markdown-defined slash commands) but not YAML playbooks, a persistent memory store, or a general hook runner. These are differentiated features — port them.

### LSP integration

`LspTool` with hover/definition/references via `LspClient`. codex-rs has no LSP. Port it — it's particularly useful for the Roslyn replacement story.

### `AgentDefinition` + `AgentTool`

Named sub-agent personas with restricted tool lists and injected system prompts. codex-rs has sub-sessions but not named agents. Clean concept, port it.

---

## Tool System Mapping

| OpenMono Tool | codex-rs Equivalent | Port Notes |
|---------------|---------------------|------------|
| `FileReadTool` | `file-search/` crate | Port; include cursor + cache |
| `FileWriteTool` / `FileEditTool` | core tools | Port as-is |
| `ApplyPatchTool` | `apply-patch/` crate | Use codex-rs version as reference — it's more complete |
| `BashTool` + `BashParser` | `exec/` + `execpolicy/` | codex-rs goes deeper — study its exec model |
| `GlobTool`, `GrepTool`, `ListDirectoryTool` | `file-search/` | Port; use `globset` + `ignore` crate |
| `WebFetchTool`, `WebSearchTool` | not in codex-rs | Port as-is (`reqwest` + `scraper`) |
| `TodoTool`, `MemorySaveTool` | not in codex-rs | Port as-is |
| `PlanModeTool`, `AskUserTool` | approvals / `request_user_input` | Map to session event channels |
| `AgentTool` | review thread / sub-sessions | Port OpenMono's simpler version |
| `PlaybookTool`, `LspTool`, `ToolSearchTool` | no equivalent | Port as-is |
| `RoslynTool` | **drop** | Replace with tree-sitter + LSP later |

---

## Permission Model Comparison

**OpenMono — capability-based:**
Each tool declares `RequiredCapabilities()` returning fine-grained types: `FileReadCap`, `ProcessExecCap`, `NetworkEgressCap`, `VcsMutationCap`, etc. `PermissionEngine` matches capabilities against allow/deny rules from config, then prompts for any uncovered ones. Session-level grants (`AllowAllForSession`, `DenyAllForSession`) avoid re-prompting.

**codex-rs — policy-based:**
`ExecPolicyManager` + `SandboxPolicy` with OS-level enforcement (Linux landlock/seccomp, Windows AppContainer). Approval prompts for exec and patch come through `request_command_approval` / `request_patch_approval` channels back to the TUI.

**For the port:** Keep OpenMono's capability system (it's cleaner and cross-platform) but wire the user prompts through channels (oneshot back from TUI) rather than blocking on `IInputReader`. That's what the migration plan Phase 4 prescribes — it's correct.

---

## Crate Layout Recommendation

```
openmono/
├── Cargo.toml                    # workspace
├── crates/
│   ├── openmono-protocol/        # shared types: Op, Event, Message, ToolResult, Capability
│   ├── openmono-core/            # session, tools, permissions, llm, mcp, lsp
│   │   ├── src/session/          # ConversationLoop, SessionManager, Compactor, Checkpointer,
│   │   │                         #   TurnJournal, ArtifactStore, DoomLoopDetector,
│   │   │                         #   ToolResultCache, CursorStore, TokenTracker
│   │   ├── src/tools/            # all Tool impls + ToolRegistry + ToolDispatcher
│   │   ├── src/permissions/      # Capability, PathGuard, PermissionEngine
│   │   ├── src/llm/              # LlmClient trait, AnthropicClient, OpenAiCompatClient,
│   │   │                         #   ProviderRegistry
│   │   └── src/mcp/              # McpClient, McpServerManager, McpToolAdapter
│   ├── openmono-tui/             # ratatui + crossterm (ref: codex-rs/tui/)
│   └── openmono-cli/             # binary: arg parsing, wires everything together
```

This is slightly more collapsed than the plan's 4-crate layout but appropriate for the scale.

---

## Additions to the Migration Plan

The existing `RUST_MIGRATION_PLAN.md` is solid. Add these:

1. **Rollout / crash-recovery persistence** — add a `RolloutItem` model to Phase 5. Don't rely on only JSON session snapshots.
2. **Wire permission prompts as channels** — make the `PermissionEngine → TUI` oneshot protocol explicit in the Phase 4 design before coding starts.
3. **`DoomLoopDetector`, `ToolResultCache`, `ArtifactStore`, `CursorStore` are first-class features** — the plan mentions them briefly; give each an explicit design step in Phase 5.
4. **Read `tui/src/app.rs` in codex-rs before writing any ratatui code** — it's the most directly useful reference available.
5. **`async_trait` decision**: codex-rs uses it throughout. With Rust 1.75+ RPITIT, prefer native `async fn` in traits. Use `async_trait` only where `dyn Trait` object safety requires it (i.e., `dyn LlmClient`, `dyn Tool`).
6. **Ghost snapshots** — add to Phase 9 as a supporting system worth porting.
