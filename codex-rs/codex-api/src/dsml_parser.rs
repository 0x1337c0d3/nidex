//! Streaming parser for DeepSeek DSML (DeepSeek Markup Language) tool calls.
//!
//! DeepSeek V3+ models embed tool calls directly in `delta.content` text using
//! an XML format delimited by the special token `｜DSML｜` (U+FF5C FULLWIDTH
//! VERTICAL LINE).  Unlike the OpenAI JSON `tool_calls` field, these arrive as
//! plain text and must be intercepted before the content reaches the assistant
//! message accumulator.
//!
//! Format example:
//! ```text
//! <｜DSML｜tool_calls>
//! <｜DSML｜invoke name="my_tool">
//! <｜DSML｜parameter name="query" string="true">hello world</｜DSML｜parameter>
//! <｜DSML｜parameter name="count" string="false">3</｜DSML｜parameter>
//! </｜DSML｜invoke>
//! </｜DSML｜tool_calls>
//! ```

use regex_lite::Regex;
use serde_json::Map;
use serde_json::Value;
use std::sync::OnceLock;

const OPEN_TAG: &str = "<\u{FF5C}DSML\u{FF5C}tool_calls>";
const CLOSE_TAG: &str = "</\u{FF5C}DSML\u{FF5C}tool_calls>";

fn invoke_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"<\u{{FF5C}}DSML\u{{FF5C}}invoke name="([^"]+)">([\s\S]*?)</\u{{FF5C}}DSML\u{{FF5C}}invoke>"#
        ))
        .expect("valid DSML invoke regex")
    })
}

fn param_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"<\u{{FF5C}}DSML\u{{FF5C}}parameter name="([^"]+)" string="(true|false)">([\s\S]*?)</\u{{FF5C}}DSML\u{{FF5C}}parameter>"#
        ))
        .expect("valid DSML parameter regex")
    })
}

/// A fully-parsed DSML tool invocation ready to be dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedToolCall {
    pub(crate) name: String,
    /// Compact JSON object string, e.g. `{"query":"foo","count":3}`.
    pub(crate) arguments: String,
    /// Synthetic call id, e.g. `dsml-tool-0`.
    pub(crate) call_id: String,
}

/// Output produced by [`DsmlParser::feed`] and [`DsmlParser::finish`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DsmlSegment {
    /// Regular assistant text that should be forwarded to the message accumulator.
    Normal(String),
    /// One or more fully-parsed tool calls extracted from a DSML block.
    ToolCalls(Vec<ParsedToolCall>),
}

enum State {
    /// No DSML block is open.
    Idle,
    /// Inside an open `<｜DSML｜tool_calls>` wrapper; accumulating the raw block.
    Buffering(String),
}

pub(crate) struct DsmlParser {
    state: State,
    call_counter: u32,
}

