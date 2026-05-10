use crate::error::ApiError;
use crate::provider::Provider;
use crate::requests::headers::build_conversation_headers;
use crate::requests::headers::insert_header;
use crate::requests::headers::subagent_header;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use http::HeaderMap;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

/// Assembled request body plus headers for Chat Completions streaming calls.
pub struct ChatRequest {
    pub body: Value,
    pub headers: HeaderMap,
}

pub struct ChatRequestBuilder<'a> {
    model: &'a str,
    instructions: &'a str,
    input: &'a [ResponseItem],
    tools: &'a [Value],
    conversation_id: Option<String>,
    session_source: Option<SessionSource>,
    supported_message_roles: Vec<String>,
    model_slug: Option<&'a str>,
    reasoning_field_name: Option<&'a str>,
}

impl<'a> ChatRequestBuilder<'a> {
    pub fn new(
        model: &'a str,
        instructions: &'a str,
        input: &'a [ResponseItem],
        tools: &'a [Value],
    ) -> Self {
        Self {
            model,
            instructions,
            input,
            tools,
            conversation_id: None,
            session_source: None,
            supported_message_roles: vec![
                "system".to_string(),
                "user".to_string(),
                "assistant".to_string(),
                "tool".to_string(),
            ],
            model_slug: None,
            reasoning_field_name: None,
        }
    }

    pub fn conversation_id(mut self, id: Option<String>) -> Self {
        self.conversation_id = id;
        self
    }

    pub fn session_source(mut self, source: Option<SessionSource>) -> Self {
        self.session_source = source;
        self
    }

    pub fn supported_message_roles(mut self, roles: Vec<String>) -> Self {
        self.supported_message_roles = roles;
        self
    }

    pub fn model_slug(mut self, slug: &'a str) -> Self {
        self.model_slug = Some(slug);
        self
    }

    pub fn reasoning_field_name(mut self, name: Option<&'a str>) -> Self {
        self.reasoning_field_name = name;
        self
    }

