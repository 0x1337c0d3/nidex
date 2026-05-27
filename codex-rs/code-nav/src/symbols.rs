use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use serde::Serialize;

use crate::language::Lang;
use crate::query::run_query;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Mod,
    Type,
    Const,
    Static,
    Macro,
    Class,
    Method,
    Interface,
    Namespace,
    Unknown,
}

impl SymbolKind {
    fn from_capture_prefix(prefix: &str) -> Self {
        match prefix {
            "fn" => Self::Fn,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "mod" => Self::Mod,
            "type" => Self::Type,
            "const" => Self::Const,
            "static" => Self::Static,
            "macro" => Self::Macro,
            "class" => Self::Class,
            "method" => Self::Method,
            "interface" => Self::Interface,
            "namespace" => Self::Namespace,
            _ => Self::Unknown,
        }
    }

    /// Round-trip string form used for SQLite storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Mod => "mod",
            Self::Type => "type",
            Self::Const => "const",
            Self::Static => "static",
            Self::Macro => "macro",
            Self::Class => "class",
            Self::Method => "method",
            Self::Interface => "interface",
            Self::Namespace => "namespace",
            Self::Unknown => "unknown",
        }
    }

    /// Deserialize from the string stored in SQLite.
    pub fn from_str(s: &str) -> Self {
        Self::from_capture_prefix(s)
    }
}

#[derive(Debug, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: usize,
}

/// Returns the tree-sitter query for extracting top-level symbols from a language.
///
/// Capture names follow the convention `<kind>.name` so the caller can derive
/// `SymbolKind` from the prefix before the first `.`.
fn symbols_query(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => {
            "(function_item name: (identifier) @fn.name)
             (struct_item name: (type_identifier) @struct.name)
             (enum_item name: (type_identifier) @enum.name)
             (trait_item name: (type_identifier) @trait.name)
             (mod_item name: (identifier) @mod.name)
             (type_item name: (type_identifier) @type.name)
             (const_item name: (identifier) @const.name)
             (static_item name: (identifier) @static.name)
             (macro_definition name: (identifier) @macro.name)"
        }
        Lang::Python => {
            "(function_definition name: (identifier) @fn.name)
             (class_definition name: (identifier) @class.name)"
        }
        Lang::JavaScript => {
            "(function_declaration name: (identifier) @fn.name)
             (class_declaration name: (identifier) @class.name)
             (method_definition name: (property_identifier) @method.name)"
        }
        Lang::TypeScript => {
            "(function_declaration name: (identifier) @fn.name)
             (class_declaration name: (type_identifier) @class.name)
             (method_definition name: (property_identifier) @method.name)
             (interface_declaration name: (type_identifier) @interface.name)
             (type_alias_declaration name: (type_identifier) @type.name)
             (enum_declaration name: (identifier) @enum.name)"
        }
        Lang::Go => {
            "(function_declaration name: (identifier) @fn.name)
             (method_declaration name: (field_identifier) @method.name)
             (type_spec name: (type_identifier) @type.name)"
        }
        Lang::C => {
            "(function_definition
               declarator: (function_declarator
                 declarator: (identifier) @fn.name))
             (struct_specifier name: (type_identifier) @struct.name)
             (enum_specifier name: (type_identifier) @enum.name)"
        }
        Lang::Cpp => {
            "(function_definition
               declarator: (function_declarator
                 declarator: [(identifier) @fn.name
                              (qualified_identifier) @fn.name]))
             (class_specifier name: (type_identifier) @class.name)
             (struct_specifier name: (type_identifier) @struct.name)
             (enum_specifier name: (type_identifier) @enum.name)
             (namespace_definition name: (namespace_identifier) @namespace.name)"
        }
        Lang::Swift => {
            "(class_declaration
               declaration_kind: \"struct\"
               name: (type_identifier) @struct.name)
             (class_declaration
               declaration_kind: \"class\"
               name: (type_identifier) @class.name)
             (class_declaration
               declaration_kind: \"actor\"
               name: (type_identifier) @class.name)
             (class_declaration
               declaration_kind: \"enum\"
               name: (type_identifier) @enum.name)
             (class_declaration
               declaration_kind: \"extension\"
               name: (type_identifier) @type.name)
             (function_declaration
               (simple_identifier) @fn.name)
             (protocol_declaration
               name: (type_identifier) @interface.name)
             (typealias_declaration
               name: (type_identifier) @type.name)"
        }
        Lang::Bash => "(function_definition name: (word) @fn.name)",
    }
}

