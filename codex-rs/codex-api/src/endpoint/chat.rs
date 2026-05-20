use crate::ChatRequest;
use crate::auth::AuthProvider;
use crate::common::Prompt as ApiPrompt;
use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::endpoint::streaming::StreamingClient;
use crate::error::ApiError;
use crate::provider::Provider;
use crate::provider::WireApi;
use crate::sse::chat::spawn_chat_stream;
use crate::telemetry::SseTelemetry;
use codex_client::HttpTransport;
use codex_client::RequestCompression;
use codex_client::RequestTelemetry;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use futures::Stream;
use http::HeaderMap;
use serde_json::Value;
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

pub struct ChatClient<T: HttpTransport, A: AuthProvider> {
    streaming: StreamingClient<T, A>,
}

impl<T: HttpTransport, A: AuthProvider> ChatClient<T, A> {
    pub fn new(transport: T, provider: Provider, auth: A) -> Self {
        Self {
            streaming: StreamingClient::new(transport, provider, auth),
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<std::sync::Arc<dyn RequestTelemetry>>,
        sse: Option<std::sync::Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            streaming: self.streaming.with_telemetry(request, sse),
        }
    }

    pub async fn stream_request(&self, request: ChatRequest) -> Result<ResponseStream, ApiError> {
        self.stream(request.body, request.headers).await
    }

    pub async fn stream_prompt(
        &self,
        model: &str,
        prompt: &ApiPrompt,
        conversation_id: Option<String>,
        session_source: Option<SessionSource>,
    ) -> Result<ResponseStream, ApiError> {
        self.stream_prompt_with_roles(
            model,
            prompt,
            conversation_id,
            session_source,
            None,
        )
        .await
    }

    pub async fn stream_prompt_with_roles(
        &self,
        model: &str,
        prompt: &ApiPrompt,
        conversation_id: Option<String>,
        session_source: Option<SessionSource>,
        supported_message_roles: Option<Vec<String>>,
    ) -> Result<ResponseStream, ApiError> {
        self.stream_prompt_with_roles_and_reasoning(
            model,
            prompt,
            conversation_id,
            session_source,
            supported_message_roles,
            None,
            None,
        )
        .await
    }

    pub async fn stream_prompt_with_roles_and_reasoning(
        &self,
        model: &str,
        prompt: &ApiPrompt,
        conversation_id: Option<String>,
        session_source: Option<SessionSource>,
        supported_message_roles: Option<Vec<String>>,
        reasoning_field_name: Option<&str>,
        image_url_supported: Option<bool>,
    ) -> Result<ResponseStream, ApiError> {
        use crate::requests::ChatRequestBuilder;

        let mut builder =
            ChatRequestBuilder::new(model, &prompt.instructions, &prompt.input, &prompt.tools)
                .conversation_id(conversation_id.clone())
                .session_source(session_source.clone())
                .model_slug(model)
                .reasoning_field_name(reasoning_field_name);

        if let Some(roles) = supported_message_roles.clone() {
            builder = builder.supported_message_roles(roles);
        }
        if let Some(supported) = image_url_supported {
            builder = builder.image_url_supported(supported);
        }

        let request = builder.build(self.streaming.provider())?;

        let msg_count = request
            .body
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        tracing::debug!(
            instructions_len = prompt.instructions.len(),
            instructions_starts_with = &prompt.instructions.chars().take(120).collect::<String>(),
            message_count = msg_count,
            "chat_completions request"
        );
        tracing::trace!(
            body = %request.body,
            "chat_completions request body"
        );

        self.stream_request(request).await
    }

