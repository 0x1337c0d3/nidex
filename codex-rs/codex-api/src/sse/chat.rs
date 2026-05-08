use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::telemetry::SseTelemetry;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::Stream;
use futures::StreamExt;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;
use tracing::warn;

pub(crate) fn spawn_chat_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    _turn_state: Option<Arc<OnceLock<String>>>,
) -> ResponseStream {
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(async move {
        process_chat_sse(stream_response.bytes, tx_event, idle_timeout, telemetry).await;
    });
    ResponseStream { rx_event }
}

/// Processes Server-Sent Events from the legacy Chat Completions streaming API.
///
/// The upstream protocol terminates a streaming response with a final sentinel event
/// (`data: [DONE]`). Historically, some of our test stubs have emitted `data: DONE`
/// (without brackets) instead.
///
/// `eventsource_stream` delivers these sentinels as regular events rather than signaling
/// end-of-stream. If we try to parse them as JSON, we log and skip them, then keep
/// polling for more events.
///
/// On servers that keep the HTTP connection open after emitting the sentinel (notably
/// wiremock on Windows), skipping the sentinel means we never emit `ResponseEvent::Completed`.
/// Higher-level workflows/tests that wait for completion before issuing subsequent model
/// calls will then stall, which shows up as "expected N requests, got 1" verification
/// failures in the mock server.
#[derive(Default, Debug)]
struct ToolCallState {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub async fn process_chat_sse<S>(
    stream: S,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<std::sync::Arc<dyn SseTelemetry>>,
) where
    S: Stream<Item = Result<bytes::Bytes, codex_client::TransportError>> + Unpin,
{
    let mut stream = stream.eventsource();

    let mut tool_calls: HashMap<usize, ToolCallState> = HashMap::new();
    let mut tool_call_order: Vec<usize> = Vec::new();
    let mut tool_call_order_seen: HashSet<usize> = HashSet::new();
    let mut tool_call_index_by_id: HashMap<String, usize> = HashMap::new();
    let mut next_tool_call_index = 0usize;
    let mut last_tool_call_index: Option<usize> = None;
    let mut assistant_item: Option<ResponseItem> = None;
    let mut reasoning_item: Option<ResponseItem> = None;
    let mut completed_sent = false;
    let mut saw_tool_calls_finish = false;

    async fn flush_and_complete(
        tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
        reasoning_item: &mut Option<ResponseItem>,
        assistant_item: &mut Option<ResponseItem>,
        tool_calls: &mut HashMap<usize, ToolCallState>,
        tool_call_order: &mut Vec<usize>,
        tool_call_order_seen: &mut HashSet<usize>,
        truncated: bool,
    ) {
        let mut emitted_any = false;

        if let Some(reasoning) = reasoning_item.take() {
            let _ = tx_event
                .send(Ok(ResponseEvent::OutputItemDone(reasoning)))
                .await;
            emitted_any = true;
        }

        if let Some(assistant) = assistant_item.take() {
            let _ = tx_event
                .send(Ok(ResponseEvent::OutputItemDone(assistant)))
                .await;
            emitted_any = true;
        }

        // Emit any tool calls that were being streamed when the connection dropped.
        // Without this, a FunctionCall lands in history with no FunctionCallOutput,
        // causing providers like Minimax to reject the next request (error 2013).
        for index in tool_call_order.drain(..) {
            let Some(state) = tool_calls.remove(&index) else {
                continue;
            };
            tool_call_order_seen.remove(&index);
            let ToolCallState { id, name, arguments } = state;
            let Some(name) = name else {
                debug!("Skipping tool call at index {index} because name is missing");
                continue;
            };
            let item = ResponseItem::FunctionCall {
                id: None,
                name,
                arguments,
                call_id: id.unwrap_or_else(|| format!("tool-call-{index}")),
            };
            let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
            emitted_any = true;
        }

        // If the stream closed prematurely (no [DONE]) and sent nothing useful,
        // surface an error so the caller can report it rather than treating the
        // empty response as a successful turn completion.
        if truncated && !emitted_any {
            let _ = tx_event
                .send(Err(ApiError::Stream(
                    "SSE stream closed without [DONE] and without any content".into(),
                )))
                .await;
            return;
        }

        let _ = tx_event
            .send(Ok(ResponseEvent::Completed {
                response_id: String::new(),
                token_usage: None,
            }))
            .await;
    }

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(t) = telemetry.as_ref() {
            t.on_sse_poll(&response, start.elapsed());
        }
        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                warn!("SSE transport error: {e}");
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => {
                if !completed_sent {
                    debug!("SSE stream ended without [DONE] sentinel; flushing");
                    flush_and_complete(&tx_event, &mut reasoning_item, &mut assistant_item, &mut tool_calls, &mut tool_call_order, &mut tool_call_order_seen, true).await;
                }
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                    .await;
                return;
            }
        };

