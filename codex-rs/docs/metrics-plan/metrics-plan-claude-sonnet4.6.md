# Plan: Tokens Per Second Metrics in codex-rs

## Context

Users have no visibility into how fast the model is generating tokens during a turn. Adding a tokens-per-second (TPS) metric surfaces generation speed at the end of each turn (in the `FinalMessageSeparator` divider) and in the `./docs` directory as a design document. This is useful for benchmarking models and catching regressions in latency.

---

## Approach

Compute TPS per-turn in the TUI by snapshotting output token count at turn start, then dividing the delta by elapsed wall-clock seconds at turn completion. Inject the result into the existing `RuntimeMetricsSummary` struct so it flows naturally to the existing `FinalMessageSeparator` display pipeline with no new plumbing.

---

## Implementation Steps

### Step 1 — Extend `RuntimeMetricsSummary`
**File:** `otel/src/metrics/runtime_metrics.rs`

Add an optional field to the struct (lines 28–35):
```rust
pub struct RuntimeMetricsSummary {
    pub tool_calls: RuntimeMetricTotals,
    pub api_calls: RuntimeMetricTotals,
    pub streaming_events: RuntimeMetricTotals,
    pub websocket_calls: RuntimeMetricTotals,
    pub websocket_events: RuntimeMetricTotals,
    pub tokens_per_second: Option<f64>,   // ← add this
}
```
Update `is_empty()` to ignore `tokens_per_second` (it is optional by nature, not a blocker).

---

### Step 2 — Track per-turn token baseline in `ChatWidget`
**File:** `tui/src/chatwidget.rs`

Add one new field to the `ChatWidget` struct:
```rust
turn_start_output_tokens: u64,
```

In `on_task_started()` (line ~970) — snapshot the current cumulative output tokens:
```rust
self.turn_start_output_tokens = self
    .token_info
    .as_ref()
    .map(|i| i.total_token_usage.output_tokens.max(0) as u64)
    .unwrap_or(0);
```

---

### Step 3 — Compute TPS at turn completion
**File:** `tui/src/chatwidget.rs`

In `on_task_complete()` (line ~999), after getting `runtime_metrics` and `elapsed_seconds`, compute and inject TPS:

```rust
let elapsed_secs = elapsed_seconds.unwrap_or(0) as f64;
let current_output_tokens = self
    .token_info
    .as_ref()
    .map(|i| i.total_token_usage.output_tokens.max(0) as u64)
    .unwrap_or(0)
    .saturating_sub(self.turn_start_output_tokens);

if let Some(ref mut metrics) = runtime_metrics {
    if elapsed_secs > 0.0 && current_output_tokens > 0 {
        metrics.tokens_per_second = Some(current_output_tokens as f64 / elapsed_secs);
    }
}
```

`elapsed_seconds` is already captured at line 1002–1005 from `status_widget().elapsed_seconds()`.

---

### Step 4 — Display TPS in `runtime_metrics_label`
**File:** `tui/src/history_cell.rs`

In `runtime_metrics_label()` (line ~2175), add TPS rendering after the other parts:

```rust
if let Some(tps) = summary.tokens_per_second {
    parts.push(format!("{tps:.1} tok/s"));
}
```

This appends to the `•`-separated label shown in the `FinalMessageSeparator` divider, e.g.:
```
─ Inference: 2 calls (1.2s) • 38.4 tok/s ─────────────────────────
```

---

### Step 5 — Write design doc
**File:** `docs/tokens-per-second.md` (new file)

Write a brief document describing the feature, the data flow, and how to interpret the metric.

---

## Critical Files

| File | Role |
|------|------|
| `otel/src/metrics/runtime_metrics.rs` | Add `tokens_per_second` field to `RuntimeMetricsSummary` |
| `tui/src/chatwidget.rs` | Snapshot token baseline at turn start, compute TPS at completion |
| `tui/src/history_cell.rs` | Render TPS in `runtime_metrics_label()` |
| `docs/tokens-per-second.md` | New design doc (requested by user) |

## Existing Utilities to Reuse

- `StatusIndicatorWidget::elapsed_seconds()` — `tui/src/status_indicator_widget.rs:156`
- `fmt_elapsed_compact()` — `tui/src/status_indicator_widget.rs:49`
- `runtime_metrics_label()` — `tui/src/history_cell.rs:2175` (extend, don't replace)
- `TokenUsageInfo.total_token_usage.output_tokens` — already stored in `self.token_info`

---

## Verification

1. Run `cargo build -p codex-tui` — must compile clean.
2. Start a session, send a prompt that triggers at least one API call.
3. After the turn completes, the divider line should show `X.X tok/s`.
4. Verify it is absent for turns with no API calls (e.g., local tool-only turns).
5. Run `cargo test -p codex-otel` and `cargo test -p codex-tui` — all existing tests must pass.
