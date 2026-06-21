pub use coca_core::server::{serve, CoreOptions};

use std::path::PathBuf;

use anyhow::{Context, Result};
use coca_app::{AppOptions, AppService};
use coca_core::catalog::SessionCatalog;
use coca_core::launch::{LaunchMode, LaunchOption, LaunchOptionKind};
use coca_core::model::{ProviderFilter, ProviderKind, Session, SessionOrigin};
use coca_core::settings::{save_settings, Settings};
use coca_protocol::{
    methods, CorePingResult, JsonRpcRequest, JsonRpcResponse, LaunchModeWire, LaunchOptionKindWire,
    LaunchOptionWire, LaunchOptionsParams, LaunchPrepareParams, PreparedLaunch, RpcId,
    SessionGetParams, SessionRef, SettingsUpdateParams, ShareUrlParams,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

#[cfg(unix)]
pub fn serve_rpc(socket_path: &std::path::Path, options: RpcDaemonOptions) -> Result<()> {
    let mut daemon = RpcDaemon::new(options);
    coca_ipc::unix::serve(socket_path, move |request| daemon.handle_request(request))
}

#[cfg(not(unix))]
pub fn serve_rpc(_socket_path: &std::path::Path, _options: RpcDaemonOptions) -> Result<()> {
    anyhow::bail!("local daemon IPC is not implemented on this platform yet")
}

#[derive(Clone, Debug)]
pub struct RpcDaemonOptions {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
}

#[derive(Clone, Debug)]
pub struct RpcDaemon {
    options: RpcDaemonOptions,
}

impl RpcDaemon {
    pub fn new(options: RpcDaemonOptions) -> Self {
        Self { options }
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
            methods::CORE_PING => serde_json::to_value(CorePingResult::default()),
            methods::SESSIONS_LIST => serde_json::to_value(
                self.app()
                    .session_catalog()
                    .map_err(RpcDispatchError::from_anyhow)?,
            ),
            methods::SESSIONS_GET => {
                let params: SessionGetParams = parse_params(request.params)?;
                let session = self
                    .find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                serde_json::to_value(session)
            }
            methods::SETTINGS_GET => serde_json::to_value(&self.options.settings),
            methods::SETTINGS_UPDATE => {
                let params: SettingsUpdateParams = parse_params(request.params)?;
                let mut settings: Settings = serde_json::from_value(params.settings)
                    .map_err(|err| RpcDispatchError::invalid_params(err.to_string()))?;
                settings.ensure_defaults();
                settings
                    .validate()
                    .map_err(|err| RpcDispatchError::invalid_params(format!("{err:#}")))?;
                if let Some(path) = &self.options.settings_path {
                    save_settings(path, &settings)
                        .map_err(|err| RpcDispatchError::internal(format!("{err:#}")))?;
                }
                self.options.settings = settings;
                serde_json::to_value(&self.options.settings)
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
            _ => return Err(RpcDispatchError::method_not_found()),
        }
        .map_err(|err| RpcDispatchError::internal(err.to_string()))
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
pub struct LocalRpcClient {
    daemon: RpcDaemon,
    next_id: i64,
}

impl LocalRpcClient {
    pub fn new(options: RpcDaemonOptions) -> Self {
        Self::from_daemon(RpcDaemon::new(options))
    }

    pub fn from_daemon(daemon: RpcDaemon) -> Self {
        Self { daemon, next_id: 1 }
    }

    pub fn session_catalog(&mut self) -> Result<SessionCatalog> {
        self.call(methods::SESSIONS_LIST, None)
    }

    pub fn settings(&mut self) -> Result<Settings> {
        self.call(methods::SETTINGS_GET, None)
    }

    pub fn settings_update(&mut self, settings: Settings) -> Result<Settings> {
        let settings = serde_json::to_value(settings).context("failed to encode settings")?;
        self.call(
            methods::SETTINGS_UPDATE,
            to_params(SettingsUpdateParams { settings })?,
        )
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

fn launch_mode(mode: LaunchModeWire) -> LaunchMode {
    match mode {
        LaunchModeWire::Resume => LaunchMode::Resume,
        LaunchModeWire::Fork => LaunchMode::Fork,
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
    use coca_core::settings::ConfiguredRemote;
    use coca_protocol::{methods, PROTOCOL_VERSION};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn ping_returns_protocol_version() {
        let mut daemon = RpcDaemon::new(test_options());

        let response = daemon.handle_request(JsonRpcRequest::new(1, methods::CORE_PING, None));

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