        trace!("SSE event: {}", sse.data);

        let data = sse.data.trim();

        if data.is_empty() {
            continue;
        }

        if data == "[DONE]" || data == "DONE" {
            if !completed_sent {
                flush_and_complete(&tx_event, &mut reasoning_item, &mut assistant_item, &mut tool_calls, &mut tool_call_order, &mut tool_call_order_seen, false).await;
            }
            return;
        }

        let value: serde_json::Value = match serde_json::from_str(data) {
            Ok(val) => val,
            Err(err) => {
                debug!(
                    "Failed to parse ChatCompletions SSE event: {err}, data: {}",
                    data
                );
                continue;
            }
        };

        let Some(choices) = value.get("choices").and_then(|c| c.as_array()) else {
            if value.get("error").is_some() {
                // Propagate provider errors (e.g. Minimax 2013) so the caller sees
                // the real error message rather than a generic "stream closed" error.
                warn!("SSE error from server: {value}");
                let _ = tx_event
                    .send(Err(ApiError::Stream(format!("Provider error: {value}"))))
                    .await;
                return;
            }
            debug!("SSE event has no choices field, skipping: {value}");
            continue;
        };

        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                // Check for reasoning (OpenAI) or reasoning_content (DeepSeek)
                let reasoning = delta.get("reasoning").or_else(|| delta.get("reasoning_content"));

                if let Some(reasoning) = reasoning {
                    if let Some(text) = reasoning.as_str() {
                        append_reasoning_text(&tx_event, &mut reasoning_item, text.to_string())
                            .await;
                    } else if let Some(text) = reasoning.get("text").and_then(|v| v.as_str()) {
                        append_reasoning_text(&tx_event, &mut reasoning_item, text.to_string())
                            .await;
                    } else if let Some(text) = reasoning.get("content").and_then(|v| v.as_str()) {
                        append_reasoning_text(&tx_event, &mut reasoning_item, text.to_string())
                            .await;
                    }
                }

