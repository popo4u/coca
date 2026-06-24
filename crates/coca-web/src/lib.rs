use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use coca_app::{AuthValidation, StreamInfo};
use coca_protocol::{
    methods, AccountDevicesRevokeParams, AccountPasswordUpdateParams, AccountProfileUpdateParams,
    AccountSubjectParams, AccountTokensCreateParams, AccountTokensRevokeParams,
    AiSettingsUpdateParams, AuthLoginParams, AuthLogoutParams, AuthSignupParams,
    AuthValidateParams, DaemonPingResult, JsonRpcRequest, RpcError, RpcId, SessionGetParams,
    SessionRef, SettingsSummaryParams, ShareUrlParams, TerminalClientFrame, TerminalError,
    TerminalListResult, TerminalServerFrame,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

#[derive(Clone, Debug)]
pub struct GatewayOptions {
    pub bind: String,
    pub read_token: String,
    pub share_base_url: String,
    pub terminal_enabled: bool,
    pub terminal_token: String,
    pub static_dir: PathBuf,
    pub daemon_socket: Option<PathBuf>,
    pub terminal_socket: Option<PathBuf>,
}

const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_WEBSOCKET_PAYLOAD_LEN: u64 = coca_ipc::MAX_FRAME_LEN as u64;

pub fn serve(options: GatewayOptions) -> Result<()> {
    if options.read_token.trim().is_empty() {
        anyhow::bail!("gateway API token must not be empty");
    }

    let listener = TcpListener::bind(options.bind.trim())
        .with_context(|| format!("failed to bind gateway {}", options.bind))?;
    print_startup_info(&options, listener.local_addr()?);
    for stream in listener.incoming() {
        let stream = stream.context("failed to accept gateway connection")?;
        let options = options.clone();
        thread::spawn(move || {
            if let Err(err) = handle_stream(stream, options) {
                eprintln!("coca gateway connection failed: {err:#}");
            }
        });
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct GatewayStartupInfo {
    listen_addr: String,
    browser_url: String,
    share_base_url: String,
    token: String,
}

fn print_startup_info(options: &GatewayOptions, listen_addr: SocketAddr) {
    let info = startup_info(options, listen_addr);
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "coca gateway");
    let _ = writeln!(stdout, "  address: {}", info.listen_addr);
    let _ = writeln!(stdout, "  browser: {}", info.browser_url);
    let _ = writeln!(stdout, "  share base_url: {}", info.share_base_url);
    let _ = writeln!(stdout, "  token: {}", info.token);
    let _ = stdout.flush();
}

fn startup_info(options: &GatewayOptions, listen_addr: SocketAddr) -> GatewayStartupInfo {
    let token = options.read_token.trim().to_string();
    GatewayStartupInfo {
        listen_addr: listen_addr.to_string(),
        browser_url: format!(
            "{}/?token={}",
            local_gateway_base_url(listen_addr),
            percent_encode_query_value(&token)
        ),
        share_base_url: options.share_base_url.trim().to_string(),
        token,
    }
}

fn local_gateway_base_url(addr: SocketAddr) -> String {
    let host = match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_string(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    format!("http://{host}:{}", addr.port())
}

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn handle_stream(stream: TcpStream, options: GatewayOptions) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let response = match read_request(&mut reader)? {
        Some(request) if request.path() == "/api/v1/terminal/ws" => {
            return handle_terminal_websocket(reader, request, options);
        }
        Some(request) => route_request(&request, &options),
        None => Response::text(400, "Bad Request", "Malformed request"),
    };
    write_response(reader.get_mut(), &request_safe_response(&response))
}

fn route_request(request: &Request, options: &GatewayOptions) -> Response {
    let path = request.path();
    if path.starts_with("/api/") {
        return route_api(request, options);
    }
    route_static(request, &options.static_dir)
}

fn route_api(request: &Request, options: &GatewayOptions) -> Response {
    let auth = if is_public_auth_route(request) {
        None
    } else {
        match authenticate_api_request(request, options) {
            Ok(auth) => Some(auth),
            Err(response) => return response,
        }
    };

    match (request.method.as_str(), request.path()) {
        ("GET", "/api/v1/health") => json_response(gateway_health(options)),
        ("GET", "/api/v1/auth/capabilities") => daemon_json_response(daemon_rpc::<Value>(
            options,
            methods::AUTH_CAPABILITIES,
            None,
        )),
        ("POST", "/api/v1/auth/login") => {
            let Ok(body) = serde_json::from_slice::<AuthLoginParams>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid auth login payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::AUTH_LOGIN,
                Some(rpc_params(body)),
            ))
        }
        ("POST", "/api/v1/auth/signup") => {
            let Ok(body) = serde_json::from_slice::<AuthSignupParams>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid auth signup payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::AUTH_SIGNUP,
                Some(rpc_params(body)),
            ))
        }
        ("POST", "/api/v1/auth/logout") => {
            let Some(token) = user_auth_token(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::AUTH_LOGOUT,
                Some(rpc_params(AuthLogoutParams { token })),
            ))
        }
        ("GET", "/api/v1/account/me") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_ME,
                Some(rpc_params(AccountSubjectParams { user_id })),
            ))
        }
        ("PATCH", "/api/v1/account/profile") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            let Ok(body) = serde_json::from_slice::<AccountProfileBody>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid account profile payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_PROFILE_UPDATE,
                Some(rpc_params(AccountProfileUpdateParams {
                    user_id,
                    display_name: body.display_name,
                })),
            ))
        }
        ("POST", "/api/v1/account/password") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            let Ok(body) = serde_json::from_slice::<AccountPasswordBody>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid account password payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_PASSWORD_UPDATE,
                Some(rpc_params(AccountPasswordUpdateParams {
                    user_id,
                    current_password: body.current_password,
                    new_password: body.new_password,
                })),
            ))
        }
        ("GET", "/api/v1/account/devices") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_DEVICES_LIST,
                Some(rpc_params(AccountSubjectParams { user_id })),
            ))
        }
        ("POST", "/api/v1/account/devices/revoke") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            let Ok(body) = serde_json::from_slice::<DeviceRevokeBody>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid account device revoke payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_DEVICES_REVOKE,
                Some(rpc_params(AccountDevicesRevokeParams {
                    user_id,
                    session_id: body.session_id,
                })),
            ))
        }
        ("GET", "/api/v1/account/tokens") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_TOKENS_LIST,
                Some(rpc_params(AccountSubjectParams { user_id })),
            ))
        }
        ("POST", "/api/v1/account/tokens") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            let Ok(body) = serde_json::from_slice::<AccessTokenCreateBody>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid account token payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_TOKENS_CREATE,
                Some(rpc_params(AccountTokensCreateParams {
                    user_id,
                    name: body.name,
                })),
            ))
        }
        ("POST", "/api/v1/account/tokens/revoke") => {
            let Some(user_id) = user_id(&auth) else {
                return Response::text(401, "Unauthorized", "unauthorized");
            };
            let Ok(body) = serde_json::from_slice::<AccessTokenRevokeBody>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid account token revoke payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::ACCOUNT_TOKENS_REVOKE,
                Some(rpc_params(AccountTokensRevokeParams {
                    user_id,
                    token_id: body.token_id,
                })),
            ))
        }
        ("GET", "/api/v1/sessions") => daemon_json_response(daemon_rpc::<Value>(
            options,
            methods::SESSIONS_SUMMARIES,
            None,
        )),
        ("GET", "/api/sessions") => daemon_json_response(legacy_sessions(options)),
        ("GET", "/api/v1/session") => {
            let Some(reference) = session_ref_from_query(request) else {
                return Response::text(400, "Bad Request", "missing session reference");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::SESSIONS_DETAIL,
                Some(rpc_params(SessionGetParams { session: reference })),
            ))
        }
        ("GET", "/api/v1/config/summary") => daemon_json_response(daemon_rpc::<Value>(
            options,
            methods::SETTINGS_SUMMARY,
            Some(rpc_params(SettingsSummaryParams {
                gateway_bind: options.bind.clone(),
                terminal_socket_available: terminal_stream_available(options),
            })),
        )),
        ("GET", "/api/v1/terminal/sessions") => {
            if let Some(response) = reject_terminal_token_request(request, options) {
                return response;
            }
            daemon_json_response(terminal_sessions(options))
        }
        ("PUT", "/api/v1/config/ai") => {
            let Ok(body) = serde_json::from_slice::<AiSettingsUpdateParams>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid ai config payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::SETTINGS_AI_UPDATE,
                Some(rpc_params(body)),
            ))
        }
        ("POST", "/api/v1/share-session") => {
            let Ok(body) = serde_json::from_slice::<ShareSessionRequest>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid share-session payload");
            };
            daemon_json_response(daemon_rpc::<Value>(
                options,
                methods::SHARE_URL,
                Some(rpc_params(ShareUrlParams {
                    session: body.session,
                })),
            ))
        }
        ("GET", "/api/v1/stream") => json_response(json!({
            "ok": false,
            "error": "terminal stream transport is reserved but not implemented",
            "stream": StreamInfo::default(),
        }))
        .with_status(501, "Not Implemented"),
        (_, "/api/v1/health")
        | (_, "/api/v1/auth/capabilities")
        | (_, "/api/v1/auth/login")
        | (_, "/api/v1/auth/signup")
        | (_, "/api/v1/auth/logout")
        | (_, "/api/v1/account/me")
        | (_, "/api/v1/account/profile")
        | (_, "/api/v1/account/password")
        | (_, "/api/v1/account/devices")
        | (_, "/api/v1/account/devices/revoke")
        | (_, "/api/v1/account/tokens")
        | (_, "/api/v1/account/tokens/revoke")
        | (_, "/api/v1/sessions")
        | (_, "/api/sessions")
        | (_, "/api/v1/session")
        | (_, "/api/v1/config/summary")
        | (_, "/api/v1/terminal/sessions")
        | (_, "/api/v1/config/ai")
        | (_, "/api/v1/share-session")
        | (_, "/api/v1/stream") => Response::text(405, "Method Not Allowed", "method not allowed"),
        _ => Response::text(404, "Not Found", "not found"),
    }
}

