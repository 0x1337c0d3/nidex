# Plan: Session Memory Notes

## Context

Codex-rs has no persistent "next session" notes. When a user does `/new` or restarts, all
context is lost. AiDex solves this with per-directory session notes that survive restarts and
appear as a banner at the top of every new session. This feature adds the same capability.

**Key design choice**: notes are keyed by the **git root** of the working directory, **not**
thread ID or raw cwd — so they survive `/new`, restarts, UUID changes, and launching from any
subdirectory of the same project. The git root is resolved via `codex_code_nav::find_project_root(cwd)`
(already a dep in `codex-core`), which walks parent directories looking for `.git/` and falls
back to `cwd` if no `.git/` is found.

---

## Files to Create / Modify

| File | Action |
|------|--------|
| `state/migrations/0005_session_notes.sql` | **Create** — schema |
| `state/src/runtime.rs` | **Modify** — add 3 CRUD methods |
| `protocol/src/protocol.rs` | **Modify** — add `Op::GetSessionNote`, `Op::SetSessionNote { note }`, `EventMsg::SessionNote` |
| `core/src/codex.rs` | **Modify** — inject note into `post_session_configured_events`; handle 2 new Ops |
| `tui/src/slash_command.rs` | **Modify** — add `Note` variant |
| `tui/src/chatwidget.rs` | **Modify** — route `/note [args]`, handle `EventMsg::SessionNote` |
| `tui/src/history_cell.rs` | **Modify** — add `new_session_note_cell()` for styled display |

---

## Step 1 — DB Schema

**`state/migrations/0005_session_notes.sql`**

```sql
CREATE TABLE IF NOT EXISTS session_notes (
    cwd         TEXT    NOT NULL PRIMARY KEY,
    note        TEXT    NOT NULL,
    updated_at  INTEGER NOT NULL
);
```

No `threads` foreign key — notes are cwd-scoped and outlive individual threads.

---

## Step 2 — State CRUD Methods

**`state/src/runtime.rs`** — follow the `get_thread` / `upsert_thread` patterns (lines 91-120 and 343-404):

```rust
pub async fn get_session_note(&self, cwd: &str) -> anyhow::Result<Option<String>> {
    let row = sqlx::query("SELECT note FROM session_notes WHERE cwd = ?")
        .bind(cwd)
        .fetch_optional(self.pool.as_ref())
        .await?;
    Ok(row.map(|r| r.get::<String, _>("note")))
}

pub async fn set_session_note(&self, cwd: &str, note: &str) -> anyhow::Result<()> {
    let now = unix_now_secs();  // already exists in runtime.rs
    sqlx::query(
        "INSERT INTO session_notes (cwd, note, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(cwd) DO UPDATE SET note = excluded.note, updated_at = excluded.updated_at"
    )
    .bind(cwd).bind(note).bind(now)
    .execute(self.pool.as_ref()).await?;
    Ok(())
}

pub async fn clear_session_note(&self, cwd: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM session_notes WHERE cwd = ?")
        .bind(cwd)
        .execute(self.pool.as_ref()).await?;
    Ok(())
}
```

---

## Step 3 — Protocol: New Op Variants + Event Type

**`protocol/src/protocol.rs`** — add to `Op` enum after `SetThreadName { name }` (line ~271):

```rust
/// Retrieve the session note for the current working directory.
/// Response is EventMsg::SessionNote.
GetSessionNote,

/// Persist a note for the current working directory.
/// Empty string clears the note. Response is EventMsg::SessionNote.
SetSessionNote { note: String },
```

Add to `EventMsg` enum (near `DeprecationNotice`):

```rust
SessionNote(SessionNoteEvent),
```