    pub fn build(self, provider: &Provider) -> Result<ChatRequest, ApiError> {
        let mut messages = Vec::<Value>::new();
        messages.push(json!({"role": "system", "content": self.instructions}));

        // Determine the reasoning field name once for the whole request.
        // Check the model slug in addition to the provider name so that
        // DeepSeek models accessed through non-DeepSeek providers (e.g. Ollama,
        // NVIDIA NIM) are still handled correctly.
        let reasoning_field = self.reasoning_field_name.unwrap_or_else(|| {
            let slug = self.model_slug.unwrap_or(self.model);
            if is_deepseek_variant(&provider.name) || is_deepseek_variant(slug) {
                "reasoning_content"
            } else {
                "reasoning"
            }
        });

        let input = self.input;
        let mut reasoning_by_anchor_index: HashMap<usize, String> = HashMap::new();

        // Collect reasoning text for all turns (including historical ones).
        // DeepSeek and similar providers require the reasoning field on every
        // assistant message it produced, even across multiple conversation turns.
        for (idx, item) in input.iter().enumerate() {
            if let ResponseItem::Reasoning {
                content: Some(items),
                ..
            } = item
            {
                let mut text = String::new();
                for entry in items {
                    match entry {
                        ReasoningItemContent::ReasoningText { text: segment }
                        | ReasoningItemContent::Text { text: segment } => {
                            text.push_str(segment)
                        }
                    }
                }

                let mut attached = false;
                if idx > 0
                    && let ResponseItem::Message { role, .. } = &input[idx - 1]
                    && role == "assistant"
                    // Don't attach reasoning when a user message follows — the reasoning
                    // belongs to a previous turn and should not be forwarded.
                    && (idx + 1 >= input.len() || !matches!(&input[idx + 1], ResponseItem::Message { role, .. } if role == "user"))
                {
                    reasoning_by_anchor_index
                        .entry(idx - 1)
                        .and_modify(|v| v.push_str(&text))
                        .or_insert(text.clone());
                    attached = true;
                }

                if !attached && idx + 1 < input.len() {
                    match &input[idx + 1] {
                        ResponseItem::FunctionCall { .. }
                        | ResponseItem::LocalShellCall { .. } => {
                            reasoning_by_anchor_index
                                .entry(idx + 1)
                                .and_modify(|v| v.push_str(&text))
                                .or_insert(text.clone());
                        }
                        ResponseItem::Message { role, .. } if role == "assistant" => {
                            reasoning_by_anchor_index
                                .entry(idx + 1)
                                .and_modify(|v| v.push_str(&text))
                                .or_insert(text.clone());
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut last_assistant_text: Option<String> = None;

        for (idx, item) in input.iter().enumerate() {
            match item {
                ResponseItem::Message { role, content, .. } => {
                    let mut text = String::new();
                    let mut items: Vec<Value> = Vec::new();
                    let mut saw_image = false;

                    for c in content {
                        match c {
                            ContentItem::InputText { text: t }
                            | ContentItem::OutputText { text: t } => {
                                text.push_str(t);
                                items.push(json!({"type":"text","text": t}));
                            }
                            ContentItem::InputImage { image_url } => {
                                saw_image = true;
                                items.push(
                                    json!({"type":"image_url","image_url": {"url": image_url}}),
                                );
                            }
                        }
                    }

                    if role == "assistant" {
                        if let Some(prev) = &last_assistant_text
                            && prev == &text
                        {
                            continue;
                        }
                        last_assistant_text = Some(text.clone());
                    }

                    let content_value = if role == "assistant" {
                        json!(text)
                    } else if saw_image {
                        json!(items)
                    } else {
                        json!(text)
                    };

                    let effective_role = if is_role_supported(role, &self.supported_message_roles)
                        && !self.model_slug.map_or(false, |slug| provider_explicitly_unsupports_role(slug, role))
                    {
                        role.to_string()
                    } else {
                        get_fallback_role(role).to_string()
                    };

                    let mut msg = json!({"role": effective_role, "content": content_value});
                    if role == "assistant" {
                        if let Some(obj) = msg.as_object_mut() {
                            let reasoning_text = reasoning_by_anchor_index.get(&idx).map(String::as_str).unwrap_or("");
                            // Some providers (e.g. DeepSeek) require the field on every assistant
                            // message even when empty — omitting it causes an API error.
                            if !reasoning_text.is_empty() || reasoning_field == "reasoning_content" {
                                obj.insert(reasoning_field.to_string(), json!(reasoning_text));
                            }
                        }
                    }
                    messages.push(msg);
                }
                ResponseItem::FunctionCall {
                    name,
                    arguments,
                    call_id,
                    ..
                } => {
                    let reasoning = reasoning_by_anchor_index.get(&idx).map(String::as_str);
                    let tool_call = json!({
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments,
                        }
                    });
                    push_tool_call_message(&mut messages, tool_call, reasoning, reasoning_field);
                }
                ResponseItem::LocalShellCall {
                    id,
                    call_id: _,
                    status,
                    action,
                } => {
                    let reasoning = reasoning_by_anchor_index.get(&idx).map(String::as_str);
                    let tool_call = json!({
                        "id": id.clone().unwrap_or_default(),
                        "type": "local_shell_call",
                        "status": status,
                        "action": action,
                    });
                    push_tool_call_message(&mut messages, tool_call, reasoning, reasoning_field);
                }
                ResponseItem::FunctionCallOutput { call_id, output } => {
                    let content_value = if let Some(items) = &output.content_items {
                        let mapped: Vec<Value> = items
                            .iter()
                            .map(|it| match it {
                                FunctionCallOutputContentItem::InputText { text } => {
                                    json!({"type":"text","text": text})
                                }
                                FunctionCallOutputContentItem::InputImage { image_url } => {
                                    json!({"type":"image_url","image_url": {"url": image_url}})
                                }
                            })
                            .collect();
                        json!(mapped)
                    } else {
                        json!(output.content)
                    };

                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": content_value,
                    }));
                }
                ResponseItem::CustomToolCall {
                    id,
                    call_id: _,
                    name,
                    input,
                    status: _,
                } => {
                    let tool_call = json!({
                        "id": id,
                        "type": "custom",
                        "custom": {
                            "name": name,
                            "input": input,
                        }
                    });
                    let reasoning = reasoning_by_anchor_index.get(&idx).map(String::as_str);
                    push_tool_call_message(&mut messages, tool_call, reasoning, reasoning_field);
                }
                ResponseItem::CustomToolCallOutput { call_id, output } => {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": output,
                    }));
                }
                ResponseItem::GhostSnapshot { .. } => {
                    continue;
                }
                ResponseItem::Reasoning { .. }
                | ResponseItem::WebSearchCall { .. }
                | ResponseItem::Other
                | ResponseItem::Compaction { .. } => {
                    continue;
                }
            }
        }

        let payload = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": {"include_usage": true},
            "tools": self.tools,
        });

        let mut headers = build_conversation_headers(self.conversation_id);
        if let Some(subagent) = subagent_header(&self.session_source) {
            insert_header(&mut headers, "x-openai-subagent", &subagent);
        }

        Ok(ChatRequest {
            body: payload,
            headers,
        })
    }
}