fn legacy_sessions(options: &GatewayOptions) -> DaemonRpcResult<Vec<Value>> {
    daemon_rpc::<LegacySessionCatalog>(options, methods::SESSIONS_LIST, None)
        .map(|catalog| catalog.sessions)
}

#[derive(Debug, Deserialize)]
struct LegacySessionCatalog {
    #[serde(default)]
    sessions: Vec<Value>,
}

fn rpc_params<T>(params: T) -> Value
where
    T: Serialize,
{
    serde_json::to_value(params).expect("RPC params should serialize")
}

fn terminal_stream_available(options: &GatewayOptions) -> bool {
    terminal_socket_path(options)
        .map(|path| terminal_stream_connectable(&path))
        .unwrap_or(false)
}

#[cfg(unix)]
fn terminal_stream_connectable(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
fn terminal_stream_connectable(_path: &Path) -> bool {
    false
}

fn route_static(request: &Request, static_dir: &Path) -> Response {
    if request.method != "GET" && request.method != "HEAD" {
        return Response::text(405, "Method Not Allowed", "method not allowed");
    }
    let relative = match request.path() {
        "/" => PathBuf::from("index.html"),
        path => {
            let clean = path.trim_start_matches('/');
            if clean.is_empty()
                || clean.contains("..")
                || clean.split('/').any(|part| part.starts_with('.'))
            {
                return Response::text(404, "Not Found", "not found");
            }
            PathBuf::from(clean)
        }
    };

    let candidate = static_dir.join(&relative);
    if candidate.is_file() {
        return file_response(&candidate);
    }

    let index = static_dir.join("index.html");
    if index.is_file() {
        return file_response(&index);
    }

    Response::text(
        404,
        "Not Found",
        "React web assets were not built. Run npm install && npm run build in app/web.",
    )
}

fn file_response(path: &Path) -> Response {
    match fs::read(path) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: content_type(path),
            body,
        },
        Err(err) => Response::text(500, "Internal Server Error", format!("{err:#}")),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ApiAuth {
    LegacyShare,
    Account { token: String, user_id: String },
}

fn is_public_auth_route(request: &Request) -> bool {
    matches!(
        request.path(),
        "/api/v1/auth/capabilities" | "/api/v1/auth/login" | "/api/v1/auth/signup"
    )
}

fn authenticate_api_request(
    request: &Request,
    options: &GatewayOptions,
) -> std::result::Result<ApiAuth, Response> {
    let Some(token) = request.read_token() else {
        return Err(Response::text(401, "Unauthorized", "unauthorized"));
    };
    if token == options.read_token.trim() {
        return Ok(ApiAuth::LegacyShare);
    }
    match daemon_rpc::<Option<AuthValidation>>(
        options,
        methods::AUTH_VALIDATE,
        Some(rpc_params(AuthValidateParams {
            token: token.clone(),
        })),
    ) {
        Ok(Some(validation)) => Ok(ApiAuth::Account {
            token,
            user_id: validation.user.id,
        }),
        Ok(None) => Err(Response::text(401, "Unauthorized", "unauthorized")),
        Err(error) => Err(daemon_error_response(error)),
    }
}

fn user_id(auth: &Option<ApiAuth>) -> Option<String> {
    match auth {
        Some(ApiAuth::Account { user_id, .. }) => Some(user_id.clone()),
        _ => None,
    }
}

fn user_auth_token(auth: &Option<ApiAuth>) -> Option<String> {
    match auth {
        Some(ApiAuth::Account { token, .. }) => Some(token.clone()),
        _ => None,
    }
}

fn reject_terminal_token_request(request: &Request, options: &GatewayOptions) -> Option<Response> {
    if !options.terminal_enabled {
        return Some(Response::text(403, "Forbidden", "terminal disabled"));
    }
    let expected = options.terminal_token.trim();
    if expected.is_empty() || request.terminal_token().as_deref() != Some(expected) {
        return Some(Response::text(403, "Forbidden", "terminal unauthorized"));
    }
    None
}

fn gateway_health(options: &GatewayOptions) -> serde_json::Value {
    json!({
        "ok": true,
        "service": "coca-gateway",
        "version": env!("CARGO_PKG_VERSION"),
        "daemon": daemon_health(options),
        "stream": StreamInfo::default(),
    })
}

fn daemon_health(options: &GatewayOptions) -> serde_json::Value {
    match daemon_ping(options) {
        Ok(result) => json!({
            "ready": true,
            "service": result.service,
            "protocol_version": result.protocol_version,
        }),
        Err(error) => json!({
            "ready": false,
            "error": daemon_unavailable_payload(error.detail()),
        }),
    }
}

fn daemon_ping(options: &GatewayOptions) -> DaemonRpcResult<DaemonPingResult> {
    daemon_rpc(options, methods::DAEMON_PING, None)
}

fn terminal_sessions(options: &GatewayOptions) -> DaemonRpcResult<TerminalListResult> {
    daemon_rpc(options, methods::TERMINAL_LIST, None)
}

type DaemonRpcResult<T> = std::result::Result<T, DaemonRpcError>;

#[derive(Debug)]
enum DaemonRpcError {
    Unavailable(anyhow::Error),
    Rpc { code: i64, message: String },
    Decode(anyhow::Error),
}

impl DaemonRpcError {
    fn rpc(error: RpcError) -> Self {
        Self::Rpc {
            code: error.code,
            message: error.message,
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::Unavailable(error) | Self::Decode(error) => format!("{error:#}"),
            Self::Rpc { code, message } => format!("daemon RPC error {code}: {message}"),
        }
    }
}

fn daemon_rpc<T>(
    options: &GatewayOptions,
    method: &'static str,
    params: Option<Value>,
) -> DaemonRpcResult<T>
where
    T: DeserializeOwned,
{
    let socket = daemon_socket_path(options).map_err(DaemonRpcError::Unavailable)?;
    daemon_rpc_from_daemon(&socket, method, params)
}

#[cfg(unix)]
fn daemon_rpc_from_daemon<T>(
    socket: &Path,
    method: &'static str,
    params: Option<Value>,
) -> DaemonRpcResult<T>
where
    T: DeserializeOwned,
{
    let request = JsonRpcRequest::new(RpcId::Number(1), method, params);
    let response = coca_ipc::unix::roundtrip(socket, &request)
        .with_context(|| format!("failed to query daemon socket {}", socket.display()))
        .map_err(DaemonRpcError::Unavailable)?;
    if let Some(error) = response.error {
        return Err(DaemonRpcError::rpc(error));
    }
    let result = response.result.unwrap_or(Value::Null);
    serde_json::from_value(result)
        .with_context(|| format!("failed to decode daemon {method} response"))
        .map_err(DaemonRpcError::Decode)
}

#[cfg(not(unix))]
fn daemon_rpc_from_daemon<T>(
    _socket: &Path,
    _method: &'static str,
    _params: Option<Value>,
) -> DaemonRpcResult<T>
where
    T: DeserializeOwned,
{
    Err(DaemonRpcError::Unavailable(anyhow::anyhow!(
        "local daemon IPC is not implemented on this platform yet"
    )))
}

fn daemon_socket_path(options: &GatewayOptions) -> Result<PathBuf> {
    if let Some(path) = &options.daemon_socket {
        return Ok(path.clone());
    }
    default_daemon_socket_path()
        .context("failed to resolve daemon socket path: home directory was not found")
}

fn default_daemon_socket_path() -> Option<PathBuf> {
    std::env::var_os("COCA_DAEMON_SOCKET")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config").join("coca").join("daemon.sock"))
        })
}