Add struct and action enum (near other event structs, e.g. near `DeprecationNoticeEvent`):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionNoteEvent {
    /// The note text, or None if no note is set / note was cleared.
    pub note: Option<String>,
    pub action: SessionNoteAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SessionNoteAction {
    Startup,  // shown automatically at session start
    Current,  // user ran /note with no args
    Set,      // user saved a new note
    Cleared,  // user cleared the note
}
```

---

## Step 4 — Core: Session-Start Injection

**`core/src/codex.rs`** — `state_db_ctx` is available from line 792. After
`maybe_push_unstable_features_warning` (~line 857), push a note event if one exists:

```rust
if let Some(state_db) = &state_db_ctx {
    let project_root = codex_code_nav::find_project_root(&config.cwd);
    let cwd_str = project_root.to_string_lossy();
    if let Ok(Some(note)) = state_db.get_session_note(&cwd_str).await {
        post_session_configured_events.push(Event {
            id: INITIAL_SUBMIT_ID.to_owned(),
            msg: EventMsg::SessionNote(SessionNoteEvent {
                note: Some(note),
                action: SessionNoteAction::Startup,
            }),
        });
    }
}
```

**Op dispatch** — in the submission loop match after `Op::SetThreadName` (line ~2460):

```rust
Op::GetSessionNote => {
    handlers::get_session_note(&sess, sub.id.clone()).await;
}
Op::SetSessionNote { note } => {
    handlers::set_session_note(&sess, sub.id.clone(), note).await;
}
```

**Handler functions** — follow the `set_thread_name` pattern (lines 2940-3010):

- `get_session_note`: resolves `codex_code_nav::find_project_root(&turn_context.cwd)`, reads `state_db.get_session_note(&root_str)`, emits `EventMsg::SessionNote { note, action: Current }`
- `set_session_note`: resolves git root, if `note.is_empty()` calls `clear_session_note` + emits `Cleared`, else calls `set_session_note` + emits `Set`

Both use `sess.new_default_turn_with_sub_id(sub_id)` and `sess.send_event(...)`.

---

## Step 5 — TUI: Slash Command

**`tui/src/slash_command.rs`** — add `Note` to `SlashCommand` enum:

```rust
Note,
```

| Method | Value |
|--------|-------|
| `description()` | `"view or set a persistent note for this project directory"` |
| `supports_inline_args()` | `true` (so `/note text here` works) |
| `available_during_task()` | `true` (can read/set during active tasks, like `/status`) |

---

## Step 6 — TUI: Command Routing

**`tui/src/chatwidget.rs`** — in the slash-command dispatch match:

```rust
SlashCommand::Note => {
    let trimmed = args.trim().to_string();
    let op = if trimmed.is_empty() {
        Op::GetSessionNote
    } else {
        // "/note clear" clears; anything else sets
        let note = if trimmed == "clear" { String::new() } else { trimmed };
        Op::SetSessionNote { note }
    };
    self.app_event_tx.send(AppEvent::CodexOp(op)).ok();
}
```

Add `EventMsg::SessionNote` handler in `dispatch_event_msg` (near `DeprecationNotice` at line ~3458):

```rust
EventMsg::SessionNote(ev) => self.on_session_note(ev),
```

Handler method:

```rust
fn on_session_note(&mut self, event: SessionNoteEvent) {
    self.add_to_history(history_cell::new_session_note_cell(event));
    self.request_redraw();
}
```

---

## Step 7 — TUI: Styled History Cell

**`tui/src/history_cell.rs`** — add `new_session_note_cell(event: SessionNoteEvent) -> impl HistoryCell`.

Model on `new_deprecation_notice` / `new_info_event`. Rendering logic:

| Action | Display |
|--------|---------|
| `Startup` | Header "Session Note" + note text (dimmed/boxed) |
| `Current` | Same as Startup but labeled "Current Note" |
| `Set` | "Note saved." + note text preview |
| `Cleared` | "Session note cleared." |
| `note = None` on `Current` | "No session note set. Use `/note <text>` to add one." |

---

## Verification

1. `cargo test -p codex-state` — add tests for the 3 new `runtime.rs` methods
2. `cargo build` — full workspace build
3. **End-to-end manual test**:
   - Type `/note Next: finish the auth refactor` → "Note saved." confirmation appears
   - `/new` → note banner appears at top of fresh session
   - `/note` (no args) → note displayed again in history
   - `/note clear` → "Session note cleared." confirmation
   - `/new` → no note banner
4. `cargo test -p codex-core` — all existing tests still pass (target: 1537+)
