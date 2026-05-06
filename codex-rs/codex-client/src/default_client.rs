use http::Error as HttpError;
use opentelemetry::global;
use opentelemetry::propagation::Injector;
use reqwest::IntoUrl;
use reqwest::Method;
use reqwest::Response;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use serde::Serialize;
use std::fmt::Display;
use std::sync::LazyLock;
use std::time::Duration;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

static SESSION_ID: LazyLock<String> = LazyLock::new(|| format!("ses_{}", Uuid::new_v4()));
static PROJECT_ID: LazyLock<String> = LazyLock::new(|| Uuid::new_v4().to_string());

#[derive(Clone, Debug)]
pub struct CodexHttpClient {
    inner: reqwest::Client,
    api_key: Option<String>,
    session_id: String,
    project_id: String,
}

impl CodexHttpClient {
    pub fn new(inner: reqwest::Client) -> Self {
        Self {
            inner,
            api_key: None,
            session_id: SESSION_ID.clone(),
            project_id: PROJECT_ID.clone(),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn get<U>(&self, url: U) -> CodexRequestBuilder
    where
        U: IntoUrl,
    {
        self.request(Method::GET, url)
    }

    pub fn post<U>(&self, url: U) -> CodexRequestBuilder
    where
        U: IntoUrl,
    {
        self.request(Method::POST, url)
    }

    pub fn request<U>(&self, method: Method, url: U) -> CodexRequestBuilder
    where
        U: IntoUrl,
    {
        let url_str = url.as_str().to_string();
        CodexRequestBuilder::new(
            self.inner.request(method.clone(), url),
            method,
            url_str,
            self.api_key.clone(),
            self.session_id.clone(),
            self.project_id.clone(),
        )
    }
}

#[must_use = "requests are not sent unless `send` is awaited"]
#[derive(Debug)]
pub struct CodexRequestBuilder {
    builder: reqwest::RequestBuilder,
    method: Method,
    url: String,
    api_key: Option<String>,
    session_id: String,
    project_id: String,
}

impl CodexRequestBuilder {
    fn new(
        builder: reqwest::RequestBuilder,
        method: Method,
        url: String,
        api_key: Option<String>,
        session_id: String,
        project_id: String,
    ) -> Self {
        Self {
            builder,
            method,
            url,
            api_key,
            session_id,
            project_id,
        }
    }

    fn map(self, f: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Self {
        Self {
            builder: f(self.builder),
            method: self.method,
            url: self.url,
            api_key: self.api_key,
            session_id: self.session_id,
            project_id: self.project_id,
        }
    }

    pub fn headers(self, headers: HeaderMap) -> Self {
        self.map(|builder| builder.headers(headers))
    }

    pub fn header<K, V>(self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.map(|builder| builder.header(key, value))
    }

    pub fn bearer_auth<T>(self, token: T) -> Self
    where
        T: Display,
    {
        self.map(|builder| builder.bearer_auth(token))
    }

    pub fn timeout(self, timeout: Duration) -> Self {
        self.map(|builder| builder.timeout(timeout))
    }

    pub fn json<T>(self, value: &T) -> Self
    where
        T: ?Sized + Serialize,
    {
        self.map(|builder| builder.json(value))
    }

    pub fn body<B>(self, body: B) -> Self
    where
        B: Into<reqwest::Body>,
    {
        self.map(|builder| builder.body(body))
    }

    pub async fn send(mut self) -> Result<Response, reqwest::Error> {
        let trace_hdrs = trace_headers();
        for (name, value) in trace_hdrs.iter() {
            self.builder = self.builder.header(name.clone(), value.clone());
        }

        let opencode_hdrs = opencode_headers(&self.session_id, &self.project_id);
        for (name, value) in opencode_hdrs.iter() {
            self.builder = self.builder.header(name.clone(), value.clone());
        }

        // Add Authorization header if API key is set
        if let Some(api_key) = &self.api_key {
            if let Ok(auth_value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
                self.builder = self.builder.header(http::header::AUTHORIZATION, auth_value);
            }
        }

        match self.builder.send().await {
            Ok(response) => {
                tracing::debug!(
                    method = %self.method,
                    url = %self.url,
                    status = %response.status(),
                    headers = ?response.headers(),
                    version = ?response.version(),
                    "Request completed"
                );

                Ok(response)
            }
            Err(error) => {
                let status = error.status();
                tracing::debug!(
                    method = %self.method,
                    url = %self.url,
                    status = status.map(|s| s.as_u16()),
                    error = %error,
                    "Request failed"
                );
                Err(error)
            }
        }
    }
}

struct HeaderMapInjector<'a>(&'a mut HeaderMap);

impl<'a> Injector for HeaderMapInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, val);
        }
    }
}

fn trace_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    global::get_text_map_propagator(|prop| {
        prop.inject_context(
            &Span::current().context(),
            &mut HeaderMapInjector(&mut headers),
        );
    });
    headers
}

fn opencode_headers(session_id: &str, project_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();

    if let Ok(client_id) = HeaderValue::from_str("cli") {
        headers.insert("x-opencode-client", client_id);
    }
    if let Ok(session_id) = HeaderValue::from_str(session_id) {
        headers.insert("x-opencode-session", session_id);
    }
    if let Ok(project_id) = HeaderValue::from_str(project_id) {
        headers.insert("x-opencode-project", project_id);
    }
    if let Ok(request_id) = HeaderValue::from_str(&Uuid::new_v4().to_string()) {
        headers.insert("x-opencode-request", request_id);
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::propagation::Extractor;
    use opentelemetry::propagation::TextMapPropagator;
    use opentelemetry::trace::TraceContextExt;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing::trace_span;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    #[test]
    fn inject_trace_headers_uses_current_span_context() {
        global::set_text_map_propagator(TraceContextPropagator::new());

        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("test-tracer");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        let _guard = subscriber.set_default();

        let span = trace_span!("client_request");
        let _entered = span.enter();
        let span_context = span.context().span().span_context().clone();

        let headers = trace_headers();

        let extractor = HeaderMapExtractor(&headers);
        let extracted = TraceContextPropagator::new().extract(&extractor);
        let extracted_span = extracted.span();
        let extracted_context = extracted_span.span_context();

        assert!(extracted_context.is_valid());
        assert_eq!(extracted_context.trace_id(), span_context.trace_id());
        assert_eq!(extracted_context.span_id(), span_context.span_id());
    }

    struct HeaderMapExtractor<'a>(&'a HeaderMap);

    impl<'a> Extractor for HeaderMapExtractor<'a> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.get(key).and_then(|value| value.to_str().ok())
        }

        fn keys(&self) -> Vec<&str> {
            self.0.keys().map(HeaderName::as_str).collect()
        }
    }
}
