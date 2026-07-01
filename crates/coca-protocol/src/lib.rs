use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const JSONRPC_VERSION: &str = "2.0";

pub mod methods {
    pub const DAEMON_PING: &str = "daemon.ping";
    pub const SESSIONS_LIST: &str = "sessions.list";
    pub const SESSIONS_SUMMARIES: &str = "sessions.summaries";
    pub const SESSIONS_GET: &str = "sessions.get";
    pub const SESSIONS_DETAIL: &str = "sessions.detail";
    pub const SETTINGS_GET: &str = "settings.get";
    pub const SETTINGS_SUMMARY: &str = "settings.summary";
    pub const SETTINGS_UPDATE: &str = "settings.update";
    pub const SETTINGS_AI_UPDATE: &str = "settings.ai.update";
    pub const SHARE_URL: &str = "share.url";
    pub const LAUNCH_OPTIONS: &str = "launch.options";
    pub const LAUNCH_PREPARE: &str = "launch.prepare";
    pub const TERMINAL_LIST: &str = "terminal.list";
    pub const TERMINAL_GET: &str = "terminal.get";
    pub const AUTH_CAPABILITIES: &str = "auth.capabilities";
    pub const AUTH_LOGIN: &str = "auth.login";
    pub const AUTH_SIGNUP: &str = "auth.signup";
    pub const AUTH_VALIDATE: &str = "auth.validate";
    pub const AUTH_LOGOUT: &str = "auth.logout";
    pub const ACCOUNT_ME: &str = "account.me";
    pub const ACCOUNT_PROFILE_UPDATE: &str = "account.profile.update";
    pub const ACCOUNT_PASSWORD_UPDATE: &str = "account.password.update";
    pub const ACCOUNT_DEVICES_LIST: &str = "account.devices.list";
    pub const ACCOUNT_DEVICES_REVOKE: &str = "account.devices.revoke";
    pub const ACCOUNT_TOKENS_LIST: &str = "account.tokens.list";
    pub const ACCOUNT_TOKENS_CREATE: &str = "account.tokens.create";
    pub const ACCOUNT_TOKENS_REVOKE: &str = "account.tokens.revoke";
    pub const ACCOUNT_SHARE_LINKS_LIST: &str = "account.share_links.list";
    pub const ACCOUNT_SHARE_LINKS_REVOKE: &str = "account.share_links.revoke";
    pub const SHARE_PUBLIC_DETAIL: &str = "share.public.detail";
}

pub mod auth_scopes {
    pub const SESSIONS_READ: &str = "sessions.read";
    pub const SHARE_MANAGE: &str = "share.manage";
    pub const ACCOUNT_MANAGE: &str = "account.manage";
    pub const TOKENS_MANAGE: &str = "tokens.manage";
    pub const TERMINAL_READ: &str = "terminal.read";
    pub const TERMINAL_WRITE: &str = "terminal.write";
    pub const TERMINAL_KILL: &str = "terminal.kill";
}