fn daemon_json_response<T>(result: DaemonRpcResult<T>) -> Response
where
    T: Serialize,
{
    match result {
        Ok(value) => json_response(value),
        Err(error) => daemon_error_response(error),
    }
}

fn daemon_error_response(error: DaemonRpcError) -> Response {
    match error {
        DaemonRpcError::Unavailable(error) => daemon_unavailable_response(error),
        DaemonRpcError::Rpc { code: 404, message } => Response::text(404, "Not Found", message),
        DaemonRpcError::Rpc {
            code: -32602,
            message,
        } => Response::text(400, "Bad Request", message),
        DaemonRpcError::Rpc { code, message } => Response::text(
            500,
            "Internal Server Error",
            format!("daemon RPC error {code}: {message}"),
        ),
        DaemonRpcError::Decode(error) => error_response(error),
    }
}

fn daemon_unavailable_response(error: anyhow::Error) -> Response {
    json_response(daemon_unavailable_payload(format!("{error:#}")))
        .with_status(503, "Service Unavailable")
}

fn daemon_unavailable_payload(detail: String) -> serde_json::Value {
    json!({
        "code": "daemon_unavailable",
        "message": "coca daemon is not available.",
        "action": "Start coca daemon and retry.",
        "detail": detail,
    })
}

fn handle_terminal_websocket(
    mut reader: BufReader<TcpStream>,
    request: Request,
    options: GatewayOptions,
) -> Result<()> {
    if let Some(response) = reject_terminal_websocket_request(&request, &options) {
        return write_response(reader.get_mut(), &response);
    }

    let Some(key) = request.header("sec-websocket-key").map(str::trim) else {
        return write_response(
            reader.get_mut(),
            &Response::text(400, "Bad Request", "missing websocket key"),
        );
    };
    let terminal_socket = terminal_socket_path(&options)?;

    #[cfg(unix)]
    {
        let daemon = match connect_terminal_daemon(&terminal_socket) {
            Ok(daemon) => daemon,
            Err(err) => {
                eprintln!("coca gateway terminal daemon unavailable: {err:#}");
                return write_response(reader.get_mut(), &daemon_unavailable_response(err));
            }
        };
        write_websocket_upgrade(reader.get_mut(), key)?;
        bridge_terminal_websocket(reader, daemon)
    }

    #[cfg(not(unix))]
    {
        write_websocket_upgrade(reader.get_mut(), key)?;
        bridge_terminal_websocket(reader, terminal_socket)
    }
}

fn reject_terminal_websocket_request(
    request: &Request,
    options: &GatewayOptions,
) -> Option<Response> {
    if request.method != "GET" {
        return Some(Response::text(
            405,
            "Method Not Allowed",
            "method not allowed",
        ));
    }
    if let Err(response) = authenticate_api_request(request, options) {
        return Some(response);
    }
    if let Some(response) = reject_terminal_token_request(request, options) {
        return Some(response);
    }
    if !header_contains_token(request.header("connection"), "upgrade")
        || !request
            .header("upgrade")
            .map(|value| value.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false)
        || request.header("sec-websocket-version") != Some("13")
    {
        return Some(Response::text(
            426,
            "Upgrade Required",
            "websocket upgrade required",
        ));
    }
    None
}

fn terminal_socket_path(options: &GatewayOptions) -> Result<PathBuf> {
    if let Some(path) = &options.terminal_socket {
        return Ok(path.clone());
    }
    if let Some(path) = &options.daemon_socket {
        return Ok(sibling_terminal_socket_path(path));
    }
    default_terminal_socket_path()
        .context("failed to resolve terminal daemon socket path: home directory was not found")
}

fn default_terminal_socket_path() -> Option<PathBuf> {
    std::env::var_os("COCA_TERMINAL_SOCKET")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(PathBuf::from).map(|home| {
                home.join(".config")
                    .join("coca")
                    .join("daemon.terminal.sock")
            })
        })
}

fn sibling_terminal_socket_path(socket: &Path) -> PathBuf {
    let file_name = socket
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("daemon.sock");
    let terminal_name = if let Some(stem) = file_name.strip_suffix(".sock") {
        format!("{stem}.terminal.sock")
    } else {
        format!("{file_name}.terminal.sock")
    };
    socket.with_file_name(terminal_name)
}

#[cfg(unix)]
fn connect_terminal_daemon(terminal_socket: &Path) -> Result<std::os::unix::net::UnixStream> {
    use std::os::unix::net::UnixStream;

    UnixStream::connect(terminal_socket).with_context(|| {
        format!(
            "failed to connect terminal daemon socket {}",
            terminal_socket.display()
        )
    })
}

