<!-- AIDEX-START -->
## AiDex - Persistent Code Index (MCP Server)

AiDex provides fast, precise code search through a pre-built index.
**Always prefer AiDex over Grep/Glob for code searches.**

### REQUIRED: Before using Grep/Glob/Read for code searches

```
Do I want to search code?
├── .aidex/ exists    → STOP! Use AiDex instead
├── .aidex/ missing   → run aidex_init (don't ask), THEN use AiDex
└── Config/Logs/Text  → Grep/Read is fine
```

**NEVER do this when .aidex/ exists:**
- ❌ `Grep pattern="functionName"` → ✅ `aidex_query term="functionName"`
- ❌ `Grep pattern="class.*Name"` → ✅ `aidex_query term="Name" mode="contains"`
- ❌ `Read file.cs` to see methods → ✅ `aidex_signature file="file.cs"`
- ❌ `Glob pattern="**/*.cs"` + Read → ✅ `aidex_signatures pattern="**/*.cs"`

### Session-Start Rule (REQUIRED — every session, no exceptions)

1. Call `aidex_session({ path: "<project>" })` — detects external changes, auto-reindexes
2. If `.aidex/` does NOT exist → run `aidex_init` automatically (don't ask)
3. If a session note exists → **show it to the user** before continuing
4. **Before ending a session:** always leave a note about what to do next

### Question → Right Tool

| Question | Tool |
|----------|------|
| "Where is X defined?" | `aidex_query term="X"` |
| "Find anything containing X" | `aidex_query term="X" mode="contains"` |
| "All functions starting with X" | `aidex_query term="X" mode="starts_with"` |
| "What methods does file Y have?" | `aidex_signature file="Y"` |
| "Explore all files in src/" | `aidex_signatures pattern="src/**"` |
| "Project overview" | `aidex_summary` + `aidex_tree` |
| "What changed recently?" | `aidex_query term="X" modified_since="2h"` |
| "What changed before yesterday?" | `aidex_query term="X" modified_before="1d"` |
| "What files changed today?" | `aidex_files path="." modified_since="8h"` |
| "Only search in src/server/" | `aidex_query term="X" file_filter="src/server/**"` |
| "Only method definitions" | `aidex_query term="X" type_filter=["method"]` |
| "Have I ever written X?" | `aidex_global_query term="X" mode="contains"` |
| "Which project has class Y?" | `aidex_global_signatures term="Y" kind="class"` |
| "All indexed projects?" | `aidex_global_status` |
| "Store coding convention" | `aidex_global_guideline action="set"` |
| "Load review checklist" | `aidex_global_guideline action="get"` |
| "Debug my app — see logs" | `aidex_log action="init"` + `aidex_viewer` |

### Search Modes

- **`exact`** (default): Finds only the exact identifier — `log` won't match `catalog`
- **`contains`**: Finds identifiers containing the term — `render` matches `preRenderSetup`
- **`starts_with`**: Finds identifiers starting with the term — `Update` matches `UpdatePlayer`, `UpdateUI`

### All Tools (30)

| Category | Tools | Purpose |
|----------|-------|---------|
| Search & Index | `aidex_init`, `aidex_query`, `aidex_update`, `aidex_remove`, `aidex_status` | Index project, search identifiers (exact/contains/starts_with), time filter |
| Signatures | `aidex_signature`, `aidex_signatures` | Get classes + methods without reading files |
| Overview | `aidex_summary`, `aidex_tree`, `aidex_describe`, `aidex_files` | Entry points, file tree, file listing by type |
| Cross-Project | `aidex_link`, `aidex_unlink`, `aidex_links`, `aidex_scan` | Link dependencies, discover projects |
| Global Search | `aidex_global_init`, `aidex_global_query`, `aidex_global_signatures`, `aidex_global_status`, `aidex_global_refresh` | Search across ALL projects |
| Guidelines | `aidex_global_guideline` | Persistent AI instructions & conventions (key-value, global) |
| Sessions | `aidex_session`, `aidex_note` | Track sessions, leave notes (with searchable history & summaries) |
| Tasks | `aidex_task`, `aidex_tasks` | Built-in backlog with priorities, tags, summaries, auto-logged history |
| Log Hub | `aidex_log` | Universal log receiver — any program sends logs via HTTP, AI queries them, live in Viewer |
| Screenshots | `aidex_screenshot`, `aidex_windows` | Cross-platform screen capture with LLM optimization (scale + color reduction) |
| Viewer | `aidex_viewer` | Interactive browser UI with file tree, signatures, tasks, and live logs |

### Session Notes

Leave notes for the next session — they persist in the database:
```
aidex_note({ path: ".", note: "Test the fix after restart" })        # Write
aidex_note({ path: ".", note: "Also check edge cases", append: true }) # Append
aidex_note({ path: "." })                                              # Read
aidex_note({ path: ".", search: "parser" })                            # Search history
aidex_note({ path: ".", clear: true })                                 # Clear
```
- **Before ending a session:** automatically leave a note about next steps
- **User says "remember for next session: ..."** → write it immediately
- Provide `summary` when writing/clearing — the archived note gets a one-sentence description

### Task Backlog

Track TODOs, bugs, and features right next to your code index:
```
aidex_task({ path: ".", action: "create", title: "Fix bug", priority: 1, tags: "bug", summary: "Short description for backlog overview" })
aidex_task({ path: ".", action: "update", id: 1, status: "done" })
aidex_task({ path: ".", action: "log", id: 1, note: "Root cause found" })
aidex_tasks({ path: ".", status: "active" })
```
Priority: 1=high, 2=medium, 3=low | Status: `backlog → active → done | cancelled`
Summaries: One-sentence table-of-contents per task — scan the backlog without reading full details.

### Log Hub — Universal Logging

Any program (C#, Python, Node, C++, etc.) can send logs via HTTP POST. The AI queries them, and the user sees them live in the Viewer.

**When to use:** Whenever working on a program that needs debugging, testing, or logging — **proactively offer Log Hub + Viewer**.

```
aidex_log({ action: "init" })                                         # Start server (port 3335)
aidex_log({ action: "init", persist: true, path: "." })               # With DB persistence
aidex_log({ action: "query" })                                        # Last 50 entries
aidex_log({ action: "query", since: "10m", level: "error" })          # Errors last 10 min
aidex_log({ action: "query", source: "MyApp", contains: "crash" })    # Filtered
aidex_log({ action: "query", consume: true })                         # Poll & remove (no duplicates)
aidex_log({ action: "write", message: "Debug started" })              # AI writes entry
aidex_log({ action: "status" })                                       # Stats
aidex_log({ action: "clear" })                                        # Clear buffer
aidex_log({ action: "free" })                                         # Stop server
```

**HTTP API** for external programs — just POST to `http://localhost:3335/log`:
```json
{ "level": "info", "source": "MyApp", "message": "Player spawned", "data": { "x": 10 } }
```
Levels: `debug`, `info`, `warn`, `error` | Batch: POST array to `/logs` | Health: GET `/health`

**Viewer integration:** Open `aidex_viewer({ path: "." })` — the Logs tab shows a live WebSocket stream with level/source/text filters.

### Global Search (across all projects)

```
aidex_global_init({ path: "/path/to/all/repos" })                     # Scan & register
aidex_global_init({ path: "...", index_unindexed: true, show_progress: true }) # Auto-index + browser progress UI
aidex_global_query({ term: "TransparentWindow", mode: "contains" })   # Search everywhere
aidex_global_signatures({ term: "Render", kind: "method" })           # Find methods everywhere
aidex_global_status({ sort: "recent" })                                # List all projects
aidex_global_refresh()                                                 # Update stats, remove stale
```

### Screenshots

```
aidex_screenshot()                                             # Full screen
aidex_screenshot({ mode: "active_window" })                    # Active window
aidex_screenshot({ mode: "window", window_title: "VS Code" }) # Specific window
aidex_screenshot({ scale: 0.5, colors: 2 })                   # B&W, half size (ideal for LLM)
aidex_screenshot({ colors: 16 })                               # 16 colors (UI readable)
aidex_windows({ filter: "chrome" })                            # Find window titles
```
No index needed. Returns file path → use `Read` to view immediately.

**LLM optimization strategy:** Always start with aggressive settings, then retry if unreadable:
1. First try: `scale: 0.5, colors: 2` (B&W, half size — smallest possible)
2. If unreadable: retry with `colors: 16` (adds shading for UI elements)
3. If still unclear: `scale: 0.75` or omit `colors` for full quality
4. **Remember** what works for each window/app during the session — don't retry every time.
<!-- AIDEX-END -->