pub mod terminal_events {
    pub const OPEN: &str = "terminal.open";
    pub const ATTACH: &str = "terminal.attach";
    pub const INPUT: &str = "terminal.input";
    pub const RESIZE: &str = "terminal.resize";
    pub const DETACH: &str = "terminal.detach";
    pub const CLOSE: &str = "terminal.close";
    pub const OPENED: &str = "terminal.opened";
    pub const OUTPUT: &str = "terminal.output";
    pub const EXIT: &str = "terminal.exit";
    pub const ERROR: &str = "terminal.error";
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RpcId {
    Number(i64),
    String(String),
    Null,
}

impl From<i64> for RpcId {
    fn from(value: i64) -> Self {
        Self::Number(value)
    }
}

impl From<&str> for RpcId {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: RpcId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: impl Into<RpcId>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: impl Into<RpcId>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: impl Into<RpcId>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonPingResult {
    pub protocol_version: u32,
    pub service: String,
}

impl Default for DaemonPingResult {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            service: "coca-daemon".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionRef {
    pub origin: String,
    pub provider: String,
    pub id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionListParams {
    pub provider: Option<String>,
    pub query: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionGetParams {
    pub session: SessionRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShareUrlParams {
    pub session: SessionRef,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LaunchModeWire {
    Resume,
    Fork,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LaunchOptionKindWire {
    UseCurrentDir,
    Yolo,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchOptionWire {
    pub kind: LaunchOptionKindWire,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchOptionsParams {
    pub session: SessionRef,
    pub mode: LaunchModeWire,
    pub current_cwd: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchPrepareParams {
    pub session: SessionRef,
    pub mode: LaunchModeWire,
    pub current_cwd: String,
    pub options: Vec<LaunchOptionWire>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PreparedLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SettingsUpdateParams {
    pub settings: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SettingsSummaryParams {
    pub gateway_bind: String,
    pub terminal_socket_available: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AiSettingsUpdateParams {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthLoginParams {
    pub email: String,
    pub password: String,
    pub device_label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthSignupParams {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub device_label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthValidateParams {
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthLogoutParams {
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountSubjectParams {
    pub user_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountProfileUpdateParams {
    pub user_id: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountPasswordUpdateParams {
    pub user_id: String,
    pub current_password: String,
    pub new_password: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountDevicesRevokeParams {
    pub user_id: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountTokensCreateParams {
    pub user_id: String,
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountTokensRevokeParams {
    pub user_id: String,
    pub token_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountShareLinksRevokeParams {
    pub user_id: String,
    pub link_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublicShareDetailParams {
    pub link_id: String,
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalSeq(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TerminalModeWire {
    Resume,
    Fork,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TerminalStateWire {
    Starting,
    Running,
    Detached,
    Exited,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalSessionSummary {
    pub terminal_id: TerminalId,
    pub session: SessionRef,
    pub mode: TerminalModeWire,
    pub state: TerminalStateWire,
    pub attached_clients: usize,
    pub active_writer: Option<String>,
    pub last_seq: TerminalSeq,
    pub size: TerminalSize,
    pub exit: Option<TerminalExitInfo>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalExitInfo {
    pub code: Option<i32>,
    pub signal: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalListResult {
    pub terminals: Vec<TerminalSessionSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalGetParams {
    pub terminal_id: TerminalId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", content = "payload")]
pub enum TerminalClientFrame {
    #[serde(rename = "terminal.open")]
    Open(TerminalOpen),
    #[serde(rename = "terminal.attach")]
    Attach(TerminalAttach),
    #[serde(rename = "terminal.input")]
    Input(TerminalInput),
    #[serde(rename = "terminal.resize")]
    Resize(TerminalResize),
    #[serde(rename = "terminal.detach")]
    Detach(TerminalDetach),
    #[serde(rename = "terminal.close")]
    Close(TerminalClose),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", content = "payload")]
pub enum TerminalServerFrame {
    #[serde(rename = "terminal.opened")]
    Opened(TerminalOpened),
    #[serde(rename = "terminal.output")]
    Output(TerminalOutput),
    #[serde(rename = "terminal.exit")]
    Exit(TerminalExit),
    #[serde(rename = "terminal.error")]
    Error(TerminalError),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalOpen {
    pub session: SessionRef,
    pub mode: TerminalModeWire,
    pub size: TerminalSize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalAttach {
    pub terminal_id: TerminalId,
    pub since_seq: Option<TerminalSeq>,
    pub size: TerminalSize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalInput {
    pub terminal_id: TerminalId,
    pub data_b64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalResize {
    pub terminal_id: TerminalId,
    pub size: TerminalSize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalDetach {
    pub terminal_id: TerminalId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalClose {
    pub terminal_id: TerminalId,
    pub kill: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalOpened {
    pub terminal: TerminalSessionSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalOutput {
    pub terminal_id: TerminalId,
    pub seq: TerminalSeq,
    pub data_b64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalExit {
    pub terminal_id: TerminalId,
    pub exit: TerminalExitInfo,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalError {
    pub request_id: Option<String>,
    pub terminal_id: Option<TerminalId>,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrips_with_typed_params() {
        let params = serde_json::to_value(LaunchOptionsParams {
            session: SessionRef {
                origin: "local".to_string(),
                provider: "codex".to_string(),
                id: "sid".to_string(),
            },
            mode: LaunchModeWire::Resume,
            current_cwd: "/work".to_string(),
        })
        .unwrap();
        let request = JsonRpcRequest::new(7, methods::LAUNCH_OPTIONS, Some(params));

        let json = serde_json::to_string(&request).unwrap();
        let decoded: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, request);
        assert_eq!(decoded.jsonrpc, JSONRPC_VERSION);
    }

    #[test]
    fn response_roundtrips_with_error_shape() {
        let response = JsonRpcResponse::error("abc", -32601, "method not found");

        let json = serde_json::to_string(&response).unwrap();
        let decoded: JsonRpcResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, response);
        assert!(decoded.result.is_none());
        assert_eq!(decoded.error.unwrap().code, -32601);
    }

    #[test]
    fn settings_summary_params_roundtrip() {
        let params = SettingsSummaryParams {
            gateway_bind: "127.0.0.1:8787".to_string(),
            terminal_socket_available: true,
        };

        let json = serde_json::to_string(&params).unwrap();
        let decoded: SettingsSummaryParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, params);
        assert!(json.contains("gateway_bind"));
    }

    #[test]
    fn terminal_frames_roundtrip_with_event_tags() {
        let frame = TerminalClientFrame::Open(TerminalOpen {
            session: SessionRef {
                origin: "local".to_string(),
                provider: "codex".to_string(),
                id: "sid".to_string(),
            },
            mode: TerminalModeWire::Resume,
            size: TerminalSize { cols: 80, rows: 24 },
        });

        let json = serde_json::to_string(&frame).unwrap();
        let decoded: TerminalClientFrame = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, frame);
        assert!(json.contains("terminal.open"));
    }

    #[test]
    fn auth_method_names_match_gateway_contract() {
        assert_eq!(methods::AUTH_CAPABILITIES, "auth.capabilities");
        assert_eq!(methods::AUTH_LOGIN, "auth.login");
        assert_eq!(methods::AUTH_SIGNUP, "auth.signup");
        assert_eq!(methods::AUTH_VALIDATE, "auth.validate");
        assert_eq!(methods::AUTH_LOGOUT, "auth.logout");
        assert_eq!(methods::ACCOUNT_ME, "account.me");
        assert_eq!(methods::ACCOUNT_PROFILE_UPDATE, "account.profile.update");
        assert_eq!(methods::ACCOUNT_PASSWORD_UPDATE, "account.password.update");
        assert_eq!(methods::ACCOUNT_DEVICES_LIST, "account.devices.list");
        assert_eq!(methods::ACCOUNT_DEVICES_REVOKE, "account.devices.revoke");
        assert_eq!(methods::ACCOUNT_TOKENS_LIST, "account.tokens.list");
        assert_eq!(methods::ACCOUNT_TOKENS_CREATE, "account.tokens.create");
        assert_eq!(methods::ACCOUNT_TOKENS_REVOKE, "account.tokens.revoke");
        assert_eq!(
            methods::ACCOUNT_SHARE_LINKS_LIST,
            "account.share_links.list"
        );
        assert_eq!(
            methods::ACCOUNT_SHARE_LINKS_REVOKE,
            "account.share_links.revoke"
        );
        assert_eq!(methods::SHARE_PUBLIC_DETAIL, "share.public.detail");
    }

    #[test]
    fn auth_params_roundtrip() {
        let params = AuthSignupParams {
            email: "user@example.com".to_string(),
            password: "password".to_string(),
            display_name: Some("User".to_string()),
            device_label: Some("Browser".to_string()),
        };

        let json = serde_json::to_string(&params).unwrap();
        let decoded: AuthSignupParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, params);
    }

    #[test]
    fn auth_scopes_and_share_params_roundtrip() {
        let params = AccountTokensCreateParams {
            user_id: "usr_1".to_string(),
            name: "CI".to_string(),
            scopes: vec![
                auth_scopes::SESSIONS_READ.to_string(),
                auth_scopes::TERMINAL_READ.to_string(),
            ],
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: AccountTokensCreateParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, params);
        assert!(json.contains("sessions.read"));
        assert!(json.contains("terminal.read"));

        let share = PublicShareDetailParams {
            link_id: "shr_1".to_string(),
            token: "coca_share_secret".to_string(),
        };
        let json = serde_json::to_string(&share).unwrap();
        let decoded: PublicShareDetailParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, share);
    }
}