                if let Some(content) = delta.get("content") {
                    if content.is_array() {
                        for item in content.as_array().unwrap_or(&vec![]) {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str())
                                && !text.is_empty()
                            {
                                append_assistant_text(
                                    &tx_event,
                                    &mut assistant_item,
                                    text.to_string(),
                                )
                                .await;
                            }
                        }
                    } else if let Some(text) = content.as_str()
                        && !text.is_empty()
                    {
                        append_assistant_text(&tx_event, &mut assistant_item, text.to_string())
                            .await;
                    }
                }

                if let Some(tool_call_values) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                    for tool_call in tool_call_values {
                        let mut index = tool_call
                            .get("index")
                            .and_then(serde_json::Value::as_u64)
                            .map(|i| i as usize);

                        let mut call_id_for_lookup = None;
                        if let Some(call_id) = tool_call.get("id").and_then(|i| i.as_str()) {
                            call_id_for_lookup = Some(call_id.to_string());
                            if let Some(existing) = tool_call_index_by_id.get(call_id) {
                                index = Some(*existing);
                            } else {
                                // New id: if there's already a tool call at the proposed index with a different id,
                                // force allocation of a fresh index instead of reusing the existing one
                                if let Some(proposed_idx) = index {
                                    if let Some(existing_call) = tool_calls.get(&proposed_idx) {
                                        if let Some(existing_id) = &existing_call.id {
                                            if existing_id != call_id {
                                                index = None;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if index.is_none() && call_id_for_lookup.is_none() {
                            index = last_tool_call_index;
                        }

                        let index = index.unwrap_or_else(|| {
                            while tool_calls.contains_key(&next_tool_call_index) {
                                next_tool_call_index += 1;
                            }
                            let idx = next_tool_call_index;
                            next_tool_call_index += 1;
                            idx
                        });

                        let call_state = tool_calls.entry(index).or_default();
                        if tool_call_order_seen.insert(index) {
                            tool_call_order.push(index);
                        }

                        if let Some(id) = tool_call.get("id").and_then(|i| i.as_str()) {
                            call_state.id.get_or_insert_with(|| id.to_string());
                            tool_call_index_by_id.entry(id.to_string()).or_insert(index);
                        }

                        if let Some(func) = tool_call.get("function") {
                            if let Some(fname) = func.get("name").and_then(|n| n.as_str())
                                && !fname.is_empty()
                            {
                                call_state.name.get_or_insert_with(|| fname.to_string());
                            }
                            if let Some(arguments) = func.get("arguments").and_then(|a| a.as_str())
                            {
                                call_state.arguments.push_str(arguments);
                            }
                        }

                        last_tool_call_index = Some(index);
                    }
                }
            }

            if let Some(message) = choice.get("message") {
                let reasoning = message.get("reasoning").or_else(|| message.get("reasoning_content"));

                if let Some(reasoning) = reasoning {
                    if let Some(text) = reasoning.as_str() {
                        append_reasoning_text(&tx_event, &mut reasoning_item, text.to_string()).await;
                    } else if let Some(text) = reasoning.get("text").and_then(|v| v.as_str()) {
                        append_reasoning_text(&tx_event, &mut reasoning_item, text.to_string()).await;
                    } else if let Some(text) = reasoning.get("content").and_then(|v| v.as_str()) {
                        append_reasoning_text(&tx_event, &mut reasoning_item, text.to_string()).await;
                    }
                }
            }

            let finish_reason = choice.get("finish_reason").and_then(|r| r.as_str());
            if finish_reason == Some("stop") {
                if let Some(reasoning) = reasoning_item.take() {
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemDone(reasoning)))
                        .await;
                }

                if let Some(assistant) = assistant_item.take() {
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemDone(assistant)))
                        .await;
                }
                if !completed_sent {
                    let _ = tx_event
                        .send(Ok(ResponseEvent::Completed {
                            response_id: String::new(),
                            token_usage: parse_usage(&value),
                        }))
                        .await;
                    completed_sent = true;
                }
                continue;
            }

            if finish_reason == Some("length") {
                let _ = tx_event.send(Err(ApiError::ContextWindowExceeded)).await;
                return;
            }

            if finish_reason == Some("tool_calls") {
                if let Some(reasoning) = reasoning_item.take() {
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemDone(reasoning)))
                        .await;
                }
                saw_tool_calls_finish = true;
            }
        }

        if saw_tool_calls_finish {
            // Emit any assistant text BEFORE the tool call items so history is ordered
            // text → FunctionCall → FunctionCallOutput.  If the text were emitted after
            // the calls it would land between the FunctionCall and its result, which
            // causes error 2013 on providers like Minimax that require the tool result
            // to immediately follow the assistant message that issued the call.
            if let Some(assistant) = assistant_item.take() {
                let _ = tx_event
                    .send(Ok(ResponseEvent::OutputItemDone(assistant)))
                    .await;
            }
            for index in tool_call_order.drain(..) {
                let Some(state) = tool_calls.remove(&index) else {
                    continue;
                };
                tool_call_order_seen.remove(&index);
                let ToolCallState {
                    id,
                    name,
                    arguments,
                } = state;
                let Some(name) = name else {
                    debug!("Skipping tool call at index {index} because name is missing");
                    continue;
                };
                let item = ResponseItem::FunctionCall {
                    id: None,
                    name,
                    arguments,
                    call_id: id.unwrap_or_else(|| format!("tool-call-{index}")),
                };
                let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
            }
            if !completed_sent {
                let _ = tx_event
                    .send(Ok(ResponseEvent::Completed {
                        response_id: String::new(),
                        token_usage: parse_usage(&value),
                    }))
                    .await;
                completed_sent = true;
            }

            saw_tool_calls_finish = false;
        }
    }
}


/// Parse `usage` from an SSE event body (OpenAI streaming format).
fn parse_usage(value: &serde_json::Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        cached_input_tokens: usage.get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        output_tokens: usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        reasoning_output_tokens: usage.get("completion_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        total_tokens: usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
    })
        .filter(|u| u.total_tokens > 0 || u.input_tokens > 0 || u.output_tokens > 0)
}

