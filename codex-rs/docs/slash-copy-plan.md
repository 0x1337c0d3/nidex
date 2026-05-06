# Implement `/copy` Slash Command

## Goal
Add a `/copy` slash command that renders the recent transcript to markdown format and copies it to the system clipboard.

## What "recent transcript" means
- All committed `history_cells` in `ChatWidget.history` (transcript only, not display-only cells)
- Plus the current active cell (`ChatWidget.active_cell`) if present
- Rendered as plain text using each cell's `transcript_lines()` method (not `display_lines()` which includes ANSI/timestamps)
- Output as markdown: user messages as quoted blocks, agent responses as code blocks or plain text

## Files to modify

### 1. `tui/src/slash_command.rs`
Add `Copy` variant to the `SlashCommand` enum:
```rust
Copy,
```
Add description:
```rust
SlashCommand::Copy => "copy the conversation transcript to clipboard",
```
Add to `available_during_task()`: `true` (like `Status`)

### 2. `tui/src/chatwidget.rs`

#### Add helper to collect transcript lines (new method)
```rust
pub(crate) fn transcript_for_copy(&self, width: u16) -> Vec<String>
```
- Collect all `transcript_lines()` from `self.history` cells
- If `self.active_cell` is present, append its `transcript_lines()` 
- Convert each `Line` to a plain string (strip ANSI codes via `ratatui::style::Styled`)
- Return `Vec<String>` of lines

#### Add command handler (in `dispatch_command`)
```rust
SlashCommand::Copy => {
    self.copy_transcript_to_clipboard();
}
```

#### Add clipboard handler (new method)
```rust
pub(crate) fn copy_transcript_to_clipboard(&mut self)
```
- Get transcript lines via `transcript_for_copy(120)` (reasonable width for wrapping)
- Join with newlines to form markdown string
- Write to clipboard using `arboard::Clipboard` (same crate used in `clipboard_paste.rs`)
- Show confirmation: add a plain info cell like "Transcript copied to clipboard"

### 3. Optional: strip ANSI if needed
Check if `transcript_lines()` returns clean text or if ANSI stripping is needed. If cells use ANSI styles in transcript, add a helper to strip them before clipboard write.

## Key decisions

| Decision | Choice |
|----------|--------|
| Clipboard crate | `arboard` (already used in `clipboard_paste.rs`) |
| Terminal width for transcript | 120 chars (gives readable wrapping) |
| What to copy | All committed cells + active cell |
| Output format | Plain text with newlines (markdown-compatible) |
| Available during task | Yes (safe, read-only operation) |

## Test scenarios
1. `/copy` with no conversation → empty or "no transcript" message
2. `/copy` with simple user/assistant exchange → copies both as markdown
3. `/copy` while streaming → includes the in-progress cell content
4. `/copy` with exec/tool output cells → includes command and output
5. Clipboard unavailable → shows error message in chat

## Dependencies
- `arboard` crate (already in Cargo.toml via `clipboard_paste`)
- No new external dependencies needed
