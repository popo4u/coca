use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const JSONRPC_VERSION: &str = "2.0";

pub mod methods {
    pub const CORE_PING: &str = "core.ping";
    pub const SESSIONS_LIST: &str = "sessions.list";
    pub const SESSIONS_GET: &str = "sessions.get";
    pub const SETTINGS_GET: &str = "settings.get";
    pub const SETTINGS_UPDATE: &str = "settings.update";
    pub const SHARE_URL: &str = "share.url";
    pub const LAUNCH_OPTIONS: &str = "launch.options";
    pub const LAUNCH_PREPARE: &str = "launch.prepare";
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
pub struct CorePingResult {
    pub protocol_version: u32,
    pub service: String,
}

impl Default for CorePingResult {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            service: "coca-core".to_string(),
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
}