fn push_tool_call_message(messages: &mut Vec<Value>, tool_call: Value, reasoning: Option<&str>, reasoning_field: &str) {
    // Chat Completions requires that tool calls are grouped into a single assistant message
    // (with `tool_calls: [...]`) followed by tool role responses.
    if let Some(Value::Object(obj)) = messages.last_mut()
        && obj.get("role").and_then(Value::as_str) == Some("assistant")
    {
        if let Some(tool_calls) = obj.get_mut("tool_calls").and_then(Value::as_array_mut) {
            // Append to existing tool call message (content may be null or text).
            // This also handles the case where a text+tool_calls assistant message was
            // already produced by the fold below and a second tool call now arrives.
            tool_calls.push(tool_call);
            if let Some(reasoning) = reasoning {
                if let Some(Value::String(existing)) = obj.get_mut(reasoning_field) {
                    if !existing.is_empty() {
                        existing.push('\n');
                    }
                    existing.push_str(reasoning);
                } else {
                    obj.insert(reasoning_field.to_string(), Value::String(reasoning.to_string()));
                }
            }
            return;
        } else if !obj.contains_key("tool_calls") {
            // The preceding assistant message has no tool_calls yet (text-only or empty).
            // Fold the first tool call into it to produce a single assistant message,
            // avoiding two consecutive assistant messages that providers like Minimax
            // reject with error 2013.
            obj.insert("tool_calls".to_string(), json!([tool_call]));
            if let Some(reasoning) = reasoning {
                if let Some(Value::String(existing)) = obj.get_mut(reasoning_field) {
                    if !existing.is_empty() {
                        existing.push('\n');
                    }
                    existing.push_str(reasoning);
                } else {
                    obj.insert(reasoning_field.to_string(), Value::String(reasoning.to_string()));
                }
            }
            return;
        }
    }

    let mut msg = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [tool_call],
    });
    if let Some(obj) = msg.as_object_mut() {
        let reasoning_text = reasoning.unwrap_or("");
        if !reasoning_text.is_empty() || reasoning_field == "reasoning_content" {
            obj.insert(reasoning_field.to_string(), json!(reasoning_text));
        }
    }
    messages.push(msg);
}

/// Check if a message role is supported by the model.
fn is_role_supported(role: &str, supported_roles: &[String]) -> bool {
    supported_roles.iter().any(|r| r == role)
}

/// Get the fallback role for an unsupported role.
/// Maps "developer" to "system" for providers that don't support it.
fn get_fallback_role(role: &str) -> &str {
    match role {
        "developer" => "system",
        other => other,
    }
}

/// Check if a provider/model identifier is a DeepSeek variant (including pseudonyms).
fn is_deepseek_variant(identifier: &str) -> bool {
    let lower = identifier.to_lowercase();
    lower.contains("deepseek")
        || lower.contains("big-pickle")
        || lower.contains("deep-seek")
        || (lower.contains("r1") && lower.contains("deep"))
}

