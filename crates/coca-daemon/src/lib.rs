pub use coca_core::server::{serve, CoreOptions};

use std::path::PathBuf;

use anyhow::Result;
use coca_core::catalog::{load_session_catalog, SessionCatalogOptions};
use coca_core::frontend::{
    launch_options_with_defaults, prepare_launch, share_url_for_session, FrontendError,
};
use coca_core::launch::{LaunchMode, LaunchOption, LaunchOptionKind};
use coca_core::model::{ProviderFilter, ProviderKind, Session, SessionOrigin};
use coca_core::settings::{save_settings, Settings};
use coca_protocol::{
    methods, CorePingResult, JsonRpcRequest, JsonRpcResponse, LaunchModeWire, LaunchOptionKindWire,
    LaunchOptionWire, LaunchOptionsParams, LaunchPrepareParams, PreparedLaunch, SessionGetParams,
    SessionRef, SettingsUpdateParams, ShareUrlParams,
};
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
            methods::SESSIONS_LIST => serde_json::to_value(self.sessions()?),
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
                let session = self
                    .find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                let url = share_url_for_session(&self.options.settings, &session)
                    .map_err(RpcDispatchError::from_frontend)?;
                Ok(json!({ "url": url }))
            }
            methods::LAUNCH_OPTIONS => {
                let params: LaunchOptionsParams = parse_params(request.params)?;
                let session = self
                    .find_session(&params.session)?
                    .ok_or_else(|| RpcDispatchError::not_found("session not found"))?;
                let mode = launch_mode(params.mode);
                let options = launch_options_with_defaults(
                    &self.options.settings,
                    &session,
                    mode,
                    PathBuf::from(params.current_cwd).as_path(),
                )
                .map_err(RpcDispatchError::from_frontend)?;
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
                let target =
                    prepare_launch(&session, launch_mode(params.mode), &current_cwd, &options)
                        .map_err(RpcDispatchError::from_frontend)?;
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

    fn sessions(&self) -> Result<Vec<Session>, RpcDispatchError> {
        let catalog = load_session_catalog(SessionCatalogOptions {
            codex_home: self.options.codex_home.clone(),
            claude_home: self.options.claude_home.clone(),
            provider_filter: self.options.provider_filter,
            remote_config: self.options.settings.remote_config(),
        })
        .map_err(|err| RpcDispatchError::internal(format!("{err:#}")))?;
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

    fn from_frontend(error: FrontendError) -> Self {
        Self {
            code: 400,
            message: error.message(),
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
    use coca_protocol::{methods, PROTOCOL_VERSION};

    #[test]
    fn ping_returns_protocol_version() {
        let mut daemon = RpcDaemon::new(test_options());

        let response = daemon.handle_request(JsonRpcRequest::new(1, methods::CORE_PING, None));

        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["protocol_version"], PROTOCOL_VERSION);
    }

    #[test]
    fn sessions_list_returns_json_array() {
        let mut daemon = RpcDaemon::new(test_options());

        let response = daemon.handle_request(JsonRpcRequest::new(1, methods::SESSIONS_LIST, None));

        assert!(response.error.is_none());
        assert!(response.result.unwrap().is_array());
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
        RpcDaemonOptions {
            settings,
            settings_path: None,
            codex_home: None,
            claude_home: None,
            provider_filter: ProviderFilter::All,
        }
    }
}