    fn path(&self) -> &'static str {
        match self.streaming.provider().wire {
            WireApi::Chat => "chat/completions",
            _ => "responses",
        }
    }

    pub async fn stream(
        &self,
        body: Value,
        extra_headers: HeaderMap,
    ) -> Result<ResponseStream, ApiError> {
        self.streaming
            .stream(
                self.path(),
                body,
                extra_headers,
                RequestCompression::None,
                spawn_chat_stream,
                None,
            )
            .await
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum AggregateMode {
    AggregatedOnly,
    Streaming,
}

/// Stream adapter that merges token deltas into a single assistant message per turn.
pub struct AggregatedStream {
    inner: ResponseStream,
    cumulative: String,
    cumulative_reasoning: String,
    pending: VecDeque<ResponseEvent>,
    mode: AggregateMode,
}

impl Stream for AggregatedStream {
    type Item = Result<ResponseEvent, ApiError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if let Some(ev) = this.pending.pop_front() {
            return Poll::Ready(Some(Ok(ev)));
        }

        loop {
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(Some(Ok(ResponseEvent::OutputItemDone(item)))) => {
                    let is_assistant_message = matches!(
                        &item,
                        ResponseItem::Message { role, .. } if role == "assistant"
                    );

                    if is_assistant_message {
                        match this.mode {
                            AggregateMode::AggregatedOnly => {
                                if this.cumulative.is_empty()
                                    && let ResponseItem::Message { content, .. } = &item
                                    && let Some(text) = content.iter().find_map(|c| match c {
                                        ContentItem::OutputText { text } => Some(text),
                                        _ => None,
                                    })
                                {
                                    this.cumulative.push_str(text);
                                }
                                continue;
                            }
                            AggregateMode::Streaming => {
                                if this.cumulative.is_empty() {
                                    return Poll::Ready(Some(Ok(ResponseEvent::OutputItemDone(
                                        item,
                                    ))));
                                } else {
                                    continue;
                                }
                            }
                        }
                    }

                    // Before passing a function-call item downstream, flush any buffered
                    // assistant text.  The SSE handler emits text before tool calls, but
                    // AggregatedOnly mode buffers that text until Completed — reversing the
                    // order in the core's history to [FunctionCall, AssistantMessage].  When
                    // the next request is built the serialiser inserts the assistant text
                    // between the tool_calls message and the tool results, which Minimax
                    // rejects as error 2013.  Flushing here preserves text → call → result.
                    if matches!(this.mode, AggregateMode::AggregatedOnly)
                        && !this.cumulative.is_empty()
                        && matches!(
                            &item,
                            ResponseItem::FunctionCall { .. }
                                | ResponseItem::LocalShellCall { .. }
                                | ResponseItem::CustomToolCall { .. }
                        )
                    {
                        let assistant_msg = ResponseItem::Message {
                            id: None,
                            role: "assistant".to_string(),
                            content: vec![ContentItem::OutputText {
                                text: std::mem::take(&mut this.cumulative),
                            }],
                            end_turn: None,
                            phase: None,
                        };
                        this.pending.push_back(ResponseEvent::OutputItemDone(item));
                        return Poll::Ready(Some(Ok(ResponseEvent::OutputItemDone(
                            assistant_msg,
                        ))));
                    }

                    return Poll::Ready(Some(Ok(ResponseEvent::OutputItemDone(item))));
                }
                Poll::Ready(Some(Ok(ResponseEvent::ServerReasoningIncluded(included)))) => {
                    return Poll::Ready(Some(Ok(ResponseEvent::ServerReasoningIncluded(included))));
                }
                Poll::Ready(Some(Ok(ResponseEvent::RateLimits(snapshot)))) => {
                    return Poll::Ready(Some(Ok(ResponseEvent::RateLimits(snapshot))));
                }
                Poll::Ready(Some(Ok(ResponseEvent::ModelsEtag(etag)))) => {
                    return Poll::Ready(Some(Ok(ResponseEvent::ModelsEtag(etag))));
                }
                Poll::Ready(Some(Ok(ResponseEvent::Completed {
                    response_id,
                    token_usage,
                }))) => {
                    let mut emitted_any = false;

                    if !this.cumulative_reasoning.is_empty() {
                        let aggregated_reasoning = ResponseItem::Reasoning {
                            id: String::new(),
                            summary: Vec::new(),
                            content: Some(vec![ReasoningItemContent::ReasoningText {
                                text: std::mem::take(&mut this.cumulative_reasoning),
                            }]),
                            encrypted_content: None,
                        };
                        this.pending
                            .push_back(ResponseEvent::OutputItemDone(aggregated_reasoning));
                        emitted_any = true;
                    }

                    if !this.cumulative.is_empty() {
                        let aggregated_message = ResponseItem::Message {
                            id: None,
                            role: "assistant".to_string(),
                            content: vec![ContentItem::OutputText {
                                text: std::mem::take(&mut this.cumulative),
                            }],
                            end_turn: None,
                            phase: None,
                        };
                        this.pending
                            .push_back(ResponseEvent::OutputItemDone(aggregated_message));
                        emitted_any = true;
                    }

                    if emitted_any {
                        this.pending.push_back(ResponseEvent::Completed {
                            response_id: response_id.clone(),
                            token_usage: token_usage.clone(),
                        });
                        if let Some(ev) = this.pending.pop_front() {
                            return Poll::Ready(Some(Ok(ev)));
                        }
                    }

                    return Poll::Ready(Some(Ok(ResponseEvent::Completed {
                        response_id,
                        token_usage,
                    })));
                }
                Poll::Ready(Some(Ok(ResponseEvent::Created))) => {
                    continue;
                }
                Poll::Ready(Some(Ok(ResponseEvent::OutputTextDelta(delta)))) => {
                    this.cumulative.push_str(&delta);
                    if matches!(this.mode, AggregateMode::Streaming) {
                        return Poll::Ready(Some(Ok(ResponseEvent::OutputTextDelta(delta))));
                    } else {
                        continue;
                    }
                }
                Poll::Ready(Some(Ok(ResponseEvent::ReasoningContentDelta {
                    delta,
                    content_index,
                }))) => {
                    this.cumulative_reasoning.push_str(&delta);
                    if matches!(this.mode, AggregateMode::Streaming) {
                        return Poll::Ready(Some(Ok(ResponseEvent::ReasoningContentDelta {
                            delta,
                            content_index,
                        })));
                    } else {
                        continue;
                    }
                }
                Poll::Ready(Some(Ok(ResponseEvent::ReasoningSummaryDelta { .. }))) => continue,
                Poll::Ready(Some(Ok(ResponseEvent::ReasoningSummaryPartAdded { .. }))) => {
                    continue;
                }
                Poll::Ready(Some(Ok(ResponseEvent::OutputItemAdded(item)))) => {
                    return Poll::Ready(Some(Ok(ResponseEvent::OutputItemAdded(item))));
                }
            }
        }
    }
}