impl DsmlParser {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Idle,
            call_counter: 0,
        }
    }

    /// Feed a streaming content delta and receive zero or more segments back.
    ///
    /// Normal text is returned as [`DsmlSegment::Normal`]; complete DSML blocks
    /// are parsed and returned as [`DsmlSegment::ToolCalls`].  Multiple DSML
    /// blocks in a single delta are supported.
    pub(crate) fn feed(&mut self, delta: &str) -> Vec<DsmlSegment> {
        let mut segments = Vec::new();
        let mut remaining = delta;

        loop {
            match &mut self.state {
                State::Idle => {
                    if let Some(pos) = remaining.find(OPEN_TAG) {
                        // Emit any text before the DSML block.
                        if pos > 0 {
                            push_normal(&mut segments, &remaining[..pos]);
                        }
                        // Transition to buffering; skip past the open tag.
                        remaining = &remaining[pos + OPEN_TAG.len()..];
                        self.state = State::Buffering(String::new());
                    } else {
                        // Check whether the tail of `remaining` could be a partial
                        // OPEN_TAG prefix — if so, buffer it rather than emitting it
                        // as normal text (it might complete in the next delta).
                        let hold = longest_suffix_prefix(remaining, OPEN_TAG);
                        let emit_len = remaining.len() - hold;
                        if emit_len > 0 {
                            push_normal(&mut segments, &remaining[..emit_len]);
                        }
                        if hold > 0 {
                            // Hold this potential prefix; store in a temporary buffer
                            // and re-process it next time only if no more input comes.
                            // For simplicity we keep it as a Buffering("") with a
                            // pending-prefix variant — instead we use a dedicated field.
                            // Since OPEN_TAG is a single special token and almost always
                            // arrives atomically, we just hold the prefix in an internal
                            // pre-buffer and re-process next call.
                            self.state = State::Buffering(format!("{OPEN_TAG_PREFIX_SENTINEL}{}", &remaining[emit_len..]));
                        }
                        break;
                    }
                }
                State::Buffering(buf) => {
                    // Check if this buffer starts with the prefix-sentinel (we're
                    // waiting to see if a potential OPEN_TAG completes).
                    if buf.starts_with(OPEN_TAG_PREFIX_SENTINEL) {
                        let held = buf[OPEN_TAG_PREFIX_SENTINEL.len()..].to_string();
                        let combined = held + remaining;
                        *buf = String::new();
                        // Drop the sentinel; try again from Idle with combined text.
                        self.state = State::Idle;
                        let mut sub = self.feed(&combined);
                        segments.append(&mut sub);
                        break;
                    }

                    if let Some(pos) = remaining.find(CLOSE_TAG) {
                        // Append content up to (but not including) the close tag.
                        buf.push_str(&remaining[..pos]);
                        let block = std::mem::take(buf);
                        self.state = State::Idle;
                        remaining = &remaining[pos + CLOSE_TAG.len()..];

                        let calls = self.parse_dsml_block(&block);
                        if !calls.is_empty() {
                            segments.push(DsmlSegment::ToolCalls(calls));
                        }
                        // Continue the loop — there may be more text or another block.
                    } else {
                        buf.push_str(remaining);
                        break;
                    }
                }
            }
        }

        segments
    }

    /// Flush any buffered state at end-of-stream.
    ///
    /// If a DSML block was never closed (truncated stream), we attempt to parse
    /// whatever was accumulated rather than silently discarding it.
    pub(crate) fn finish(&mut self) -> Vec<DsmlSegment> {
        let mut segments = Vec::new();
        match &mut self.state {
            State::Idle => {}
            State::Buffering(buf) => {
                if buf.starts_with(OPEN_TAG_PREFIX_SENTINEL) {
                    // A partial OPEN_TAG was buffered but never completed — emit as normal text.
                    let text = buf[OPEN_TAG_PREFIX_SENTINEL.len()..].to_string();
                    if !text.is_empty() {
                        push_normal(&mut segments, &text);
                    }
                } else {
                    let block = std::mem::take(buf);
                    let calls = self.parse_dsml_block(&block);
                    if !calls.is_empty() {
                        segments.push(DsmlSegment::ToolCalls(calls));
                    }
                }
                self.state = State::Idle;
            }
        }
        segments
    }

    fn parse_dsml_block(&mut self, block: &str) -> Vec<ParsedToolCall> {
        let mut calls = Vec::new();

        for invoke_cap in invoke_re().captures_iter(block) {
            let name = invoke_cap[1].to_string();
            let body = &invoke_cap[2];

            let mut map: Map<String, Value> = Map::new();
            for param_cap in param_re().captures_iter(body) {
                let param_name = param_cap[1].to_string();
                let is_string = &param_cap[2] == "true";
                let raw_value = param_cap[3].trim();

                let json_value = if is_string {
                    Value::String(raw_value.to_string())
                } else {
                    serde_json::from_str(raw_value).unwrap_or_else(|_| Value::String(raw_value.to_string()))
                };
                map.insert(param_name, json_value);
            }

            let arguments = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
            let call_id = format!("dsml-tool-{}", self.call_counter);
            self.call_counter += 1;

            calls.push(ParsedToolCall { name, arguments, call_id });
        }

        calls
    }
}

/// A sentinel prefix used internally to distinguish "buffering a potential
/// OPEN_TAG prefix" from "buffering interior DSML content".  It is never
/// visible outside this module.
const OPEN_TAG_PREFIX_SENTINEL: &str = "\x00DSML_PREFIX\x00";

/// Returns the length of the longest suffix of `text` that is also a prefix
/// of `pattern`.  Used to detect partial tag matches at chunk boundaries.
fn longest_suffix_prefix(text: &str, pattern: &str) -> usize {
    let text_bytes = text.as_bytes();
    let pat_bytes = pattern.as_bytes();
    let max_len = text_bytes.len().min(pat_bytes.len());
    for len in (1..=max_len).rev() {
        if text_bytes[text_bytes.len() - len..] == pat_bytes[..len] {
            return len;
        }
    }
    0
}