/// Check if a model explicitly does NOT support a role based on provider.
/// Some providers like DeepSeek don't support "developer" role even if model definitions claim it.
fn provider_explicitly_unsupports_role(model_slug: &str, role: &str) -> bool {
    if role != "developer" {
        return false;
    }

    // DeepSeek and derivatives don't support the "developer" role
    is_deepseek_variant(model_slug)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::RetryConfig;
    use crate::provider::WireApi;
    use codex_protocol::models::FunctionCallOutputPayload;
    use codex_protocol::protocol::SessionSource;
    use codex_protocol::protocol::SubAgentSource;
    use http::HeaderValue;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn provider() -> Provider {
        Provider {
            name: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            query_params: None,
            wire: WireApi::Chat,
            headers: HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(10),
                retry_429: false,
                retry_5xx: true,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn attaches_conversation_and_subagent_headers() {
        let prompt_input = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "hi".to_string(),
            }],
            end_turn: None,
            phase: None,
        }];
        let req = ChatRequestBuilder::new("gpt-test", "inst", &prompt_input, &[])
            .conversation_id(Some("conv-1".into()))
            .session_source(Some(SessionSource::SubAgent(SubAgentSource::Review)))
            .build(&provider())
            .expect("request");

        assert_eq!(
            req.headers.get("session_id"),
            Some(&HeaderValue::from_static("conv-1"))
        );
        assert_eq!(
            req.headers.get("x-openai-subagent"),
            Some(&HeaderValue::from_static("review"))
        );
    }

    #[test]
    fn groups_consecutive_tool_calls_into_a_single_assistant_message() {
        let prompt_input = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "read these".to_string(),
                }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"path":"a.txt"}"#.to_string(),
                call_id: "call-a".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"path":"b.txt"}"#.to_string(),
                call_id: "call-b".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"path":"c.txt"}"#.to_string(),
                call_id: "call-c".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-a".to_string(),
                output: FunctionCallOutputPayload {
                    content: "A".to_string(),
                    ..Default::default()
                },
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-b".to_string(),
                output: FunctionCallOutputPayload {
                    content: "B".to_string(),
                    ..Default::default()
                },
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-c".to_string(),
                output: FunctionCallOutputPayload {
                    content: "C".to_string(),
                    ..Default::default()
                },
            },
        ];

        let req = ChatRequestBuilder::new("gpt-test", "inst", &prompt_input, &[])
            .build(&provider())
            .expect("request");

        let messages = req
            .body
            .get("messages")
            .and_then(|v| v.as_array())
            .expect("messages array");
        // system + user + assistant(tool_calls=[...]) + 3 tool outputs
        assert_eq!(messages.len(), 6);

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");

        let tool_calls_msg = &messages[2];
        assert_eq!(tool_calls_msg["role"], "assistant");
        assert_eq!(tool_calls_msg["content"], serde_json::Value::Null);
        let tool_calls = tool_calls_msg["tool_calls"]
            .as_array()
            .expect("tool_calls array");
        assert_eq!(tool_calls.len(), 3);
        assert_eq!(tool_calls[0]["id"], "call-a");
        assert_eq!(tool_calls[1]["id"], "call-b");
        assert_eq!(tool_calls[2]["id"], "call-c");

        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call-a");
        assert_eq!(messages[4]["role"], "tool");
        assert_eq!(messages[4]["tool_call_id"], "call-b");
        assert_eq!(messages[5]["role"], "tool");
        assert_eq!(messages[5]["tool_call_id"], "call-c");
    }

    #[test]
    fn folds_multiple_tool_calls_into_text_assistant_message() {
        // Regression: when an assistant message with text content is followed by two or
        // more tool calls, all calls must land in a single assistant message.  Previously
        // the second call fell through and created a second consecutive assistant message,
        // which providers like Minimax reject with error 2013.
        let prompt_input = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "go".to_string(),
                }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "sure, let me read both files".to_string(),
                }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"path":"a.txt"}"#.to_string(),
                call_id: "call-a".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: r#"{"path":"b.txt"}"#.to_string(),
                call_id: "call-b".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-a".to_string(),
                output: FunctionCallOutputPayload {
                    content: "A".to_string(),
                    ..Default::default()
                },
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-b".to_string(),
                output: FunctionCallOutputPayload {
                    content: "B".to_string(),
                    ..Default::default()
                },
            },
        ];

        let req = ChatRequestBuilder::new("gpt-test", "inst", &prompt_input, &[])
            .build(&provider())
            .expect("request");

        let messages = req
            .body
            .get("messages")
            .and_then(|v| v.as_array())
            .expect("messages array");
        // system + user + assistant(text + tool_calls=[a,b]) + tool(a) + tool(b)
        assert_eq!(messages.len(), 5, "expected exactly 5 messages, got: {messages:?}");

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");

        let asst = &messages[2];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["content"], "sure, let me read both files");
        let tool_calls = asst["tool_calls"].as_array().expect("tool_calls array");
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0]["id"], "call-a");
        assert_eq!(tool_calls[1]["id"], "call-b");

        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call-a");
        assert_eq!(messages[4]["role"], "tool");
        assert_eq!(messages[4]["tool_call_id"], "call-b");
    }

    #[test]
    fn deepseek_always_includes_reasoning_content_on_assistant_messages() {
        // Regression: DeepSeek requires `reasoning_content` on every assistant message
        // in the history, even when empty.  Omitting it causes:
        // "The `reasoning_content` in the thinking mode must be passed back to the API."
        let prompt_input = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: "hi".to_string() }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: "hello".to_string() }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: "second turn".to_string() }],
                end_turn: None,
                phase: None,
            },
        ];

        let builder = ChatRequestBuilder::new("big-pickle", "sys", &prompt_input, &[])
            .reasoning_field_name(Some("reasoning_content"));

        let request = builder.build(&provider()).unwrap();
        let messages = request.body["messages"].as_array().unwrap();

        // The assistant message must have `reasoning_content` even though no Reasoning item existed
        let assistant_msg = messages.iter().find(|m| m["role"] == "assistant").unwrap();
        assert!(
            assistant_msg.get("reasoning_content").is_some(),
            "DeepSeek assistant message must include reasoning_content (got: {})", assistant_msg
        );
        assert_eq!(assistant_msg["reasoning_content"], "", "reasoning_content must be empty string when no reasoning");
    }

    #[test]
    fn deepseek_includes_reasoning_text_when_present() {
        let prompt_input = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: "hi".to_string() }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Reasoning {
                id: String::new(),
                summary: vec![],
                content: Some(vec![ReasoningItemContent::ReasoningText {
                    text: "let me think".to_string(),
                }]),
                encrypted_content: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: "done".to_string() }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: "next".to_string() }],
                end_turn: None,
                phase: None,
            },
        ];

        let builder = ChatRequestBuilder::new("big-pickle", "sys", &prompt_input, &[])
            .reasoning_field_name(Some("reasoning_content"));

        let request = builder.build(&provider()).unwrap();
        let messages = request.body["messages"].as_array().unwrap();

        let assistant_msg = messages.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant_msg["reasoning_content"], "let me think");
    }

    #[test]
    fn deepseek_model_slug_triggers_reasoning_content_even_with_non_deepseek_provider() {
        // Regression: when DeepSeek-R1 is accessed via a generic provider (e.g. Ollama,
        // NVIDIA NIM) the provider name won't contain "deepseek", but the model slug
        // does.  We must still include `reasoning_content` on every assistant message.
        let prompt_input = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: "hi".to_string() }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: "hello".to_string() }],
                end_turn: None,
                phase: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: "second turn".to_string() }],
                end_turn: None,
                phase: None,
            },
        ];

        // Provider name is "openai" (non-deepseek), but model slug identifies the model.
        let builder = ChatRequestBuilder::new("deepseek-r1", "sys", &prompt_input, &[])
            .model_slug("deepseek-r1");

        let request = builder.build(&provider()).unwrap();
        let messages = request.body["messages"].as_array().unwrap();

        let assistant_msg = messages.iter().find(|m| m["role"] == "assistant").unwrap();
        assert!(
            assistant_msg.get("reasoning_content").is_some(),
            "DeepSeek model slug must trigger reasoning_content inclusion (got: {})", assistant_msg
        );
        assert_eq!(assistant_msg["reasoning_content"], "");
    }
}