/// List all top-level symbols in a file or directory.
pub fn run_symbols(path: &Path, lang: Option<Lang>) -> Result<Vec<Symbol>> {
    let is_file = path
        .metadata()
        .map(|m| m.is_file())
        .unwrap_or(false);

    let langs: Vec<Lang> = if let Some(l) = lang {
        vec![l]
    } else if is_file {
        // Auto-detect language from the single file's extension.
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match Lang::from_extension(ext) {
            Some(l) => vec![l],
            None => return Ok(Vec::new()),
        }
    } else {
        // Walk directory and query all supported languages.
        vec![
            Lang::Bash,
            Lang::C,
            Lang::Cpp,
            Lang::Go,
            Lang::JavaScript,
            Lang::Python,
            Lang::Rust,
            Lang::Swift,
            Lang::TypeScript,
        ]
    };

    let mut symbols: Vec<Symbol> = Vec::new();

    for l in langs {
        let query_str = symbols_query(l);
        let matches = run_query(query_str, l, path)?;
        for m in matches {
            // Capture name is "<kind>.name"; strip the leading `@` added by run_query.
            let cap = m.capture.trim_start_matches('@');
            let kind_prefix = cap.split('.').next().unwrap_or("unknown");
            symbols.push(Symbol {
                name: m.text,
                kind: SymbolKind::from_capture_prefix(kind_prefix),
                file: m.file,
                line: m.start_line,
            });
        }
    }

    symbols.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    Ok(symbols)
}

/// Parse symbols from a specific list of files (already identified as stale/new).
///
/// Returns `(path, mtime, symbols)` for each file that could be parsed.
/// Runs synchronously; call inside `tokio::task::spawn_blocking`.
pub fn run_symbols_for_files(
    files: &[(PathBuf, Lang, SystemTime)],
) -> Result<Vec<(PathBuf, SystemTime, Vec<Symbol>)>> {
    let mut out = Vec::with_capacity(files.len());
    for (path, lang, mtime) in files {
        let syms = run_symbols(path, Some(*lang))?;
        out.push((path.clone(), *mtime, syms));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn symbols_rust_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample.rs");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(
            f,
            "pub struct Foo {{}}\npub fn bar() {{}}\npub enum Baz {{}}\n"
        )
        .expect("write");

        let syms = run_symbols(&path, None).expect("run_symbols");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "expected Foo in {names:?}");
        assert!(names.contains(&"bar"), "expected bar in {names:?}");
        assert!(names.contains(&"Baz"), "expected Baz in {names:?}");
    }

    #[test]
    fn symbols_python_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample.py");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "def greet(name):\n    pass\n\nclass Dog:\n    pass\n").expect("write");

        let syms = run_symbols(&path, None).expect("run_symbols");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"), "expected greet in {names:?}");
        assert!(names.contains(&"Dog"), "expected Dog in {names:?}");
    }

    #[test]
    fn symbols_swift_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample.swift");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(
            f,
            "struct Foo {{}}\nfunc bar() {{}}\nenum Baz {{}}\nclass Qux {{}}\n"
        )
        .expect("write");

        let syms = run_symbols(&path, None).expect("run_symbols");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "expected Foo in {names:?}");
        assert!(names.contains(&"bar"), "expected bar in {names:?}");
        assert!(names.contains(&"Baz"), "expected Baz in {names:?}");
        assert!(names.contains(&"Qux"), "expected Qux in {names:?}");
    }
}
