use tree_sitter::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Bash,
    C,
    Cpp,
    Go,
    JavaScript,
    Python,
    Rust,
    Swift,
    TypeScript,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "sh" | "bash" | "zsh" => Some(Self::Bash),
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Some(Self::Cpp),
            "go" => Some(Self::Go),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "py" | "pyw" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            "swift" => Some(Self::Swift),
            "ts" | "tsx" | "mts" | "cts" => Some(Self::TypeScript),
            _ => None,
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "bash" | "sh" => Some(Self::Bash),
            "c" => Some(Self::C),
            "cpp" | "c++" | "cxx" => Some(Self::Cpp),
            "go" | "golang" => Some(Self::Go),
            "js" | "javascript" => Some(Self::JavaScript),
            "py" | "python" => Some(Self::Python),
            "rs" | "rust" => Some(Self::Rust),
            "swift" => Some(Self::Swift),
            "ts" | "tsx" | "typescript" => Some(Self::TypeScript),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Go => "go",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Swift => "swift",
            Self::TypeScript => "typescript",
        }
    }

    pub fn grammar(self) -> Language {
        match self {
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }

    /// File extensions associated with this language (for directory walks).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Bash => &["sh", "bash", "zsh"],
            Self::C => &["c", "h"],
            Self::Cpp => &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
            Self::Go => &["go"],
            Self::JavaScript => &["js", "mjs", "cjs"],
            Self::Python => &["py", "pyw"],
            Self::Rust => &["rs"],
            Self::Swift => &["swift"],
            Self::TypeScript => &["ts", "tsx", "mts", "cts"],
        }
    }
}