fn push_normal(segments: &mut Vec<DsmlSegment>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(DsmlSegment::Normal(existing)) = segments.last_mut() {
        existing.push_str(text);
    } else {
        segments.push(DsmlSegment::Normal(text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // U+FF5C = FULLWIDTH VERTICAL LINE, the namespace delimiter used by DSML.
    const NS: &str = "\u{FF5C}DSML\u{FF5C}";

    fn invoke(name: &str, params: &[(&str, bool, &str)]) -> String {
        let mut s = format!("<{NS}invoke name=\"{name}\">\n");
        for (pname, is_str, value) in params {
            let flag = if *is_str { "true" } else { "false" };
            s.push_str(&format!(
                "<{NS}parameter name=\"{pname}\" string=\"{flag}\">{value}</{NS}parameter>\n"
            ));
        }
        s.push_str(&format!("</{NS}invoke>"));
        s
    }

    fn dsml_block(invokes: &str) -> String {
        format!("{OPEN_TAG}\n{invokes}\n{CLOSE_TAG}")
    }

    fn feed_all(chunks: &[&str]) -> Vec<DsmlSegment> {
        let mut parser = DsmlParser::new();
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(parser.feed(chunk));
        }
        out.extend(parser.finish());
        out
    }

    fn single_tool_call(name: &str, arguments: &str, call_id: &str) -> DsmlSegment {
        DsmlSegment::ToolCalls(vec![ParsedToolCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
            call_id: call_id.to_string(),
        }])
    }

    #[test]
    fn parses_single_invoke_string_param() {
        let block = dsml_block(&invoke("search", &[("query", true, "hello world")]));
        let segs = feed_all(&[&block]);
        assert_eq!(
            segs,
            vec![single_tool_call("search", r#"{"query":"hello world"}"#, "dsml-tool-0")]
        );
    }

    #[test]
    fn parses_json_param_string_false() {
        let block = dsml_block(&invoke("calc", &[("n", false, "42")]));
        let segs = feed_all(&[&block]);
        assert_eq!(
            segs,
            vec![single_tool_call("calc", r#"{"n":42}"#, "dsml-tool-0")]
        );
    }

    #[test]
    fn parses_multiple_invokes() {
        let body = format!(
            "{}\n{}",
            invoke("a", &[("x", true, "foo")]),
            invoke("b", &[("y", false, "true")])
        );
        let block = dsml_block(&body);
        let segs = feed_all(&[&block]);
        assert_eq!(
            segs,
            vec![DsmlSegment::ToolCalls(vec![
                ParsedToolCall {
                    name: "a".into(),
                    arguments: r#"{"x":"foo"}"#.into(),
                    call_id: "dsml-tool-0".into()
                },
                ParsedToolCall {
                    name: "b".into(),
                    arguments: r#"{"y":true}"#.into(),
                    call_id: "dsml-tool-1".into()
                },
            ])]
        );
    }

    #[test]
    fn normal_text_before_and_after_block() {
        let text = format!(
            "prefix\n{}\nsuffix",
            dsml_block(&invoke("t", &[("k", true, "v")]))
        );
        let segs = feed_all(&[&text]);
        assert_eq!(segs[0], DsmlSegment::Normal("prefix\n".into()));
        assert!(matches!(&segs[1], DsmlSegment::ToolCalls(calls) if calls[0].name == "t"));
        assert_eq!(segs[2], DsmlSegment::Normal("\nsuffix".into()));
    }

    #[test]
    fn block_split_across_chunks() {
        let full = dsml_block(&invoke("search", &[("q", true, "split")]));
        let mid = full.len() / 2;
        let segs = feed_all(&[&full[..mid], &full[mid..]]);
        assert_eq!(segs.len(), 1);
        assert!(matches!(&segs[0], DsmlSegment::ToolCalls(calls) if calls[0].name == "search"));
    }

    #[test]
    fn truncated_block_flushed_by_finish() {
        // No closing </｜DSML｜tool_calls> — finish() must still emit the parsed calls.
        let partial = format!("{OPEN_TAG}\n{}", invoke("t", &[("k", true, "v")]));
        let segs = feed_all(&[&partial]);
        assert_eq!(segs.len(), 1);
        assert!(matches!(&segs[0], DsmlSegment::ToolCalls(calls) if calls[0].name == "t"));
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        let segs = feed_all(&["hello world"]);
        assert_eq!(segs, vec![DsmlSegment::Normal("hello world".into())]);
    }
}
