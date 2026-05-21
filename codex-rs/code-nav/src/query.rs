use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use ignore::WalkBuilder;
use serde::Serialize;
use tree_sitter::Parser;
use tree_sitter::Query;
use tree_sitter::QueryCursor;
use tree_sitter::StreamingIterator;

use crate::language::Lang;

const MAX_MATCHES: usize = 500;

#[derive(Debug, Serialize)]
pub struct QueryMatch {
    pub capture: String,
    pub text: String,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Run an arbitrary tree-sitter S-expression query against a file or directory.
///
/// Returns up to `MAX_MATCHES` capture results.
pub fn run_query(query_str: &str, lang: Lang, path: &Path) -> Result<Vec<QueryMatch>> {
    let grammar = lang.grammar();
    let query = Query::new(&grammar, query_str)
        .with_context(|| format!("invalid tree-sitter query for {}", lang.as_str()))?;

    let capture_names: Vec<&str> = query.capture_names().to_vec();

    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .context("failed to set parser language")?;

    let mut results: Vec<QueryMatch> = Vec::new();
    let extensions: std::collections::HashSet<&str> =
        lang.extensions().iter().copied().collect();

    let walk = WalkBuilder::new(path).build();
    for entry in walk.flatten() {
        if !entry.file_type().map_or(false, |t| t.is_file()) {
            continue;
        }
        let file_ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !extensions.contains(file_ext) {
            continue;
        }

        let source = match std::fs::read(entry.path()) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => continue,
        };

        let source_str = std::str::from_utf8(&source).unwrap_or("");
        let file_path = entry.path().to_string_lossy().into_owned();

        let mut cursor = QueryCursor::new();
        let mut qmatches = cursor.matches(&query, tree.root_node(), source.as_slice());
        while let Some(qmatch) = qmatches.next() {
            for cap in qmatch.captures.iter() {
                let cap_name: &str = capture_names
                    .get(cap.index as usize)
                    .copied()
                    .unwrap_or("unknown");
                let text = cap
                    .node
                    .utf8_text(source_str.as_bytes())
                    .unwrap_or("")
                    .to_string();
                results.push(QueryMatch {
                    capture: format!("@{cap_name}"),
                    text,
                    file: file_path.clone(),
                    start_line: cap.node.start_position().row + 1,
                    end_line: cap.node.end_position().row + 1,
                });
                if results.len() >= MAX_MATCHES {
                    return Ok(results);
                }
            }
        }
    }

    Ok(results)
}