#[cfg(unix)]
fn bridge_terminal_websocket(
    mut websocket_reader: BufReader<TcpStream>,
    daemon: std::os::unix::net::UnixStream,
) -> Result<()> {
    use std::net::Shutdown;

    let mut daemon_writer = daemon
        .try_clone()
        .context("failed to clone terminal daemon stream")?;
    let mut daemon_reader = daemon;
    let websocket_writer = Arc::new(Mutex::new(
        websocket_reader
            .get_ref()
            .try_clone()
            .context("failed to clone websocket stream")?,
    ));

    let daemon_to_websocket = {
        let websocket_writer = websocket_writer.clone();
        thread::spawn(move || -> Result<()> {
            loop {
                let frame: TerminalServerFrame = match coca_ipc::read_json_frame(&mut daemon_reader)
                {
                    Ok(frame) => frame,
                    Err(err) if is_clean_disconnect(&err) => return Ok(()),
                    Err(err) => return Err(err).context("failed to read terminal daemon frame"),
                };
                let payload =
                    serde_json::to_vec(&frame).context("failed to encode terminal server frame")?;
                write_ws_frame_locked(&websocket_writer, WebSocketOpcode::Text, &payload)
                    .context("failed to write websocket terminal frame")?;
            }
        })
    };

    let result = loop {
        let frame = match read_ws_frame(&mut websocket_reader) {
            Ok(frame) => frame,
            Err(err) if is_clean_disconnect(&err) => break Ok(()),
            Err(err) => {
                let _ = write_ws_close_locked(&websocket_writer, 1002, "protocol error");
                break Err(err).context("failed to read websocket terminal frame");
            }
        };
        match frame.opcode {
            WebSocketOpcode::Text | WebSocketOpcode::Binary => {
                let terminal_frame: TerminalClientFrame =
                    match serde_json::from_slice(&frame.payload) {
                        Ok(frame) => frame,
                        Err(_) => {
                            write_ws_error(
                                &websocket_writer,
                                "invalid_json",
                                "terminal websocket frame was not valid terminal JSON",
                            )?;
                            continue;
                        }
                    };
                coca_ipc::write_json_frame(&mut daemon_writer, &terminal_frame)
                    .context("failed to write terminal daemon frame")?;
            }
            WebSocketOpcode::Close => {
                let _ = write_ws_frame_locked(&websocket_writer, WebSocketOpcode::Close, &[]);
                break Ok(());
            }
            WebSocketOpcode::Ping => {
                write_ws_frame_locked(&websocket_writer, WebSocketOpcode::Pong, &frame.payload)
                    .context("failed to write websocket pong")?;
            }
            WebSocketOpcode::Pong => {}
        }
    };

    let _ = daemon_writer.shutdown(Shutdown::Both);
    let _ = daemon_to_websocket.join();
    result
}

#[cfg(not(unix))]
fn bridge_terminal_websocket(
    mut websocket_reader: BufReader<TcpStream>,
    _terminal_socket: PathBuf,
) -> Result<()> {
    write_ws_frame(
        websocket_reader.get_mut(),
        WebSocketOpcode::Text,
        &serde_json::to_vec(&TerminalServerFrame::Error(TerminalError {
            request_id: None,
            terminal_id: None,
            code: "unsupported_platform".to_string(),
            message: "local daemon terminal IPC is not implemented on this platform yet"
                .to_string(),
            action: Some("Use a platform with local daemon terminal IPC support.".to_string()),
            detail: None,
        }))?,
    )?;
    write_ws_frame(websocket_reader.get_mut(), WebSocketOpcode::Close, &[])
}

fn write_ws_error(
    websocket_writer: &Arc<Mutex<TcpStream>>,
    code: &str,
    message: &str,
) -> Result<()> {
    let frame = TerminalServerFrame::Error(TerminalError {
        request_id: None,
        terminal_id: None,
        code: code.to_string(),
        message: message.to_string(),
        action: Some("Send a valid terminal WebSocket frame and retry.".to_string()),
        detail: None,
    });
    let payload = serde_json::to_vec(&frame).context("failed to encode terminal error frame")?;
    write_ws_frame_locked(websocket_writer, WebSocketOpcode::Text, &payload)
}

fn write_websocket_upgrade(writer: &mut TcpStream, key: &str) -> Result<()> {
    write!(
        writer,
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         Cache-Control: no-store\r\n\
         \r\n",
        websocket_accept_key(key)
    )
    .context("failed to write websocket upgrade response")?;
    writer
        .flush()
        .context("failed to flush websocket upgrade response")
}

fn websocket_accept_key(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WEBSOCKET_GUID.as_bytes());
    BASE64_STANDARD.encode(hasher.finalize())
}

fn header_contains_token(value: Option<&str>, expected: &str) -> bool {
    value
        .unwrap_or_default()
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case(expected))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebSocketOpcode {
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl WebSocketOpcode {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x1 => Some(Self::Text),
            0x2 => Some(Self::Binary),
            0x8 => Some(Self::Close),
            0x9 => Some(Self::Ping),
            0xA => Some(Self::Pong),
            _ => None,
        }
    }

    fn byte(self) -> u8 {
        match self {
            Self::Text => 0x1,
            Self::Binary => 0x2,
            Self::Close => 0x8,
            Self::Ping => 0x9,
            Self::Pong => 0xA,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct WebSocketFrame {
    opcode: WebSocketOpcode,
    payload: Vec<u8>,
}

fn read_ws_frame(reader: &mut impl Read) -> Result<WebSocketFrame> {
    let mut header = [0; 2];
    reader
        .read_exact(&mut header)
        .context("failed to read websocket frame header")?;
    let fin = header[0] & 0x80 != 0;
    if !fin {
        anyhow::bail!("fragmented websocket messages are not supported");
    }
    let opcode =
        WebSocketOpcode::from_byte(header[0] & 0x0F).context("unsupported websocket opcode")?;
    let masked = header[1] & 0x80 != 0;
    if !masked {
        anyhow::bail!("client websocket frames must be masked");
    }

    let mut len = u64::from(header[1] & 0x7F);
    if len == 126 {
        let mut extended = [0; 2];
        reader
            .read_exact(&mut extended)
            .context("failed to read websocket extended payload length")?;
        len = u64::from(u16::from_be_bytes(extended));
    } else if len == 127 {
        let mut extended = [0; 8];
        reader
            .read_exact(&mut extended)
            .context("failed to read websocket extended payload length")?;
        len = u64::from_be_bytes(extended);
    }
    if len > MAX_WEBSOCKET_PAYLOAD_LEN {
        anyhow::bail!("websocket frame exceeded maximum size");
    }

    let mut mask = [0; 4];
    reader
        .read_exact(&mut mask)
        .context("failed to read websocket mask")?;
    let mut payload = vec![0; len as usize];
    reader
        .read_exact(&mut payload)
        .context("failed to read websocket payload")?;
    for (idx, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[idx % 4];
    }

    Ok(WebSocketFrame { opcode, payload })
}

fn write_ws_frame_locked(
    writer: &Arc<Mutex<TcpStream>>,
    opcode: WebSocketOpcode,
    payload: &[u8],
) -> Result<()> {
    let mut writer = writer.lock().expect("websocket writer mutex poisoned");
    write_ws_frame(&mut *writer, opcode, payload)
}

fn write_ws_close_locked(writer: &Arc<Mutex<TcpStream>>, code: u16, reason: &str) -> Result<()> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(reason.as_bytes());
    write_ws_frame_locked(writer, WebSocketOpcode::Close, &payload)
}

fn write_ws_frame(writer: &mut impl Write, opcode: WebSocketOpcode, payload: &[u8]) -> Result<()> {
    if payload.len() as u64 > MAX_WEBSOCKET_PAYLOAD_LEN {
        anyhow::bail!("websocket frame exceeded maximum size");
    }
    writer
        .write_all(&[0x80 | opcode.byte()])
        .context("failed to write websocket frame header")?;
    match payload.len() {
        len @ 0..=125 => writer.write_all(&[len as u8])?,
        len @ 126..=65535 => {
            writer.write_all(&[126])?;
            writer.write_all(&(len as u16).to_be_bytes())?;
        }
        len => {
            writer.write_all(&[127])?;
            writer.write_all(&(len as u64).to_be_bytes())?;
        }
    }
    writer
        .write_all(payload)
        .context("failed to write websocket frame payload")?;
    writer.flush().context("failed to flush websocket frame")
}

fn is_clean_disconnect(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|err| {
                matches!(
                    err.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                )
            })
            .unwrap_or(false)
    })
}