async fn append_assistant_text(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    assistant_item: &mut Option<ResponseItem>,
    text: String,
) {
    if assistant_item.is_none() {
        let item = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![],
            end_turn: None,
            phase: None,
        };
        *assistant_item = Some(item.clone());
        let _ = tx_event
            .send(Ok(ResponseEvent::OutputItemAdded(item)))
            .await;
    }

    if let Some(ResponseItem::Message { content, .. }) = assistant_item {
        content.push(ContentItem::OutputText { text: text.clone() });
        let _ = tx_event
            .send(Ok(ResponseEvent::OutputTextDelta(text.clone())))
            .await;
    }
}

async fn append_reasoning_text(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    reasoning_item: &mut Option<ResponseItem>,
    text: String,
) {
    if reasoning_item.is_none() {
        let item = ResponseItem::Reasoning {
            id: String::new(),
            summary: Vec::new(),
            content: Some(vec![]),
            encrypted_content: None,
        };
        *reasoning_item = Some(item.clone());
        let _ = tx_event
            .send(Ok(ResponseEvent::OutputItemAdded(item)))
            .await;
    }

    if let Some(ResponseItem::Reasoning {
        content: Some(content),
        ..
    }) = reasoning_item
    {
        let content_index = content.len() as i64;
        content.push(ReasoningItemContent::ReasoningText { text: text.clone() });

        let _ = tx_event
            .send(Ok(ResponseEvent::ReasoningContentDelta {
                delta: text.clone(),
                content_index,
            }))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use codex_protocol::models::ResponseItem;
    use futures::TryStreamExt;
    use serde_json::json;
    use tokio::sync::mpsc;
    use tokio_util::io::ReaderStream;

    fn build_body(events: &[serde_json::Value]) -> String {
        let mut body = String::new();
        for e in events {
            body.push_str(&format!("event: message\ndata: {e}\n\n"));
        }
        body
    }

    /// Regression test: the stream should complete when we see a `[DONE]` sentinel.
    ///
    /// This is important for tests/mocks that don't immediately close the underlying
    /// connection after emitting the sentinel.
    #[tokio::test]
    async fn completes_on_done_sentinel_without_json() {
        let events = collect_events("event: message\ndata: [DONE]\n\n").await;
        assert_matches!(&events[..], [ResponseEvent::Completed { .. }]);
    }

    async fn collect_events(body: &str) -> Vec<ResponseEvent> {
        collect_results(body)
            .await
            .into_iter()
            .map(|r| r.expect("stream error"))
            .collect()
    }

    async fn collect_results(body: &str) -> Vec<Result<ResponseEvent, ApiError>> {
        let reader = ReaderStream::new(std::io::Cursor::new(body.to_string()))
            .map_err(|err| codex_client::TransportError::Network(err.to_string()));
        let (tx, mut rx) = mpsc::channel::<Result<ResponseEvent, ApiError>>(16);
        tokio::spawn(process_chat_sse(
            reader,
            tx,
            Duration::from_millis(1000),
            None,
        ));

        let mut out = Vec::new();
        while let Some(ev) = rx.recv().await {
            out.push(ev);
        }
        out
    }


    #[tokio::test]
    async fn concatenates_tool_call_arguments_across_deltas() {
        let delta_name = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_a",
                        "index": 0,
                        "function": { "name": "do_a" }
                    }]
                }
            }]
        });

        let delta_args_1 = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "{ \"foo\":" }
                    }]
                }
            }]
        });

        let delta_args_2 = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "1}" }
                    }]
                }
            }]
        });

        let finish = json!({
            "choices": [{
                "finish_reason": "tool_calls"
            }]
        });

        let body = build_body(&[delta_name, delta_args_1, delta_args_2, finish]);
        let events = collect_events(&body).await;
        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, name, arguments, .. }),
                ResponseEvent::Completed { .. }
            ] if call_id == "call_a" && name == "do_a" && arguments == "{ \"foo\":1}"
        );
    }

    #[tokio::test]
    async fn emits_multiple_tool_calls() {
        let delta_a = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "do_a", "arguments": "{\"foo\":1}" }
                    }]
                }
            }]
        });

        let delta_b = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_b",
                        "function": { "name": "do_b", "arguments": "{\"bar\":2}" }
                    }]
                }
            }]
        });

        let finish = json!({
            "choices": [{
                "finish_reason": "tool_calls"
            }]
        });

        let body = build_body(&[delta_a, delta_b, finish]);
        let events = collect_events(&body).await;
        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id: call_a, name: name_a, arguments: args_a, .. }),
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id: call_b, name: name_b, arguments: args_b, .. }),
                ResponseEvent::Completed { .. }
            ] if call_a == "call_a" && name_a == "do_a" && args_a == "{\"foo\":1}" && call_b == "call_b" && name_b == "do_b" && args_b == "{\"bar\":2}"
        );
    }

    #[tokio::test]
    async fn emits_tool_calls_for_multiple_choices() {
        let payload = json!({
            "choices": [
                {
                    "delta": {
                        "tool_calls": [{
                            "id": "call_a",
                            "index": 0,
                            "function": { "name": "do_a", "arguments": "{}" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                },
                {
                    "delta": {
                        "tool_calls": [{
                            "id": "call_b",
                            "index": 0,
                            "function": { "name": "do_b", "arguments": "{}" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }
            ]
        });

        let body = build_body(&[payload]);
        let events = collect_events(&body).await;
        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id: call_a, name: name_a, arguments: args_a, .. }),
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id: call_b, name: name_b, arguments: args_b, .. }),
                ResponseEvent::Completed { .. }
            ] if call_a == "call_a" && name_a == "do_a" && args_a == "{}" && call_b == "call_b" && name_b == "do_b" && args_b == "{}"
        );
    }

    #[tokio::test]
    async fn merges_tool_calls_by_index_when_id_missing_on_subsequent_deltas() {
        let delta_with_id = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_a",
                        "function": { "name": "do_a", "arguments": "{ \"foo\":" }
                    }]
                }
            }]
        });

        let delta_without_id = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "1}" }
                    }]
                }
            }]
        });

        let finish = json!({
            "choices": [{
                "finish_reason": "tool_calls"
            }]
        });

        let body = build_body(&[delta_with_id, delta_without_id, finish]);
        let events = collect_events(&body).await;
        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, name, arguments, .. }),
                ResponseEvent::Completed { .. }
            ] if call_id == "call_a" && name == "do_a" && arguments == "{ \"foo\":1}"
        );
    }

    #[tokio::test]
    async fn preserves_tool_call_name_when_empty_deltas_arrive() {
        let delta_with_name = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "do_a" }
                    }]
                }
            }]
        });

        let delta_with_empty_name = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "", "arguments": "{}" }
                    }]
                }
            }]
        });

        let finish = json!({
            "choices": [{
                "finish_reason": "tool_calls"
            }]
        });

        let body = build_body(&[delta_with_name, delta_with_empty_name, finish]);
        let events = collect_events(&body).await;
        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { name, arguments, .. }),
                ResponseEvent::Completed { .. }
            ] if name == "do_a" && arguments == "{}"
        );
    }

    #[tokio::test]
    async fn emits_tool_calls_even_when_content_and_reasoning_present() {
        let delta_content_and_tools = json!({
            "choices": [{
                "delta": {
                    "content": [{"text": "hi"}],
                    "reasoning": "because",
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "do_a", "arguments": "{}" }
                    }]
                }
            }]
        });

        let finish = json!({
            "choices": [{
                "finish_reason": "tool_calls"
            }]
        });

        let body = build_body(&[delta_content_and_tools, finish]);
        let events = collect_events(&body).await;

        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemAdded(ResponseItem::Reasoning { .. }),
                ResponseEvent::ReasoningContentDelta { .. },
                ResponseEvent::OutputItemAdded(ResponseItem::Message { .. }),
                ResponseEvent::OutputTextDelta(delta),
                ResponseEvent::OutputItemDone(ResponseItem::Reasoning { .. }),
                // Message (assistant text) is emitted BEFORE tool calls so that history
                // ordering is: text → FunctionCall → FunctionCallOutput.  This ensures
                // the tool result immediately follows the tool call message when the
                // history is serialized, satisfying providers like Minimax (error 2013).
                ResponseEvent::OutputItemDone(ResponseItem::Message { .. }),
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, name, .. }),
                ResponseEvent::Completed { .. }
            ] if delta == "hi" && call_id == "call_a" && name == "do_a"
        );
    }

    #[tokio::test]
    async fn drops_partial_tool_calls_on_stop_finish_reason() {
        let delta_tool = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "do_a", "arguments": "{}" }
                    }]
                }
            }]
        });

        let finish_stop = json!({
            "choices": [{
                "finish_reason": "stop"
            }]
        });

        let body = build_body(&[delta_tool, finish_stop]);
        let events = collect_events(&body).await;

        assert!(!events.iter().any(|ev| {
            matches!(
                ev,
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })
            )
        }));
        assert_matches!(events.last(), Some(ResponseEvent::Completed { .. }));
    }

    /// Regression: when a provider sends `"content": ""` in a delta alongside tool_calls,
    /// we must NOT create a phantom empty assistant message.  Before the fix, the empty
    /// string triggered `append_assistant_text`, which created an `assistant_item`.  That
    /// item was then emitted as `OutputItemDone` when `finish_reason: tool_calls` arrived,
    /// recording an empty Message in history.  The next request body then contained two
    /// consecutive assistant messages (the empty one + the tool-calls one), which Minimax
    /// rejects with error 2013.
    #[tokio::test]
    async fn empty_content_string_does_not_create_phantom_assistant_message() {
        let delta_empty_content_with_tool = json!({
            "choices": [{
                "delta": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "do_a", "arguments": "{}" }
                    }]
                }
            }]
        });

        let finish = json!({
            "choices": [{
                "finish_reason": "tool_calls"
            }]
        });

        let body = build_body(&[delta_empty_content_with_tool, finish]);
        let events = collect_events(&body).await;

        // Must not emit a Message OutputItemDone — only FunctionCall and Completed.
        assert!(
            !events.iter().any(|ev| matches!(
                ev,
                ResponseEvent::OutputItemDone(ResponseItem::Message { .. })
            )),
            "phantom empty assistant message was emitted: {events:?}"
        );
        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, name, .. }),
                ResponseEvent::Completed { .. }
            ] if call_id == "call_a" && name == "do_a"
        );
    }

    /// Regression: when the SSE stream closes without a [DONE] sentinel and a tool call
    /// was being streamed (no finish_reason=tool_calls arrived), the tool call must still
    /// be emitted.  Previously it was silently dropped by flush_and_complete, leaving a
    /// FunctionCall in history with no FunctionCallOutput, which causes Minimax error 2013
    /// on the very next request ("tool call result does not follow tool call").
    #[tokio::test]
    async fn flushes_pending_tool_calls_when_stream_ends_without_done() {
        let delta_tool = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": "call_a",
                        "function": { "name": "do_a", "arguments": "{}" }
                    }]
                }
            }]
        });

        // Stream ends here with no finish_reason and no [DONE] — simulates a dropped
        // connection while the model is mid-response.
        let body = build_body(&[delta_tool]);
        let events = collect_events(&body).await;

        assert_matches!(
            &events[..],
            [
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, name, arguments, .. }),
                ResponseEvent::Completed { .. }
            ] if call_id == "call_a" && name == "do_a" && arguments == "{}"
        );
    }

    /// Regression: when the SSE stream contains a provider error event (no `choices`, just
    /// `{"error": {...}}`), the handler must propagate it as an `Err` rather than silently
    /// ignoring it and falling through to the generic "stream closed without [DONE]" path.
    /// Without this fix, the real Minimax error 2013 message was swallowed on the first
    /// attempt and only surfaced (as a 400 HTTP response) on the retry.
    #[tokio::test]
    async fn propagates_sse_provider_error_event() {
        let error_event = json!({
            "error": {
                "message": "Error from provider: Provider returned error",
                "code": 400,
                "metadata": {
                    "raw": "{\"type\":\"error\",\"error\":{\"type\":\"bad_request_error\",\"message\":\"invalid params, tool call result does not follow tool call (2013)\"}}",
                    "provider_name": "Minimax",
                    "is_byok": true
                }
            }
        });
        let body = build_body(&[error_event]);
        let results = collect_results(&body).await;

        assert_eq!(results.len(), 1);
        assert_matches!(&results[0], Err(ApiError::Stream(msg)) if msg.contains("Provider error"));
    }

    /// Regression: when the SSE stream closes without [DONE] and with no content at all
    /// (no tool calls, no text, no reasoning), the server has closed the connection early
    /// without a useful response.  Previously this was silently treated as a successful
    /// empty completion, causing the agent turn to end with no output or error message.
    #[tokio::test]
    async fn errors_when_stream_ends_without_done_and_no_content() {
        // An empty body — simulates a server that closes the connection immediately
        // (e.g. a Cloudflare proxy dropping the SSE stream before any model output).
        let results = collect_results("").await;

        assert_eq!(results.len(), 1);
        assert_matches!(&results[0], Err(ApiError::Stream(msg)) if msg.contains("without [DONE]"));
    }
}
