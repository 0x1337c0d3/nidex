use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use crate::codex_message_processor::CodexMessageProcessor;
use crate::codex_message_processor::CodexMessageProcessorArgs;
use crate::config_api::ConfigApi;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use crate::outgoing_message::OutgoingMessageSender;
use codex_app_server_protocol::AgentCapabilities;
use codex_app_server_protocol::AgentInfo;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigValueWriteParams;
use codex_app_server_protocol::ConfigWarningNotification;
use codex_app_server_protocol::ExperimentalApi;
use codex_app_server_protocol::InitializeResponse;
use codex_app_server_protocol::PromptCapabilities;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::experimental_required_message;
use codex_core::AuthManager;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_core::config_loader::CloudRequirementsLoader;
use codex_core::config_loader::LoaderOverrides;
use codex_core::default_client::SetOriginatorError;
use codex_core::default_client::USER_AGENT_SUFFIX;
use codex_core::default_client::get_codex_user_agent;
use codex_core::default_client::set_default_client_residency_requirement;
use codex_core::default_client::set_default_originator;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use tokio::sync::broadcast;
use tokio::time::Duration;
use toml::Value as TomlValue;

#[allow(dead_code)]
const EXTERNAL_AUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct MessageProcessor {
    #[allow(dead_code)]
    outgoing: Arc<OutgoingMessageSender>,
    codex_message_processor: CodexMessageProcessor,
    config_api: ConfigApi,
    config: Arc<Config>,
    initialized: bool,
    experimental_api_enabled: Arc<AtomicBool>,
    /// Gap 6: whether the connected client supports terminal delegation.
    client_terminal_capable: Arc<AtomicBool>,
    config_warnings: Vec<ConfigWarningNotification>,
}

pub(crate) struct MessageProcessorArgs {
    pub(crate) outgoing: OutgoingMessageSender,
    pub(crate) codex_linux_sandbox_exe: Option<PathBuf>,
    pub(crate) config: Arc<Config>,
    pub(crate) cli_overrides: Vec<(String, TomlValue)>,
    pub(crate) loader_overrides: LoaderOverrides,
    pub(crate) cloud_requirements: CloudRequirementsLoader,
    pub(crate) config_warnings: Vec<ConfigWarningNotification>,
}

impl MessageProcessor {
    /// Create a new `MessageProcessor`, retaining a handle to the outgoing
    /// `Sender` so handlers can enqueue messages to be written to stdout.
    pub(crate) fn new(args: MessageProcessorArgs) -> Self {
        let MessageProcessorArgs {
            outgoing,
            codex_linux_sandbox_exe,
            config,
            cli_overrides,
            loader_overrides,
            cloud_requirements,
            config_warnings,
        } = args;
        let outgoing = Arc::new(outgoing);
        let experimental_api_enabled = Arc::new(AtomicBool::new(false));
        let auth_manager = AuthManager::shared(
            config.codex_home.clone(),
            false,
            config.cli_auth_credentials_store_mode,
        );
        let thread_manager = Arc::new(ThreadManager::with_model_provider(
            config.codex_home.clone(),
            auth_manager.clone(),
            SessionSource::VSCode,
            config.model_provider.clone(),
        ));
        let codex_message_processor = CodexMessageProcessor::new(CodexMessageProcessorArgs {
            auth_manager,
            thread_manager,
            outgoing: outgoing.clone(),
            codex_linux_sandbox_exe,
            config: Arc::clone(&config),
            cli_overrides: cli_overrides.clone(),
            cloud_requirements: cloud_requirements.clone(),
        });
        let config_api = ConfigApi::new(
            config.codex_home.clone(),
            cli_overrides,
            loader_overrides,
        );

        Self {
            outgoing,
            codex_message_processor,
            config_api,
            config,
            initialized: false,
            experimental_api_enabled,
            client_terminal_capable: Arc::new(AtomicBool::new(false)),
            config_warnings,
        }
    }