fn session_ref_from_query(request: &Request) -> Option<SessionRef> {
    Some(SessionRef {
        origin: percent_decode(&request.query_param("origin")?)?,
        provider: percent_decode(&request.query_param("provider")?)?,
        id: percent_decode(&request.query_param("id")?)?,
    })
}

#[derive(Debug, Deserialize)]
struct ShareSessionRequest {
    session: SessionRef,
}

#[derive(Debug, Deserialize)]
struct AccountProfileBody {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccountPasswordBody {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Deserialize)]
struct DeviceRevokeBody {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct AccessTokenCreateBody {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AccessTokenRevokeBody {
    token_id: String,
}

#[derive(Debug, Eq, PartialEq)]
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn path(&self) -> &str {
        self.target
            .split_once('?')
            .map_or(self.target.as_str(), |(path, _)| path)
    }

    fn query_param(&self, name: &str) -> Option<String> {
        let (_, query) = self.target.split_once('?')?;
        query
            .split('&')
            .filter_map(|part| part.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    }

    fn decoded_query_param(&self, name: &str) -> Option<String> {
        self.query_param(name)
            .and_then(|value| percent_decode(&value))
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn bearer_token(&self) -> Option<String> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .and_then(|(_, value)| value.strip_prefix("Bearer "))
            .map(str::trim)
            .map(str::to_string)
    }

    fn read_token(&self) -> Option<String> {
        self.bearer_token()
            .or_else(|| self.decoded_query_param("token"))
    }

    fn terminal_token(&self) -> Option<String> {
        self.header("x-coca-terminal-token")
            .map(str::trim)
            .map(str::to_string)
            .or_else(|| self.decoded_query_param("terminal_token"))
    }

    fn content_length(&self) -> Option<usize> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.parse().ok())
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn text(status: u16, reason: &'static str, body: impl Into<String>) -> Self {
        Self {
            status,
            reason,
            content_type: "text/plain; charset=utf-8",
            body: body.into().into_bytes(),
        }
    }

    fn with_status(mut self, status: u16, reason: &'static str) -> Self {
        self.status = status;
        self.reason = reason;
        self
    }
}

fn json_response(value: impl serde::Serialize) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(err) => Response::text(500, "Internal Server Error", format!("{err:#}")),
    }
}

fn error_response(error: anyhow::Error) -> Response {
    Response::text(500, "Internal Server Error", format!("{error:#}"))
}

fn read_request(reader: &mut impl BufRead) -> Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let Some(method) = parts.next().map(str::to_string) else {
        return Ok(None);
    };
    let Some(target) = parts.next().map(str::to_string) else {
        return Ok(None);
    };

    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let mut request = Request {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if let Some(length) = request.content_length() {
        request.body.resize(length, 0);
        reader.read_exact(&mut request.body)?;
    }

    Ok(Some(request))
}

fn write_response(writer: &mut impl Write, response: &Response) -> Result<()> {
    write!(
        writer,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len(),
    )
    .context("failed to write response headers")?;
    writer
        .write_all(&response.body)
        .context("failed to write response body")
}

