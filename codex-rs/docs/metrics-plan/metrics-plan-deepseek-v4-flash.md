# Plan: Tokens Per Second Metrics

## Goal
Add tokens-per-second (TPS) metrics to the codex-rs codebase, showing how fast tokens are generated during model inference. Display TPS in the `/status` card and the turn separator line.

---

## 1. Add `duration_ms` to `TokenUsageInfo`

**Files:** `protocol/src/protocol.rs`

- Add a `duration_ms: Option<u64>` field to `TokenUsageInfo`
- This represents the wall-clock time spent generating the `last_token_usage` output tokens
- The `new_or_append()` method should accept an optional `duration_ms` parameter
- `append_last_usage()` updates this field each turn

```rust
// protocol/src/protocol.rs — TokenUsageInfo
pub struct TokenUsageInfo {
    pub total_token_usage: TokenUsage,
    pub last_token_usage: TokenUsage,
    pub duration_ms: Option<u64>,         // <-- new
    pub model_context_window: Option<i64>,
}
```

---

## 2. Compute Duration in the Core During Turn Execution

**Files:** `core/src/codex.rs`

- When `TurnStartedEvent` is emitted (`core/src/codex.rs:3246`), record an `Instant::now()` in the session state or turn context
- When `update_token_usage_info()` is called (line 1904), compute `elapsed_ms` from that `Instant` and pass it through to `TokenUsageInfo::new_or_append()`
- Store the `Instant` in `SessionState` with a new field `turn_started_at: Option<Instant>`

### Changes to `SessionState`

**Files:** `core/src/state/session.rs`

- Add `pub(crate) fn set_turn_started_at(&mut self)` and `pub(crate) fn turn_elapsed_ms(&self) -> Option<u64>`

### Changes to `codex.rs` turn loop

- Call `state.set_turn_started_at()` at the `TurnStartedEvent` point
- Call `state.turn_elapsed_ms()` in `update_token_usage_info()` and pass to `TokenUsageInfo`

---

## 3. Expose TPS in the Protocol

**Files:** `protocol/src/protocol.rs` — `TokenUsageInfo`

- Add a computed method `tokens_per_second(&self) -> Option<f64>` that divides `last_token_usage.output_tokens` by `duration_ms / 1000.0`
- (Or compute at the display layer — either works)

---

## 4. Display TPS in the `/status` Card

**Files:** `tui/src/status/card.rs`

- Add a `tokens_per_second: Option<f64>` field to `StatusTokenUsageData`
- Populate it from `TokenUsageInfo` in `StatusHistoryCell::new()`
- Append a line like `"Token usage: 1.2K total (800 input + 400 output)  —  12.5 tok/s"` in `token_usage_spans()`
- Show TPS after the output count: `Span::from("  —  12.5 tok/s")`

---

## 5. Display TPS in Turn Separator

**Files:** `tui/src/history_cell.rs` — `FinalMessageSeparator`

- Add an `output_tokens` field to `FinalMessageSeparator`
- In `display_lines()`, if both `output_tokens` > 0 and `elapsed_seconds` > 0, compute and show TPS:
  - Example: `"─ Worked for 2m 34s • Generated 1,500 tokens (9.7 tok/s) ─"`

---

## 6. Plumb TPS Data to the TUI

**Files:** `tui/src/chatwidget.rs`

- In `apply_token_info()`, extract `duration_ms` and store it as `last_turn_duration_ms: Option<u64>`
- Expose a method `output_tokens_last_turn(&self) -> i64`
- When creating `FinalMessageSeparator` in `on_task_complete()`, pass the output token count and TPS from stored `token_info`

---

## 7. Optional: Include TPS in `FinalOutput` Display

**Files:** `tui/src/app.rs` (around lines 135-145)

- When rendering the final usage line at session end, append TPS if available and tokens > 0

---

## Data Flow Summary

```
TurnStartedEvent
  └→ SessionState.turn_started_at = Instant::now()

ResponseEvent::Completed (with TokenUsage)
  └→ update_token_usage_info()
       └→ turn_elapsed_ms() → TokenUsageInfo.duration_ms
            └→ send_token_count_event()
                 └→ EventMsg::TokenCount(TokenCountEvent { info })

TUI receives TokenCountEvent
  └→ chatwidget.set_token_info(info)
       └→ status card shows TPS
       └→ turn separator shows "X tok/s"

FinalOutput at session end
  └→ app.rs renders TPS in final line
```
