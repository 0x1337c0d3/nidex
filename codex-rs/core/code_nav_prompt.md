# Code Navigation Tools

When `code_query` or `code_symbols` is available, **always prefer them over shell grep/find for structural code searches**.

## Decision rule

```
Need to find code?
├── List symbols in a file/dir       → code_symbols(path="src/foo.py")
├── Where is X called / referenced?  → code_query(query="...", lang="python")
├── Where is X defined?              → code_query(query="...", lang="python")
└── Plain text / config / logs       → shell grep is fine
```

**NEVER do this when code_query is available:**
- ❌ `shell ["grep", "-rn", "market_order", "."]` → ✅ `code_query` with a call expression query
- ❌ `shell ["grep", "-rn", "class OrdersClient"]` → ✅ `code_query` with a class definition query
- ❌ Read an entire file to find its functions → ✅ `code_symbols(path="src/client.py")`

## Session start

Call `code_nav_init()` once at the start of a session to warm the index. This is fast and ensures accurate results. Pass `reset: true` only if the index seems stale after a large refactor.

## Tool reference

| Goal | Tool | Example |
|------|------|---------|
| List all symbols (functions, classes, etc.) in a file | `code_symbols` | `code_symbols(path="src/orders.py", lang="python")` |
| Find where a function is **called** | `code_query` | query below |
| Find a **class or function definition** | `code_query` | query below |
| Find attribute / method calls on an object | `code_query` | query below |

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

Named captures (e.g. `@call`, `@fn`) label results — use them to get meaningful output.