pub trait AggregateStreamExt {
    fn aggregate(self) -> AggregatedStream;

    fn streaming_mode(self) -> ResponseStream;
}

impl AggregateStreamExt for ResponseStream {
    fn aggregate(self) -> AggregatedStream {
        AggregatedStream::new(self, AggregateMode::AggregatedOnly)
    }

    fn streaming_mode(self) -> ResponseStream {
        self
    }
}

impl AggregatedStream {
    fn new(inner: ResponseStream, mode: AggregateMode) -> Self {
        AggregatedStream {
            inner,
            cumulative: String::new(),
            cumulative_reasoning: String::new(),
            pending: VecDeque::new(),
            mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ResponseStream;
    use codex_protocol::models::ContentItem;
    use futures::StreamExt;
    use tokio::sync::mpsc;

    fn make_stream(events: Vec<ResponseEvent>) -> ResponseStream {
        let (tx, rx) = mpsc::channel(64);
        for ev in events {
            tx.try_send(Ok(ev)).unwrap();
        }
        ResponseStream { rx_event: rx }
    }

    async fn collect(stream: impl Stream<Item = Result<ResponseEvent, ApiError>>) -> Vec<ResponseEvent> {
        futures::pin_mut!(stream);
        let mut out = Vec::new();
        while let Some(Ok(ev)) = stream.next().await {
            out.push(ev);
        }
        out
    }

    /// Regression: when the model emits text followed by tool calls, AggregatedOnly mode
    /// was buffering the AssistantMessage until Completed while passing FunctionCall items
    /// through immediately.  This reversed the history order to [FunctionCall, AssistantMessage],
    /// causing the request serialiser to insert an extra assistant message between the
    /// tool_calls message and the tool results — Minimax error 2013.
    #[tokio::test]
    async fn aggregated_stream_emits_assistant_text_before_function_calls() {
        let assistant_msg = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText { text: "let me check".to_string() }],
            end_turn: None,
            phase: None,
        };
        let fn_call = ResponseItem::FunctionCall {
            id: None,
            name: "search".to_string(),
            arguments: "{}".to_string(),
            call_id: "call-1".to_string(),
        };

        let inner = make_stream(vec![
            ResponseEvent::OutputItemDone(assistant_msg),
            ResponseEvent::OutputItemDone(fn_call),
            ResponseEvent::Completed { response_id: String::new(), token_usage: None },
        ]);

        let events = collect(inner.aggregate()).await;

        // AssistantMessage must appear before FunctionCall in the emitted sequence.
        let positions: Vec<_> = events
            .iter()
            .enumerate()
            .filter_map(|(i, ev)| match ev {
                ResponseEvent::OutputItemDone(ResponseItem::Message { role, .. }) if role == "assistant" => Some(("assistant", i)),
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. }) => Some(("fn_call", i)),
                _ => None,
            })
            .collect();

        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].0, "assistant", "assistant message must come first");
        assert_eq!(positions[1].0, "fn_call", "function call must come second");
    }
}