    pub(crate) async fn process_request(&mut self, request: JSONRPCRequest) {
        let request_id = request.id.clone();
        let request_json = match serde_json::to_value(&request) {
            Ok(request_json) => request_json,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("Invalid request: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let codex_request = match serde_json::from_value::<ClientRequest>(request_json) {
            Ok(codex_request) => codex_request,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("Invalid request: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        match codex_request {
            // Handle Initialize internally so CodexMessageProcessor does not have to concern
            // itself with the `initialized` bool.
            ClientRequest::Initialize { request_id, params } => {
                if self.initialized {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: "Already initialized".to_string(),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                } else {
                    let experimental_api_enabled = params
                        .client_capabilities
                        .as_ref()
                        .is_some_and(|cap| cap.experimental_api);
                    self.experimental_api_enabled
                        .store(experimental_api_enabled, Ordering::Relaxed);
                    // Gap 6: record whether the client can handle terminal delegation.
                    let client_terminal_capable = params
                        .client_capabilities
                        .as_ref()
                        .is_some_and(|cap| cap.terminal);
                    self.client_terminal_capable
                        .store(client_terminal_capable, Ordering::Relaxed);
                    let (name, version) = params
                        .client_info
                        .map(|ci| (ci.name, ci.version))
                        .unwrap_or_default();
                    if let Err(error) = set_default_originator(name.clone()) {
                        match error {
                            SetOriginatorError::InvalidHeaderValue => {
                                let error = JSONRPCErrorError {
                                    code: INVALID_REQUEST_ERROR_CODE,
                                    message: format!(
                                        "Invalid clientInfo.name: '{name}'. Must be a valid HTTP header value."
                                    ),
                                    data: None,
                                };
                                self.outgoing.send_error(request_id, error).await;
                                return;
                            }
                            SetOriginatorError::AlreadyInitialized => {
                                // No-op. This is expected to happen if the originator is already set via env var.
                                // TODO(owen): Once we remove support for CODEX_INTERNAL_ORIGINATOR_OVERRIDE,
                                // this will be an unexpected state and we can return a JSON-RPC error indicating
                                // internal server error.
                            }
                        }
                    }
                    set_default_client_residency_requirement(self.config.enforce_residency.value());
                    let user_agent_suffix = format!("{name}; {version}");
                    if let Ok(mut suffix) = USER_AGENT_SUFFIX.lock() {
                        *suffix = Some(user_agent_suffix);
                    }

                    let user_agent = get_codex_user_agent();
                    let echoed_version = params.protocol_version.unwrap_or_else(|| {
                        serde_json::Value::String("2025-05-12".to_string())
                    });
                    let response = InitializeResponse {
                        user_agent,
                        protocol_version: echoed_version,
                        agent_capabilities: Some(AgentCapabilities {
                            // Gap 5: all session lifecycle methods implemented.
                            load_session: true,
                            close_session: true,
                            list_sessions: true,
                            resume_session: true,
                            authenticate: true,
                            prompt_capabilities: Some(PromptCapabilities {
                                image: true,
                                audio: false,
                                embedded_context: false,
                            }),
                            mcp_capabilities: None,
                        }),
                        agent_info: Some(AgentInfo {
                            name: "codex".to_string(),
                            title: "Codex".to_string(),
                            version: env!("CARGO_PKG_VERSION").to_string(),
                        }),
                        auth_methods: Some(vec![]),
                    };
                    self.outgoing.send_response(request_id, response).await;

                    self.initialized = true;
                    if !self.config_warnings.is_empty() {
                        for notification in self.config_warnings.drain(..) {
                            self.outgoing
                                .send_server_notification(ServerNotification::ConfigWarning(
                                    notification,
                                ))
                                .await;
                        }
                    }

                    return;
                }
            }
            _ => {
                if !self.initialized {
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: "Not initialized".to_string(),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }
            }
        }

        if let Some(reason) = codex_request.experimental_reason()
            && !self.experimental_api_enabled.load(Ordering::Relaxed)
        {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: experimental_required_message(reason),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        match codex_request {
            ClientRequest::ConfigRead { request_id, params } => {
                self.handle_config_read(request_id, params).await;
            }
            ClientRequest::ConfigValueWrite { request_id, params } => {
                self.handle_config_value_write(request_id, params).await;
            }
            ClientRequest::ConfigBatchWrite { request_id, params } => {
                self.handle_config_batch_write(request_id, params).await;
            }
            ClientRequest::ConfigRequirementsRead {
                request_id,
                params: _,
            } => {
                self.handle_config_requirements_read(request_id).await;
            }
            other => {
                self.codex_message_processor.process_request(other).await;
            }
        }
    }

    pub(crate) async fn process_notification(&mut self, notification: JSONRPCNotification) {
        use codex_app_server_protocol::ClientNotification;
        match serde_json::from_value::<ClientNotification>(
            serde_json::to_value(&notification).unwrap_or_default(),
        ) {
            Ok(ClientNotification::SessionCancel(params)) => {
                self.codex_message_processor
                    .session_cancel(params.session_id)
                    .await;
            }
            Ok(ClientNotification::Initialized) => {}
            Err(_) => {
                tracing::info!("<- notification: {:?}", notification);
            }
        }
    }

    pub(crate) fn thread_created_receiver(&self) -> broadcast::Receiver<ThreadId> {
        self.codex_message_processor.thread_created_receiver()
    }

    pub(crate) async fn try_attach_thread_listener(&mut self, thread_id: ThreadId) {
        if !self.initialized {
            return;
        }
        self.codex_message_processor
            .try_attach_thread_listener(thread_id)
            .await;
    }

    /// Handle a standalone JSON-RPC response originating from the peer.
    pub(crate) async fn process_response(&mut self, response: JSONRPCResponse) {
        tracing::info!("<- response: {:?}", response);
        let JSONRPCResponse { id, result, .. } = response;
        self.outgoing.notify_client_response(id, result).await
    }

    /// Handle an error object received from the peer.
    pub(crate) async fn process_error(&mut self, err: JSONRPCError) {
        if let Some(id) = err.id {
            tracing::error!("<- error for request {:?}: {:?}", id, err.error);
            self.outgoing.notify_client_error(id, err.error).await;
        } else {
            // Peer sent an error with no id — this is a "Method not found" response
            // to one of our server notifications. Expected when the client doesn't
            // implement all notification methods.
            tracing::debug!("<- peer rejected notification: {:?}", err.error.data);
        }
    }

    async fn handle_config_read(&self, request_id: RequestId, params: ConfigReadParams) {
        match self.config_api.read(params).await {
            Ok(response) => self.outgoing.send_response(request_id, response).await,
            Err(error) => self.outgoing.send_error(request_id, error).await,
        }
    }

    async fn handle_config_value_write(
        &self,
        request_id: RequestId,
        params: ConfigValueWriteParams,
    ) {
        match self.config_api.write_value(params).await {
            Ok(response) => self.outgoing.send_response(request_id, response).await,
            Err(error) => self.outgoing.send_error(request_id, error).await,
        }
    }

    async fn handle_config_batch_write(
        &self,
        request_id: RequestId,
        params: ConfigBatchWriteParams,
    ) {
        match self.config_api.batch_write(params).await {
            Ok(response) => self.outgoing.send_response(request_id, response).await,
            Err(error) => self.outgoing.send_error(request_id, error).await,
        }
    }

    async fn handle_config_requirements_read(&self, request_id: RequestId) {
        match self.config_api.config_requirements_read().await {
            Ok(response) => self.outgoing.send_response(request_id, response).await,
            Err(error) => self.outgoing.send_error(request_id, error).await,
        }
    }
}