fn request_safe_response(response: &Response) -> Response {
    Response {
        status: response.status,
        reason: response.reason,
        content_type: response.content_type,
        body: response.body.clone(),
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'%' => {
                let hi = *bytes.get(idx + 1)?;
                let lo = *bytes.get(idx + 2)?;
                decoded.push(hex_value(hi)? * 16 + hex_value(lo)?);
                idx += 3;
            }
            b'+' => {
                decoded.push(b' ');
                idx += 1;
            }
            byte => {
                decoded.push(byte);
                idx += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coca_protocol::{
        JsonRpcResponse, SessionRef as WireSessionRef, TerminalClientFrame, TerminalModeWire,
        TerminalOpen, TerminalOpened, TerminalServerFrame, TerminalSessionSummary, TerminalSize,
        TerminalStateWire,
    };

    #[test]
    fn api_requires_token() {
        let options = gateway_options();
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/health".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);

        assert_eq!(response.status, 401);
    }

    #[test]
    fn health_reports_stream_protocol() {
        let options = gateway_options();
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/health?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("\"service\":\"coca-gateway\""));
        assert!(body.contains("\"ready\":false"));
        assert!(body.contains("\"code\":\"daemon_unavailable\""));
        assert!(body.contains("terminal.open"));
        assert!(body.contains("terminal.output"));
    }

    #[test]
    fn api_sessions_returns_structured_daemon_unavailable() {
        let options = gateway_options();
        let request = Request {
            method: "GET".to_string(),
            target: "/api/sessions?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 503);
        assert!(body.contains("\"code\":\"daemon_unavailable\""));
        assert!(body.contains("Start coca daemon and retry."));
    }

    #[cfg(unix)]
    #[test]
    fn api_sessions_compatibility_is_loaded_from_daemon_rpc() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::SESSIONS_LIST);
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "sessions": [
                        {
                            "origin": "Local",
                            "provider": "Codex",
                            "id": "sid",
                            "title": "title",
                            "cwd": "/tmp",
                            "created_at_ms": null,
                            "updated_at_ms": null,
                            "model": null,
                            "source_path": "/tmp/session",
                            "first_user_message": null,
                            "transcript": [],
                            "resume_program": "codex",
                            "resume_args": ["resume", "sid"]
                        }
                    ],
                    "warnings": []
                }),
            )
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/sessions?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.starts_with("["));
        assert!(body.contains("\"sid\""));
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn public_auth_routes_do_not_require_api_token() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::AUTH_CAPABILITIES);
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "email_password": {
                        "available": true,
                        "configured": true,
                        "reason": null
                    },
                    "signup_enabled": true,
                    "signup_requires_bootstrap_token": true,
                    "sso": [{
                        "provider": "oidc",
                        "available": false,
                        "configured": false,
                        "reason": "unconfigured"
                    }]
                }),
            )
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/auth/capabilities".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("\"signup_enabled\":true"));
        assert!(body.contains("\"available\":false"));
        daemon_handle.join().unwrap();
    }

    #[test]
    fn malformed_auth_json_returns_400_without_secret_leakage() {
        let options = gateway_options();
        let request = json_request(
            "POST",
            "/api/v1/auth/login",
            r#"{ "email": "user@example.com", "password": "secret" "#,
        );

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 400);
        assert!(!body.contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn auth_login_forwards_to_daemon_without_api_token() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::AUTH_LOGIN);
            let params: AuthLoginParams = serde_json::from_value(request.params.unwrap()).unwrap();
            assert_eq!(params.email, "user@example.com");
            assert_eq!(params.password, "password");
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "user": account_user_json(),
                    "session": device_session_json(),
                    "session_token": "coca_sess_plaintext"
                }),
            )
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        let request = json_request(
            "POST",
            "/api/v1/auth/login",
            r#"{ "email": "user@example.com", "password": "password", "device_label": "Browser" }"#,
        );

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("coca_sess_plaintext"));
        assert!(!body.contains("password"));
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn account_routes_validate_auth_token_then_forward_user_id() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_n(2, |idx, request| match idx {
            0 => {
                assert_eq!(request.method, methods::AUTH_VALIDATE);
                let params: AuthValidateParams =
                    serde_json::from_value(request.params.unwrap()).unwrap();
                assert_eq!(params.token, "session-secret");
                JsonRpcResponse::success(request.id, auth_validation_json())
            }
            1 => {
                assert_eq!(request.method, methods::ACCOUNT_ME);
                let params: AccountSubjectParams =
                    serde_json::from_value(request.params.unwrap()).unwrap();
                assert_eq!(params.user_id, "usr_1");
                JsonRpcResponse::success(
                    request.id,
                    serde_json::json!({ "user": account_user_json() }),
                )
            }
            _ => unreachable!(),
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/account/me".to_string(),
            headers: vec![(
                "authorization".to_string(),
                "Bearer session-secret".to_string(),
            )],
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("user@example.com"));
        assert!(!body.contains("session-secret"));
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn revoked_auth_token_returns_401() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::AUTH_VALIDATE);
            JsonRpcResponse::success(request.id, serde_json::Value::Null)
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/account/me".to_string(),
            headers: vec![(
                "authorization".to_string(),
                "Bearer revoked-secret".to_string(),
            )],
            body: Vec::new(),
        };

        let response = route_request(&request, &options);

        assert_eq!(response.status, 401);
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn existing_api_routes_accept_daemon_auth_tokens() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_n(2, |idx, request| match idx {
            0 => {
                assert_eq!(request.method, methods::AUTH_VALIDATE);
                JsonRpcResponse::success(request.id, auth_validation_json())
            }
            1 => {
                assert_eq!(request.method, methods::SESSIONS_SUMMARIES);
                JsonRpcResponse::success(
                    request.id,
                    serde_json::json!({
                        "sessions": [],
                        "warnings": [],
                        "counts": {
                            "total": 0,
                            "by_provider": {},
                            "by_origin": {}
                        }
                    }),
                )
            }
            _ => unreachable!(),
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/sessions".to_string(),
            headers: vec![(
                "authorization".to_string(),
                "Bearer session-secret".to_string(),
            )],
            body: Vec::new(),
        };

        let response = route_request(&request, &options);

        assert_eq!(response.status, 200);
        daemon_handle.join().unwrap();
    }

    #[test]
    fn websocket_accept_key_matches_rfc_example() {
        assert_eq!(
            websocket_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn terminal_websocket_requires_read_and_terminal_tokens() {
        let mut options = gateway_options();
        options.terminal_enabled = true;
        options.terminal_token = "terminal-secret".to_string();

        let missing_terminal = terminal_upgrade_request(
            "/api/v1/terminal/ws?token=secret",
            "dGhlIHNhbXBsZSBub25jZQ==",
        );
        let bad_read = terminal_upgrade_request(
            "/api/v1/terminal/ws?token=bad&terminal_token=terminal-secret",
            "dGhlIHNhbXBsZSBub25jZQ==",
        );
        let accepted = terminal_upgrade_request(
            "/api/v1/terminal/ws?token=secret&terminal_token=terminal-secret",
            "dGhlIHNhbXBsZSBub25jZQ==",
        );

        assert_eq!(
            reject_terminal_websocket_request(&missing_terminal, &options)
                .unwrap()
                .status,
            403
        );
        assert_eq!(
            reject_terminal_websocket_request(&bad_read, &options)
                .unwrap()
                .status,
            503
        );
        assert!(reject_terminal_websocket_request(&accepted, &options).is_none());
    }

    #[test]
    fn terminal_sessions_requires_read_and_terminal_tokens() {
        let mut options = gateway_options();
        options.terminal_enabled = true;
        options.terminal_token = "terminal-secret".to_string();

        let missing_read = Request {
            method: "GET".to_string(),
            target: "/api/v1/terminal/sessions".to_string(),
            headers: vec![(
                "x-coca-terminal-token".to_string(),
                "terminal-secret".to_string(),
            )],
            body: Vec::new(),
        };
        let missing_terminal = Request {
            method: "GET".to_string(),
            target: "/api/v1/terminal/sessions?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        assert_eq!(route_request(&missing_read, &options).status, 401);
        assert_eq!(route_request(&missing_terminal, &options).status, 403);
    }

    #[test]
    fn terminal_sessions_returns_structured_daemon_unavailable() {
        let mut options = gateway_options();
        options.terminal_enabled = true;
        options.terminal_token = "terminal-secret".to_string();
        options.daemon_socket = Some(PathBuf::from("__missing_daemon.sock"));
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/terminal/sessions?token=secret".to_string(),
            headers: vec![(
                "x-coca-terminal-token".to_string(),
                "terminal-secret".to_string(),
            )],
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 503);
        assert!(body.contains("\"code\":\"daemon_unavailable\""));
        assert!(body.contains("Start coca daemon and retry."));
    }

    #[cfg(unix)]
    #[test]
    fn config_summary_does_not_expose_terminal_tokens() {
        let mut options = gateway_options();
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::SETTINGS_SUMMARY);
            let params: SettingsSummaryParams =
                serde_json::from_value(request.params.unwrap()).unwrap();
            assert_eq!(params.gateway_bind, "127.0.0.1:0");
            assert!(!params.terminal_socket_available);
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "terminal": {
                        "enabled": true,
                        "token_configured": true,
                        "daemon_available": true,
                        "terminal_socket_available": false,
                        "unavailable_code": "terminal_socket_unavailable",
                        "unavailable_message": "coca daemon terminal socket is not available."
                    },
                    "remotes": [{
                        "name": "remote-a",
                        "base_url": "https://remote.example",
                        "enabled": true,
                        "visible": true,
                        "token_configured": true,
                        "terminal_token_configured": true,
                        "terminal_ready": true,
                        "terminal_unavailable_code": null,
                        "terminal_unavailable_message": null,
                        "session_count": 0
                    }]
                }),
            )
        });
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/config/summary?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("\"token_configured\":true"));
        assert!(body.contains("\"terminal_token_configured\":true"));
        assert!(!body.contains("terminal-secret"));
        assert!(!body.contains("remote-terminal-secret"));
        assert!(!body.contains("read-secret"));
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sessions_response_does_not_expose_remote_terminal_tokens() {
        let mut options = gateway_options();
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::SESSIONS_SUMMARIES);
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "sessions": [],
                    "warnings": [],
                    "counts": {
                        "total": 0,
                        "by_provider": {},
                        "by_origin": {}
                    }
                }),
            )
        });
        options.daemon_socket = Some(daemon_socket);
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/sessions?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(!body.contains("remote-terminal-secret"));
        assert!(!body.contains("read-secret"));
        daemon_handle.join().unwrap();
    }

    #[test]
    fn put_ai_config_requires_authentication() {
        let options = gateway_options();
        let request = Request {
            method: "PUT".to_string(),
            target: "/api/v1/config/ai".to_string(),
            headers: vec![("content-length".to_string(), "2".to_string())],
            body: b"{}".to_vec(),
        };

        let response = route_request(&request, &options);

        assert_eq!(response.status, 401);
    }

    #[test]
    fn put_ai_config_updates_without_echoing_or_overwriting_blank_api_key() {
        let mut options = gateway_options();
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::SETTINGS_AI_UPDATE);
            let params: AiSettingsUpdateParams =
                serde_json::from_value(request.params.unwrap()).unwrap();
            assert_eq!(
                params.base_url.as_deref(),
                Some(" https://example.test/v1 ")
            );
            assert_eq!(params.model.as_deref(), Some(" test-model "));
            assert_eq!(params.api_key.as_deref(), Some("   "));
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "model": "test-model",
                    "enabled": true,
                    "provider": "openai",
                    "api_key_env": "OPENAI_API_KEY",
                    "api_key_configured": true,
                    "key_source": "stored"
                }),
            )
        });
        options.daemon_socket = Some(daemon_socket);
        let request = json_request(
            "PUT",
            "/api/v1/config/ai?token=secret",
            r#"{
                "base_url": " https://example.test/v1 ",
                "model": " test-model ",
                "api_key": "   "
            }"#,
        );

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("https://example.test/v1"));
        assert!(body.contains("test-model"));
        assert!(body.contains("\"api_key_configured\":true"));
        assert!(!body.contains("sk-existing"));
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn put_ai_config_clear_flag_clears_api_key() {
        let mut options = gateway_options();
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_once(|request| {
            assert_eq!(request.method, methods::SETTINGS_AI_UPDATE);
            let params: AiSettingsUpdateParams =
                serde_json::from_value(request.params.unwrap()).unwrap();
            assert!(params.clear_api_key);
            assert_eq!(params.api_key.as_deref(), Some("sk-new"));
            JsonRpcResponse::success(
                request.id,
                serde_json::json!({
                    "base_url": "https://api.openai.com/v1",
                    "model": "gpt-4.1",
                    "enabled": true,
                    "provider": "openai",
                    "api_key_env": "OPENAI_API_KEY",
                    "api_key_configured": false,
                    "key_source": "none"
                }),
            )
        });
        options.daemon_socket = Some(daemon_socket);
        let request = json_request(
            "PUT",
            "/api/v1/config/ai?token=secret",
            r#"{ "clear_api_key": true, "api_key": "sk-new" }"#,
        );

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("\"api_key_configured\":false"));
        assert!(!body.contains("sk-existing"));
        assert!(!body.contains("sk-new"));
        daemon_handle.join().unwrap();
    }

    #[test]
    fn static_routes_do_not_render_rust_html_when_assets_are_missing() {
        let options = gateway_options();
        let request = Request {
            method: "GET".to_string(),
            target: "/sessions".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);

        assert_eq!(response.status, 404);
        assert!(String::from_utf8(response.body)
            .unwrap()
            .contains("React web assets"));
    }

    #[test]
    fn startup_info_uses_loopback_url_for_unspecified_bind() {
        let options = gateway_options();

        let info = startup_info(&options, SocketAddr::from(([0, 0, 0, 0], 8787)));

        assert_eq!(
            info,
            GatewayStartupInfo {
                listen_addr: "0.0.0.0:8787".to_string(),
                browser_url: "http://127.0.0.1:8787/?token=secret".to_string(),
                share_base_url: "http://127.0.0.1:8787".to_string(),
                token: "secret".to_string(),
            }
        );
    }

    #[test]
    fn startup_info_formats_ipv6_and_encodes_token() {
        let mut options = gateway_options();
        options.read_token = " secret/with space? ".to_string();

        let info = startup_info(&options, "[::1]:8787".parse().unwrap());

        assert_eq!(info.listen_addr, "[::1]:8787");
        assert_eq!(
            info.browser_url,
            "http://[::1]:8787/?token=secret%2Fwith%20space%3F"
        );
        assert_eq!(info.token, "secret/with space?");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_websocket_forwards_json_frames_to_daemon_stream() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let terminal_socket = dir.path().join("terminal.sock");
        let listener = UnixListener::bind(&terminal_socket).unwrap();
        let daemon_handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let frame: TerminalClientFrame = coca_ipc::read_json_frame(&mut stream).unwrap();
            assert_eq!(
                frame,
                TerminalClientFrame::Open(TerminalOpen {
                    session: wire_session_ref(),
                    mode: TerminalModeWire::Resume,
                    size: TerminalSize { cols: 80, rows: 24 },
                })
            );
            coca_ipc::write_json_frame(
                &mut stream,
                &TerminalServerFrame::Opened(TerminalOpened {
                    terminal: terminal_summary(),
                }),
            )
            .unwrap();
        });

        let mut options = gateway_options();
        options.terminal_enabled = true;
        options.terminal_token = "terminal-secret".to_string();
        options.terminal_socket = Some(terminal_socket);

        let web_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let web_addr = web_listener.local_addr().unwrap();
        let web_handle = std::thread::spawn(move || {
            let (stream, _) = web_listener.accept().unwrap();
            handle_stream(stream, options).unwrap();
        });

        let mut client = TcpStream::connect(web_addr).unwrap();
        write!(
            client,
            "GET /api/v1/terminal/ws?token=secret&terminal_token=terminal-secret HTTP/1.1\r\n\
             Host: 127.0.0.1\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n"
        )
        .unwrap();
        let mut client_reader = BufReader::new(client.try_clone().unwrap());
        let mut status = String::new();
        client_reader.read_line(&mut status).unwrap();
        assert_eq!(status, "HTTP/1.1 101 Switching Protocols\r\n");
        loop {
            let mut line = String::new();
            client_reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }

        let open = TerminalClientFrame::Open(TerminalOpen {
            session: wire_session_ref(),
            mode: TerminalModeWire::Resume,
            size: TerminalSize { cols: 80, rows: 24 },
        });
        write_client_ws_frame(&mut client, &serde_json::to_vec(&open).unwrap());
        let response = read_server_ws_frame(&mut client_reader);
        let server_frame: TerminalServerFrame = serde_json::from_slice(&response).unwrap();
        assert_eq!(
            server_frame,
            TerminalServerFrame::Opened(TerminalOpened {
                terminal: terminal_summary(),
            })
        );
        write_client_ws_close(&mut client);

        web_handle.join().unwrap();
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn terminal_sessions_are_loaded_from_daemon_rpc() {
        let dir = tempfile::tempdir().unwrap();
        let daemon_socket = dir.path().join("daemon.sock");
        let server_socket = daemon_socket.clone();
        let daemon_handle = std::thread::spawn(move || {
            coca_ipc::unix::serve_one(&server_socket, |request| {
                assert_eq!(request.method, methods::TERMINAL_LIST);
                JsonRpcResponse::success(
                    request.id,
                    serde_json::to_value(TerminalListResult {
                        terminals: vec![terminal_summary()],
                    })
                    .unwrap(),
                )
            })
            .unwrap();
        });
        wait_for_path(&daemon_socket);

        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        options.terminal_enabled = true;
        options.terminal_token = "terminal-secret".to_string();
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/terminal/sessions?token=secret".to_string(),
            headers: vec![(
                "x-coca-terminal-token".to_string(),
                "terminal-secret".to_string(),
            )],
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("\"terminals\""));
        assert!(body.contains("\"term-1\""));
        daemon_handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn terminal_sessions_accept_auth_token_but_still_require_terminal_token() {
        let (_dir, daemon_socket, daemon_handle) = spawn_daemon_n(3, |idx, request| match idx {
            0 => {
                assert_eq!(request.method, methods::AUTH_VALIDATE);
                JsonRpcResponse::success(request.id, auth_validation_json())
            }
            1 => {
                assert_eq!(request.method, methods::AUTH_VALIDATE);
                JsonRpcResponse::success(request.id, auth_validation_json())
            }
            2 => {
                assert_eq!(request.method, methods::TERMINAL_LIST);
                JsonRpcResponse::success(
                    request.id,
                    serde_json::to_value(TerminalListResult {
                        terminals: vec![terminal_summary()],
                    })
                    .unwrap(),
                )
            }
            _ => unreachable!(),
        });
        let mut options = gateway_options();
        options.daemon_socket = Some(daemon_socket);
        options.terminal_enabled = true;
        options.terminal_token = "terminal-secret".to_string();
        let missing_terminal = Request {
            method: "GET".to_string(),
            target: "/api/v1/terminal/sessions".to_string(),
            headers: vec![(
                "authorization".to_string(),
                "Bearer session-secret".to_string(),
            )],
            body: Vec::new(),
        };
        let accepted = Request {
            method: "GET".to_string(),
            target: "/api/v1/terminal/sessions".to_string(),
            headers: vec![
                (
                    "authorization".to_string(),
                    "Bearer session-secret".to_string(),
                ),
                (
                    "x-coca-terminal-token".to_string(),
                    "terminal-secret".to_string(),
                ),
            ],
            body: Vec::new(),
        };

        assert_eq!(route_request(&missing_terminal, &options).status, 403);
        assert_eq!(route_request(&accepted, &options).status, 200);
        daemon_handle.join().unwrap();
    }

    fn json_request(method: &str, target: &str, body: &str) -> Request {
        Request {
            method: method.to_string(),
            target: target.to_string(),
            headers: vec![
                ("content-type".to_string(), "application/json".to_string()),
                ("content-length".to_string(), body.len().to_string()),
            ],
            body: body.as_bytes().to_vec(),
        }
    }

    fn gateway_options() -> GatewayOptions {
        GatewayOptions {
            bind: "127.0.0.1:0".to_string(),
            read_token: "secret".to_string(),
            share_base_url: "http://127.0.0.1:8787".to_string(),
            terminal_enabled: false,
            terminal_token: String::new(),
            static_dir: PathBuf::from("__missing_static_dir__"),
            daemon_socket: Some(PathBuf::from("__missing_daemon.sock")),
            terminal_socket: None,
        }
    }

    #[cfg(unix)]
    fn spawn_daemon_once<F>(handler: F) -> (tempfile::TempDir, PathBuf, std::thread::JoinHandle<()>)
    where
        F: FnMut(JsonRpcRequest) -> JsonRpcResponse + Send + 'static,
    {
        let dir = tempfile::tempdir().unwrap();
        let daemon_socket = dir.path().join("daemon.sock");
        let server_socket = daemon_socket.clone();
        let daemon_handle = std::thread::spawn(move || {
            coca_ipc::unix::serve_one(&server_socket, handler).unwrap();
        });
        wait_for_path(&daemon_socket);
        (dir, daemon_socket, daemon_handle)
    }

    #[cfg(unix)]
    fn spawn_daemon_n<F>(
        count: usize,
        mut handler: F,
    ) -> (tempfile::TempDir, PathBuf, std::thread::JoinHandle<()>)
    where
        F: FnMut(usize, JsonRpcRequest) -> JsonRpcResponse + Send + 'static,
    {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let daemon_socket = dir.path().join("daemon.sock");
        let server_socket = daemon_socket.clone();
        let daemon_handle = std::thread::spawn(move || {
            let listener = UnixListener::bind(&server_socket).unwrap();
            for idx in 0..count {
                let (mut stream, _) = listener.accept().unwrap();
                let request: JsonRpcRequest = coca_ipc::read_json_frame(&mut stream).unwrap();
                let response = handler(idx, request);
                coca_ipc::write_json_frame(&mut stream, &response).unwrap();
            }
        });
        wait_for_path(&daemon_socket);
        (dir, daemon_socket, daemon_handle)
    }

    fn account_user_json() -> serde_json::Value {
        serde_json::json!({
            "id": "usr_1",
            "email": "user@example.com",
            "display_name": "User",
            "created_at_ms": 1,
            "updated_at_ms": 1
        })
    }

    fn device_session_json() -> serde_json::Value {
        serde_json::json!({
            "id": "dev_1",
            "label": "Browser",
            "created_at_ms": 1,
            "last_seen_at_ms": 1,
            "revoked_at_ms": null
        })
    }

    fn auth_validation_json() -> serde_json::Value {
        serde_json::json!({
            "user": account_user_json(),
            "credential_id": "dev_1",
            "credential_kind": "DeviceSession"
        })
    }

    fn terminal_upgrade_request(target: &str, key: &str) -> Request {
        Request {
            method: "GET".to_string(),
            target: target.to_string(),
            headers: vec![
                ("upgrade".to_string(), "websocket".to_string()),
                ("connection".to_string(), "keep-alive, Upgrade".to_string()),
                ("sec-websocket-key".to_string(), key.to_string()),
                ("sec-websocket-version".to_string(), "13".to_string()),
            ],
            body: Vec::new(),
        }
    }

    fn wire_session_ref() -> WireSessionRef {
        WireSessionRef {
            origin: "local".to_string(),
            provider: "codex".to_string(),
            id: "sid".to_string(),
        }
    }

    fn terminal_summary() -> TerminalSessionSummary {
        TerminalSessionSummary {
            terminal_id: coca_protocol::TerminalId("term-1".to_string()),
            session: wire_session_ref(),
            mode: TerminalModeWire::Resume,
            state: TerminalStateWire::Running,
            attached_clients: 1,
            active_writer: Some("client-1".to_string()),
            last_seq: coca_protocol::TerminalSeq(0),
            size: TerminalSize { cols: 80, rows: 24 },
            exit: None,
        }
    }

    #[cfg(unix)]
    fn write_client_ws_frame(writer: &mut TcpStream, payload: &[u8]) {
        writer.write_all(&[0x81]).unwrap();
        match payload.len() {
            len @ 0..=125 => writer.write_all(&[0x80 | len as u8]).unwrap(),
            len @ 126..=65535 => {
                writer.write_all(&[0x80 | 126]).unwrap();
                writer.write_all(&(len as u16).to_be_bytes()).unwrap();
            }
            len => {
                writer.write_all(&[0x80 | 127]).unwrap();
                writer.write_all(&(len as u64).to_be_bytes()).unwrap();
            }
        }
        let mask = [1_u8, 2, 3, 4];
        writer.write_all(&mask).unwrap();
        for (idx, byte) in payload.iter().enumerate() {
            writer.write_all(&[*byte ^ mask[idx % 4]]).unwrap();
        }
        writer.flush().unwrap();
    }

    #[cfg(unix)]
    fn write_client_ws_close(writer: &mut TcpStream) {
        writer.write_all(&[0x88, 0x80, 1, 2, 3, 4]).unwrap();
        writer.flush().unwrap();
    }

    #[cfg(unix)]
    fn read_server_ws_frame(reader: &mut impl Read) -> Vec<u8> {
        let mut header = [0; 2];
        reader.read_exact(&mut header).unwrap();
        assert_eq!(header[0] & 0x0F, 0x1);
        assert_eq!(header[1] & 0x80, 0);
        let mut len = usize::from(header[1] & 0x7F);
        if len == 126 {
            let mut extended = [0; 2];
            reader.read_exact(&mut extended).unwrap();
            len = usize::from(u16::from_be_bytes(extended));
        }
        let mut payload = vec![0; len];
        reader.read_exact(&mut payload).unwrap();
        payload
    }

    #[cfg(unix)]
    fn wait_for_path(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("timed out waiting for {}", path.display());
    }
}
