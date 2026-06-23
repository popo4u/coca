pub mod terminal;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use coca_app::{AiSettingsUpdate, AppOptions, AppService};
use coca_core::catalog::SessionCatalog;
use coca_core::launch::{LaunchMode, LaunchOption, LaunchOptionKind};
use coca_core::model::{ProviderFilter, ProviderKind, Session, SessionOrigin};
use coca_core::settings::{save_settings, Settings};
use coca_protocol::{
    methods, AiSettingsUpdateParams, DaemonPingResult, JsonRpcRequest, JsonRpcResponse,
    LaunchModeWire, LaunchOptionKindWire, LaunchOptionWire, LaunchOptionsParams,
    LaunchPrepareParams, PreparedLaunch, RpcId, SessionGetParams, SessionRef,
    SettingsSummaryParams, SettingsUpdateParams, ShareUrlParams, TerminalGetParams, TerminalId,
    TerminalListResult, TerminalModeWire, TerminalOpen, TerminalSessionSummary,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

use crate::terminal::{
    DaemonTerminalBackend, TerminalBackend, TerminalLaunchTarget, TerminalManager,
    TerminalRuntimeError,
};

#[cfg(unix)]
pub fn serve_rpc(socket_path: &std::path::Path, options: RpcDaemonOptions) -> Result<()> {
    let mut daemon = RpcDaemon::with_state(DaemonState::new(options));
    coca_ipc::unix::serve(socket_path, move |request| daemon.handle_request(request))
}

#[cfg(not(unix))]
pub fn serve_rpc(_socket_path: &std::path::Path, _options: RpcDaemonOptions) -> Result<()> {
    anyhow::bail!("local daemon IPC is not implemented on this platform yet")
}

#[cfg(unix)]
pub fn serve_daemon(
    rpc_socket_path: &std::path::Path,
    terminal_socket_path: &std::path::Path,
    options: RpcDaemonOptions,
) -> Result<()> {
    let state = DaemonState::new(options);
    let terminal_state = state.clone();
    let terminal_socket_path = terminal_socket_path.to_path_buf();
    std::thread::spawn(move || {
        if let Err(err) = serve_terminal_stream(&terminal_socket_path, terminal_state) {
            eprintln!("coca daemon terminal stream failed: {err:#}");
        }
    });

    let mut daemon = RpcDaemon::with_state(state);
    coca_ipc::unix::serve(rpc_socket_path, move |request| {
        daemon.handle_request(request)
    })
}

#[cfg(not(unix))]
pub fn serve_daemon(
    _rpc_socket_path: &std::path::Path,
    _terminal_socket_path: &std::path::Path,
    _options: RpcDaemonOptions,
) -> Result<()> {
    anyhow::bail!("local daemon IPC is not implemented on this platform yet")
}

#[cfg(unix)]
pub fn serve_terminal_stream<B>(socket_path: &std::path::Path, state: DaemonState<B>) -> Result<()>
where
    B: TerminalBackend + Send + 'static,
{
    use std::sync::atomic::{AtomicU64, Ordering};

    let next_client_id = AtomicU64::new(1);
    let manager = state.terminal_manager();
    coca_ipc::unix::serve_stream(socket_path, move |stream| {
        let client_id = format!("client-{}", next_client_id.fetch_add(1, Ordering::Relaxed));
        let resolver_state = state.clone();
        terminal::handle_unix_stream(manager.clone(), client_id, stream, move |request| {
            resolver_state.resolve_terminal_launch(request)
        })
    })
}

#[cfg(not(unix))]
pub fn serve_terminal_stream<B>(
    _socket_path: &std::path::Path,
    _state: DaemonState<B>,
) -> Result<()>
where
    B: TerminalBackend + Send + 'static,
{
    anyhow::bail!("local daemon terminal IPC is not implemented on this platform yet")
}

#[derive(Clone, Debug)]
pub struct RpcDaemonOptions {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
}

#[derive(Debug)]
pub struct DaemonState<B = DaemonTerminalBackend> {
    options: RpcDaemonOptions,
    terminal_manager: Arc<Mutex<TerminalManager<B>>>,
}

impl<B> Clone for DaemonState<B> {
    fn clone(&self) -> Self {
        Self {
            options: self.options.clone(),
            terminal_manager: self.terminal_manager.clone(),
        }
    }
}

impl DaemonState<DaemonTerminalBackend> {
    pub fn new(options: RpcDaemonOptions) -> Self {
        Self::with_backend(options, DaemonTerminalBackend::new())
    }
}

impl<B> DaemonState<B>
where
    B: TerminalBackend,
{
    pub fn with_backend(options: RpcDaemonOptions, backend: B) -> Self {
        Self {
            options,
            terminal_manager: Arc::new(Mutex::new(TerminalManager::with_backend(backend))),
        }
    }

    pub fn terminal_manager(&self) -> Arc<Mutex<TerminalManager<B>>> {
        self.terminal_manager.clone()
    }

    fn resolve_terminal_launch(
        &self,
        request: &TerminalOpen,
    ) -> Result<TerminalLaunchTarget, TerminalRuntimeError> {
        let app = self.app();
        let session = app
            .session(&app_session_ref(&request.session))
            .map_err(|err| TerminalRuntimeError::backend(format!("{err:#}")))?
            .ok_or_else(|| TerminalRuntimeError::backend("session not found"))?;
        if let SessionOrigin::Remote(name) = &session.origin {
            let remote = self
                .options
                .settings
                .remotes
                .iter()
                .find(|remote| remote.enabled && &remote.name == name)
                .ok_or_else(|| {
                    TerminalRuntimeError::backend(format!(
                        "remote {name} is not configured for terminal access"
                    ))
                })?;
            let terminal_token = remote
                .terminal_token
                .as_deref()
                .filter(|token| !token.trim().is_empty())
                .ok_or_else(|| {
                    TerminalRuntimeError::backend(format!(
                        "remote {name} terminal token is not configured"
                    ))
                })?;
            let mut remote_open = request.clone();
            remote_open.session.origin = "local".to_string();
            return Ok(TerminalLaunchTarget::remote(
                remote.base_url.clone(),
                remote.token.clone(),
                terminal_token.to_string(),
                remote_open,
            ));
        }
        let target = app
            .prepare_terminal_launch(&session, terminal_mode(request.mode))
            .map_err(|err| TerminalRuntimeError::backend(format!("{err:#}")))?;
        Ok(TerminalLaunchTarget::local(
            target.program,
            target.args,
            target.cwd,
        ))
    }

    fn app(&self) -> AppService {
        AppService::new(AppOptions {
            settings: self.options.settings.clone(),
            settings_path: self.options.settings_path.clone(),
            codex_home: self.options.codex_home.clone(),
            claude_home: self.options.claude_home.clone(),
            provider_filter: self.options.provider_filter,
            database_path: None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct RpcDaemon<B = DaemonTerminalBackend> {
    state: DaemonState<B>,
}

impl RpcDaemon<DaemonTerminalBackend> {
    pub fn new(options: RpcDaemonOptions) -> Self {
        Self::with_state(DaemonState::new(options))
    }
}

impl<B> RpcDaemon<B>
where
    B: TerminalBackend,
{
    pub fn with_state(state: DaemonState<B>) -> Self {
        Self { state }
    }

    pub fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone();
        match self.dispatch(request) {
            Ok(value) => JsonRpcResponse::success(id, value),
            Err(error) => JsonRpcResponse::error(id, error.code, error.message),
        }
    }

    fn dispatch(&mut self, request: JsonRpcRequest) -> Result<Value, RpcDispatchError> {
        match request.method.as_str() {
            methods::DAEMON_PING => serde_json::to_value(DaemonPingResult::default()),
            methods::SESSIONS_LIST => serde_json::to_value(
                self.app()
                    .session_catalog()
                    .map_err(RpcDispatchError::from_anyhow)?,
            ),
            methods::SESSIONS_SUMMARIES => {
                let response = self
                    .app()
                    .web_sessions()
                    .map_err(RpcDispatchError::from_anyhow)?;
                serde_json::to_value(response)
            }
            methods::SESSIONS_GET => {
                let params: SessionGetParams = parse_params(request.params)?;
                let session = self
                    .find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                serde_json::to_value(session)
            }
            methods::SESSIONS_DETAIL => {
                let params: SessionGetParams = parse_params(request.params)?;
                let detail = self
                    .app()
                    .web_session_detail(&app_session_ref(&params.session))
                    .map_err(RpcDispatchError::from_anyhow)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                serde_json::to_value(detail)
            }
            methods::SETTINGS_GET => serde_json::to_value(&self.state.options.settings),
            methods::SETTINGS_SUMMARY => {
                let params: SettingsSummaryParams = parse_params(request.params)?;
                let summary = self
                    .app()
                    .config_summary(&params.gateway_bind)
                    .map_err(RpcDispatchError::from_anyhow)?
                    .with_terminal_runtime(true, params.terminal_socket_available);
                serde_json::to_value(summary)
            }
            methods::SETTINGS_UPDATE => {
                let params: SettingsUpdateParams = parse_params(request.params)?;
                let mut settings: Settings = serde_json::from_value(params.settings)
                    .map_err(|err| RpcDispatchError::invalid_params(err.to_string()))?;
                settings.ensure_defaults();
                settings
                    .validate()
                    .map_err(|err| RpcDispatchError::invalid_params(format!("{err:#}")))?;
                if let Some(path) = &self.state.options.settings_path {
                    save_settings(path, &settings)
                        .map_err(|err| RpcDispatchError::internal(format!("{err:#}")))?;
                }
                self.state.options.settings = settings;
                serde_json::to_value(&self.state.options.settings)
            }
            methods::SETTINGS_AI_UPDATE => {
                let params: AiSettingsUpdateParams = parse_params(request.params)?;
                let mut app = self.app();
                let summary = app
                    .update_ai_settings(ai_settings_update(params))
                    .map_err(RpcDispatchError::from_anyhow)?;
                self.state.options.settings = app.settings();
                serde_json::to_value(summary)
            }
            methods::SHARE_URL => {
                let params: ShareUrlParams = parse_params(request.params)?;
                self.find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                let url = self
                    .app()
                    .share_session(&app_session_ref(&params.session))
                    .map_err(RpcDispatchError::from_anyhow)?
                    .url;
                Ok(json!({ "url": url }))
            }
            methods::LAUNCH_OPTIONS => {
                let params: LaunchOptionsParams = parse_params(request.params)?;
                let session = self
                    .find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                let mode = launch_mode(params.mode);
                let current_cwd = PathBuf::from(params.current_cwd);
                let options = self
                    .app()
                    .launch_options_with_defaults(&session, mode, &current_cwd)
                    .map_err(RpcDispatchError::from_anyhow)?;
                serde_json::to_value(
                    options
                        .into_iter()
                        .map(launch_option_wire)
                        .collect::<Vec<_>>(),
                )
            }
            methods::LAUNCH_PREPARE => {
                let params: LaunchPrepareParams = parse_params(request.params)?;
                let session = self
                    .find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                let current_cwd = PathBuf::from(params.current_cwd);
                let options = params
                    .options
                    .into_iter()
                    .map(launch_option)
                    .collect::<Vec<_>>();
                let target = self
                    .app()
                    .prepare_launch(&session, launch_mode(params.mode), &current_cwd, &options)
                    .map_err(RpcDispatchError::from_anyhow)?;
                serde_json::to_value(PreparedLaunch {
                    program: target.program,
                    args: target.args,
                    cwd: target.cwd.map(|cwd| cwd.to_string_lossy().to_string()),
                })
            }
            methods::TERMINAL_LIST => serde_json::to_value(
                self.state
                    .terminal_manager
                    .lock()
                    .expect("terminal manager mutex poisoned")
                    .list(),
            ),
            methods::TERMINAL_GET => {
                let params: TerminalGetParams = parse_params(request.params)?;
                let terminal = self
                    .state
                    .terminal_manager
                    .lock()
                    .expect("terminal manager mutex poisoned")
                    .get(&params.terminal_id)
                    .ok_or_else(|| RpcDispatchError::not_found("terminal not found"))?;
                serde_json::to_value(terminal)
            }
            _ => return Err(RpcDispatchError::method_not_found()),
        }
        .map_err(|err| RpcDispatchError::internal(err.to_string()))
    }

    fn app(&self) -> AppService {
        self.state.app()
    }

    fn sessions(&self) -> Result<Vec<Session>, RpcDispatchError> {
        let catalog = self
            .app()
            .session_catalog()
            .map_err(RpcDispatchError::from_anyhow)?;
        Ok(catalog.sessions)
    }

    fn find_session(&self, reference: &SessionRef) -> Result<Option<Session>, RpcDispatchError> {
        let provider = provider_kind(&reference.provider)
            .ok_or_else(|| RpcDispatchError::invalid_params("unknown provider"))?;
        let sessions = self.sessions()?;
        Ok(sessions.into_iter().find(|session| {
            session.provider == provider
                && session.id == reference.id
                && origin_matches(&session.origin, &reference.origin)
        }))
    }
}

#[derive(Clone, Debug)]
pub struct LocalRpcClient<B = DaemonTerminalBackend> {
    daemon: RpcDaemon<B>,
    next_id: i64,
}

impl LocalRpcClient<DaemonTerminalBackend> {
    pub fn new(options: RpcDaemonOptions) -> Self {
        Self::from_daemon(RpcDaemon::new(options))
    }
}

impl<B> LocalRpcClient<B>
where
    B: TerminalBackend,
{
    pub fn from_daemon(daemon: RpcDaemon<B>) -> Self {
        Self { daemon, next_id: 1 }
    }

    pub fn session_catalog(&mut self) -> Result<SessionCatalog> {
        self.call(methods::SESSIONS_LIST, None)
    }

    pub fn session_summaries(&mut self) -> Result<coca_app::SessionsResponse> {
        self.call(methods::SESSIONS_SUMMARIES, None)
    }

    pub fn session_detail(&mut self, session: SessionRef) -> Result<coca_app::SessionDetail> {
        self.call(
            methods::SESSIONS_DETAIL,
            to_params(SessionGetParams { session })?,
        )
    }

    pub fn settings(&mut self) -> Result<Settings> {
        self.call(methods::SETTINGS_GET, None)
    }

    pub fn settings_summary(
        &mut self,
        params: SettingsSummaryParams,
    ) -> Result<coca_app::ConfigSummary> {
        self.call(methods::SETTINGS_SUMMARY, to_params(params)?)
    }

    pub fn settings_update(&mut self, settings: Settings) -> Result<Settings> {
        let settings = serde_json::to_value(settings).context("failed to encode settings")?;
        self.call(
            methods::SETTINGS_UPDATE,
            to_params(SettingsUpdateParams { settings })?,
        )
    }

    pub fn update_ai_settings(
        &mut self,
        params: AiSettingsUpdateParams,
    ) -> Result<coca_app::AiSummary> {
        self.call(methods::SETTINGS_AI_UPDATE, to_params(params)?)
    }

    pub fn share_url(&mut self, session: SessionRef) -> Result<String> {
        let result: Value =
            self.call(methods::SHARE_URL, to_params(ShareUrlParams { session })?)?;
        result
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("share.url response did not include url")
    }

    pub fn launch_options(&mut self, params: LaunchOptionsParams) -> Result<Vec<LaunchOptionWire>> {
        self.call(methods::LAUNCH_OPTIONS, to_params(params)?)
    }

    pub fn launch_prepare(&mut self, params: LaunchPrepareParams) -> Result<PreparedLaunch> {
        self.call(methods::LAUNCH_PREPARE, to_params(params)?)
    }

    pub fn terminal_list(&mut self) -> Result<TerminalListResult> {
        self.call(methods::TERMINAL_LIST, None)
    }

    pub fn terminal_get(&mut self, terminal_id: TerminalId) -> Result<TerminalSessionSummary> {
        self.call(
            methods::TERMINAL_GET,
            to_params(TerminalGetParams { terminal_id })?,
        )
    }

    fn call<T>(&mut self, method: &str, params: Option<Value>) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let id = self.next_request_id();
        let request = JsonRpcRequest::new(id.clone(), method, params);
        let response = self.daemon.handle_request(request);
        decode_response(response, id)
    }

    fn next_request_id(&mut self) -> RpcId {
        let id = self.next_id;
        self.next_id += 1;
        RpcId::Number(id)
    }
}

fn to_params<T>(params: T) -> Result<Option<Value>>
where
    T: Serialize,
{
    serde_json::to_value(params)
        .map(Some)
        .context("failed to encode RPC params")
}

fn decode_response<T>(response: JsonRpcResponse, expected_id: RpcId) -> Result<T>
where
    T: DeserializeOwned,
{
    if response.id != expected_id {
        anyhow::bail!(
            "RPC response id mismatch: expected {:?}, got {:?}",
            expected_id,
            response.id
        );
    }

    if let Some(error) = response.error {
        anyhow::bail!("RPC error {}: {}", error.code, error.message);
    }

    let result = response
        .result
        .context("RPC response did not include result")?;
    serde_json::from_value(result).context("failed to decode RPC response")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RpcDispatchError {
    code: i64,
    message: String,
}

impl RpcDispatchError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: "method not found".to_string(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: 404,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }

    fn from_anyhow(error: anyhow::Error) -> Self {
        Self {
            code: 400,
            message: format!("{error:#}"),
        }
    }
}

fn parse_params<T>(params: Option<Value>) -> Result<T, RpcDispatchError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|err| RpcDispatchError::invalid_params(err.to_string()))
}

fn provider_kind(provider: &str) -> Option<ProviderKind> {
    match provider {
        "codex" => Some(ProviderKind::Codex),
        "claude" => Some(ProviderKind::Claude),
        _ => None,
    }
}

fn origin_matches(origin: &SessionOrigin, reference: &str) -> bool {
    match origin {
        SessionOrigin::Local => reference == "local",
        SessionOrigin::Remote(name) => reference == name,
    }
}

fn app_session_ref(reference: &SessionRef) -> coca_app::SessionRef {
    coca_app::SessionRef {
        origin: reference.origin.clone(),
        provider: reference.provider.clone(),
        id: reference.id.clone(),
    }
}

fn ai_settings_update(params: AiSettingsUpdateParams) -> AiSettingsUpdate {
    AiSettingsUpdate {
        base_url: params.base_url,
        model: params.model,
        enabled: params.enabled,
        provider: params.provider,
        api_key_env: params.api_key_env,
        api_key: params.api_key,
        clear_api_key: params.clear_api_key,
    }
}

fn launch_mode(mode: LaunchModeWire) -> LaunchMode {
    match mode {
        LaunchModeWire::Resume => LaunchMode::Resume,
        LaunchModeWire::Fork => LaunchMode::Fork,
    }
}

fn terminal_mode(mode: TerminalModeWire) -> LaunchMode {
    match mode {
        TerminalModeWire::Resume => LaunchMode::Resume,
        TerminalModeWire::Fork => LaunchMode::Fork,
    }
}

fn launch_option_kind(kind: LaunchOptionKindWire) -> LaunchOptionKind {
    match kind {
        LaunchOptionKindWire::UseCurrentDir => LaunchOptionKind::UseCurrentDir,
        LaunchOptionKindWire::Yolo => LaunchOptionKind::Yolo,
    }
}

fn launch_option_kind_wire(kind: LaunchOptionKind) -> LaunchOptionKindWire {
    match kind {
        LaunchOptionKind::UseCurrentDir => LaunchOptionKindWire::UseCurrentDir,
        LaunchOptionKind::Yolo => LaunchOptionKindWire::Yolo,
    }
}

fn launch_option_wire(option: LaunchOption) -> LaunchOptionWire {
    LaunchOptionWire {
        kind: launch_option_kind_wire(option.kind),
        label: option.label,
        enabled: option.enabled,
    }
}

fn launch_option(option: LaunchOptionWire) -> LaunchOption {
    LaunchOption {
        kind: launch_option_kind(option.kind),
        label: option.label,
        enabled: option.enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::FakeTerminalBackend;
    use coca_core::settings::ConfiguredRemote;
    use coca_protocol::{
        methods, AiSettingsUpdateParams, SettingsSummaryParams, TerminalModeWire, TerminalOpen,
        TerminalSize, TerminalStateWire, PROTOCOL_VERSION,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn ping_returns_protocol_version() {
        let mut daemon = RpcDaemon::new(test_options());

        let response = daemon.handle_request(JsonRpcRequest::new(1, methods::DAEMON_PING, None));

        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["protocol_version"], PROTOCOL_VERSION);
    }

    #[test]
    fn sessions_list_returns_catalog_including_warnings() {
        let mut options = test_options();
        options.settings.remotes.push(ConfiguredRemote {
            name: "bad".to_string(),
            base_url: "https://127.0.0.1:8787".to_string(),
            token: "secret".to_string(),
            terminal_token: None,
            enabled: true,
        });
        let mut daemon = RpcDaemon::new(options);

        let response = daemon.handle_request(JsonRpcRequest::new(1, methods::SESSIONS_LIST, None));

        assert!(response.error.is_none());
        let catalog: SessionCatalog = serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(catalog.sessions.is_empty());
        assert_eq!(catalog.warnings.len(), 1);
        assert!(catalog.warnings[0].contains("bad:"));
    }

    #[test]
    fn typed_client_returns_empty_session_catalog_from_empty_provider_roots() {
        let mut client = LocalRpcClient::new(test_options());

        let catalog = client.session_catalog().unwrap();

        assert_eq!(catalog, SessionCatalog::default());
    }

    #[test]
    fn typed_client_returns_session_summaries_from_daemon_app_layer() {
        let mut client = LocalRpcClient::new(test_options());

        let response = client.session_summaries().unwrap();

        assert_eq!(response.sessions.len(), 0);
        assert_eq!(response.counts.total, 0);
        assert!(response.warnings.is_empty());
    }

    #[test]
    fn session_detail_returns_not_found_rpc_error() {
        let mut client = LocalRpcClient::new(test_options());

        let error = client
            .session_detail(missing_session_ref())
            .unwrap_err()
            .to_string();

        assert!(error.contains("RPC error 404: session not found"));
    }

    #[test]
    fn settings_summary_marks_daemon_runtime_available() {
        let mut options = test_options();
        options.settings.terminal.enabled = true;
        options.settings.terminal.token = "terminal-secret".to_string();
        let mut client = LocalRpcClient::new(options);

        let summary = client
            .settings_summary(SettingsSummaryParams {
                gateway_bind: "127.0.0.1:8787".to_string(),
                terminal_socket_available: true,
            })
            .unwrap();

        assert_eq!(summary.bind, "127.0.0.1:8787");
        assert!(summary.terminal.daemon_available);
        assert!(summary.terminal.terminal_socket_available);
        assert!(summary.terminal.token_configured);
        let body = serde_json::to_string(&summary).unwrap();
        assert!(!body.contains("terminal-secret"));
    }

    #[test]
    fn typed_client_updates_settings_through_json_rpc() {
        let mut client = LocalRpcClient::new(test_options());
        let mut settings = Settings::default();
        settings.ensure_defaults();
        settings.share.base_url = "http://host:8787".to_string();
        settings.share.token = "secret".to_string();
        settings.launch_defaults.resume.yolo = true;

        let updated = client.settings_update(settings.clone()).unwrap();

        assert_eq!(updated, settings);
    }

    #[test]
    fn typed_client_updates_ai_settings_through_daemon_state() {
        let mut client = LocalRpcClient::new(test_options());

        let summary = client
            .update_ai_settings(AiSettingsUpdateParams {
                base_url: Some(" https://example.test/v1 ".to_string()),
                model: Some(" custom-model ".to_string()),
                api_key: Some("sk-secret".to_string()),
                ..AiSettingsUpdateParams::default()
            })
            .unwrap();
        let settings = client.settings().unwrap();

        assert_eq!(summary.base_url, "https://example.test/v1");
        assert_eq!(summary.model, "custom-model");
        assert!(summary.api_key_configured);
        assert_eq!(settings.ai.base_url, "https://example.test/v1");
        assert_eq!(settings.ai.model, "custom-model");
        assert_eq!(settings.ai.api_key, "sk-secret");
    }

    #[test]
    fn typed_client_session_methods_return_rpc_errors_with_empty_provider_roots() {
        let mut client = LocalRpcClient::new(test_options());
        let session = missing_session_ref();
        let current_cwd = std::env::temp_dir().to_string_lossy().to_string();

        let share_error = client.share_url(session.clone()).unwrap_err().to_string();
        assert!(share_error.contains("RPC error 404: session not found"));

        let options_error = client
            .launch_options(LaunchOptionsParams {
                session: session.clone(),
                mode: LaunchModeWire::Resume,
                current_cwd: current_cwd.clone(),
            })
            .unwrap_err()
            .to_string();
        assert!(options_error.contains("RPC error 404: session not found"));

        let prepare_error = client
            .launch_prepare(LaunchPrepareParams {
                session,
                mode: LaunchModeWire::Resume,
                current_cwd,
                options: Vec::new(),
            })
            .unwrap_err()
            .to_string();
        assert!(prepare_error.contains("RPC error 404: session not found"));
    }

    #[test]
    fn terminal_status_methods_read_shared_daemon_state() {
        let state = DaemonState::with_backend(test_options(), FakeTerminalBackend::default());
        let terminal_id = {
            let manager = state.terminal_manager();
            let attachment = manager
                .lock()
                .unwrap()
                .open(
                    "client-a",
                    TerminalOpen {
                        session: missing_session_ref(),
                        mode: TerminalModeWire::Resume,
                        size: TerminalSize { cols: 80, rows: 24 },
                    },
                    TerminalLaunchTarget::local(
                        "codex".to_string(),
                        vec!["resume".to_string(), "missing".to_string()],
                        None,
                    ),
                )
                .unwrap()
                .terminal;
            attachment.terminal_id
        };
        let mut client = LocalRpcClient::from_daemon(RpcDaemon::with_state(state));

        let list = client.terminal_list().unwrap();
        let terminal = client.terminal_get(terminal_id).unwrap();

        assert_eq!(list.terminals.len(), 1);
        assert_eq!(terminal.state, TerminalStateWire::Running);
        assert_eq!(terminal.attached_clients, 1);
    }

    #[test]
    fn unknown_method_returns_json_rpc_error() {
        let mut daemon = RpcDaemon::new(test_options());

        let response = daemon.handle_request(JsonRpcRequest::new(
            coca_protocol::RpcId::Number(1),
            "missing",
            None,
        ));

        assert_eq!(response.error.unwrap().code, -32601);
    }

    fn test_options() -> RpcDaemonOptions {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        let (codex_home, claude_home) = empty_provider_roots();
        RpcDaemonOptions {
            settings,
            settings_path: None,
            codex_home: Some(codex_home),
            claude_home: Some(claude_home),
            provider_filter: ProviderFilter::All,
        }
    }

    fn missing_session_ref() -> SessionRef {
        SessionRef {
            origin: "local".to_string(),
            provider: "codex".to_string(),
            id: "missing".to_string(),
        }
    }

    fn empty_provider_roots() -> (PathBuf, PathBuf) {
        static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

        let root = std::env::temp_dir().join(format!(
            "coca-daemon-empty-roots-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        (root.join("codex"), root.join("claude"))
    }
}
