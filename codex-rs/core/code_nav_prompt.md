# Code Navigation Tools

**Always prefer `code_symbols` and `code_query` over shell commands for code searches.**

> **IMPORTANT:** `code_nav_init`, `code_symbols`, and `code_query` are **internal tools** — they are NOT MCP servers. Never call `read_mcp_resource` or `list_mcp_resources` with `server: "code-nav"`. There is no `docuBase://` URI scheme. Use the tools directly.

## Session-Start Rule (REQUIRED — every session, no exceptions)

Call `code_nav_init()` before any code search or file exploration. This is fast and ensures accurate results.

```
Do I need to explore or search code?
└── YES → call code_nav_init() FIRST, then use code_symbols / code_query

Am I about to call read_file, list_dir, or grep_files to understand code structure?
└── STOP → use code_symbols instead
```

Pass `reset: true` only after a large refactor or if results seem stale.

## Before using shell cat/grep/find for code

```
Do I want to understand a file's structure?
├── What functions/structs are in this file?    → code_symbols(path="src/foo.rs")
├── What's defined across a directory?          → code_symbols(path="src/")
├── Where is X defined?                         → code_query with a definition query
├── Where is X called?                          → code_query with a call expression query
└── Plain text / config / logs                  → shell grep is fine
```

**NEVER do this when code_symbols/code_query are available:**
- ❌ `shell ["cat", "frontend.rs"]` to understand its structure → ✅ `code_symbols(path="frontend.rs")`
- ❌ `shell ["cat", "mod.rs"]` to find functions → ✅ `code_symbols(path="mod.rs")`
- ❌ `shell ["grep", "-rn", "fn handle_request", "."]` → ✅ `code_query` with a function definition query
- ❌ `shell ["grep", "-rn", "class OrdersClient"]` → ✅ `code_query` with a struct/class definition query
- ❌ `shell ["find", "src", "-name", "*.rs"]` + cat each file → ✅ `code_symbols(path="src/")`
- ❌ `read_file(file_path="ingest.rs")` to see its structure → ✅ `code_symbols(path="ingest.rs")`
- ❌ `read_file` on multiple files to explore a module → ✅ `code_symbols(path="src/ingest/")`
- ❌ `list_dir(dir_path="src/api")` + reading each file → ✅ `code_symbols(path="src/api/")`
- ❌ `grep_files(pattern="*.rs", path="api")` to find files → ✅ `code_symbols(path="api/")`
- ❌ Read an entire file to see its methods → ✅ `code_symbols(path="src/client.rs")`

## Question → Right Tool

| Question | Tool |
|----------|------|
| "What functions are in this file?" | `code_symbols(path="file.rs")` |
| "What's defined in this directory?" | `code_symbols(path="src/")` |
| "Where is `handle_event` defined?" | `code_query` — function/method definition query |
| "Where is `handle_event` called?" | `code_query` — call expression query |
| "Where is struct `Config` defined?" | `code_query` — struct definition query |
| "What implements trait `Renderer`?" | `code_query` — impl block query |

## Tree-sitter query examples (Rust)

```
# Where is the function handle_event defined?
(function_item name: (identifier) @name (#eq? @name "handle_event")) @fn

# Where is handle_event called?
(call_expression function: (identifier) @fn (#eq? @fn "handle_event")) @call

# Where is handle_event called as a method?
(call_expression function: (field_expression field: (field_identifier) @method (#eq? @method "handle_event"))) @call

# Where is struct Config defined?
(struct_item name: (type_identifier) @name (#eq? @name "Config")) @struct

# Where is enum Status defined?
(enum_item name: (type_identifier) @name (#eq? @name "Status")) @enum

# What implements the Renderer trait?
(impl_item trait: (type_identifier) @trait (#eq? @trait "Renderer")) @impl
```

## Tree-sitter query examples (Python)

```
# Where is market_order() called?
(call function: (identifier) @fn (#eq? @fn "market_order")) @call

# Where is OrdersClient.market_order called as a method?
(call function: (attribute attribute: (identifier) @method (#eq? @method "market_order"))) @call

# Where is OrdersClient defined?
(class_definition name: (identifier) @name (#eq? @name "OrdersClient")) @class

# Where is the market_order function defined?
(function_definition name: (identifier) @name (#eq? @name "market_order")) @fn
```

Supported languages: `bash`, `c`, `cpp`, `go`, `javascript`, `python`, `rust`, `typescript`.

Named captures (e.g. `@fn`, `@call`) label results — use them to get meaningful output.
