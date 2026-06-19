use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::{ProviderFilter, Session, SessionOrigin};
use crate::providers;

const JSONRPC_VERSION: &str = "2.0";
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const UNAUTHENTICATED: i64 = -32001;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RemoteConfig {
    pub remotes: Vec<RemoteEndpoint>,
}

impl RemoteConfig {
    pub fn empty() -> Self {
        Self {
            remotes: Vec::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for remote in &self.remotes {
            let name = remote.name.trim();
            if name.is_empty() {
                anyhow::bail!("remote name must not be empty");
            }
            if !names.insert(name.to_string()) {
                anyhow::bail!("duplicate remote name: {name}");
            }
            if remote.addr.trim().is_empty() {
                anyhow::bail!("remote {name} addr must not be empty");
            }
            if remote.token.trim().is_empty() {
                anyhow::bail!("remote {name} token must not be empty");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RemoteEndpoint {
    pub name: String,
    pub addr: String,
    pub token: String,
}

#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub bind: String,
    pub token: String,
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: Option<String>,
    method: Option<String>,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct HelloParams {
    token: String,
}

struct RpcContext<'a> {
    token: &'a str,
    codex_home: Option<&'a Path>,
    claude_home: Option<&'a Path>,
    provider_filter: ProviderFilter,
}

pub fn default_remote_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join("coca").join("remotes.json"))
}

pub fn load_remote_config_for_cli(path: Option<&Path>) -> Result<RemoteConfig> {
    if let Some(path) = path {
        return load_remote_config(path);
    }

    let Some(path) = default_remote_config_path() else {
        return Ok(RemoteConfig::empty());
    };
    if path.exists() {
        load_remote_config(&path)
    } else {
        Ok(RemoteConfig::empty())
    }
}

pub fn load_remote_config(path: &Path) -> Result<RemoteConfig> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read remote config {}", path.display()))?;
    let config: RemoteConfig = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse remote config {}", path.display()))?;
    config.validate()?;
    Ok(config)
}

pub fn load_remote_sessions(config: &RemoteConfig) -> (Vec<Session>, Vec<String>) {
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();

    for remote in &config.remotes {
        match fetch_remote_sessions(remote) {
            Ok(mut remote_sessions) => sessions.append(&mut remote_sessions),
            Err(err) => warnings.push(format!("{}: {err:#}", remote.name)),
        }
    }

    (sessions, warnings)
}

pub fn fetch_remote_sessions(remote: &RemoteEndpoint) -> Result<Vec<Session>> {
    let mut stream = TcpStream::connect(remote.addr.trim())
        .with_context(|| format!("failed to connect to remote {}", remote.name))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .context("failed to set remote read timeout")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .context("failed to set remote write timeout")?;

    let reader_stream = stream
        .try_clone()
        .context("failed to clone remote stream for reading")?;
    let mut reader = BufReader::new(reader_stream);

    send_rpc_request(
        &mut stream,
        1,
        "rpc.hello",
        Some(json!({ "token": remote.token })),
    )?;
    read_rpc_success(&mut reader, "rpc.hello")?;

    send_rpc_request(&mut stream, 2, "sessions.list", None)?;
    let result = read_rpc_success(&mut reader, "sessions.list")?;
    let mut sessions: Vec<Session> =
        serde_json::from_value(result).context("remote sessions response had invalid shape")?;
    for session in &mut sessions {
        session.origin = SessionOrigin::Remote(remote.name.clone());
    }
    Ok(sessions)
}

pub fn serve(options: ServeOptions) -> Result<()> {
    if options.token.trim().is_empty() {
        anyhow::bail!("--token must not be empty");
    }

    let listener = TcpListener::bind(options.bind.trim())
        .with_context(|| format!("failed to bind {}", options.bind))?;
    for stream in listener.incoming() {
        let stream = stream.context("failed to accept RPC connection")?;
        let options = options.clone();
        thread::spawn(move || {
            if let Err(err) = handle_stream(stream, options) {
                eprintln!("coca RPC connection failed: {err:#}");
            }
        });
    }
    Ok(())
}

fn handle_stream(stream: TcpStream, options: ServeOptions) -> Result<()> {
    let reader_stream = stream
        .try_clone()
        .context("failed to clone RPC stream for reading")?;
    let reader = BufReader::new(reader_stream);
    handle_rpc_connection(reader, stream, &options)
}

fn handle_rpc_connection<R, W>(reader: R, mut writer: W, options: &ServeOptions) -> Result<()>
where
    R: BufRead,
    W: Write,
{
    let context = RpcContext {
        token: options.token.trim(),
        codex_home: options.codex_home.as_deref(),
        claude_home: options.claude_home.as_deref(),
        provider_filter: options.provider_filter,
    };
    let mut authenticated = false;

    for line in reader.lines() {
        let line = line.context("failed to read RPC request")?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_rpc_line(&line, &mut authenticated, &context);
        serde_json::to_writer(&mut writer, &response).context("failed to write RPC response")?;
        writer
            .write_all(b"\n")
            .context("failed to write RPC response frame")?;
        writer.flush().context("failed to flush RPC response")?;
    }

    Ok(())
}

fn handle_rpc_line(line: &str, authenticated: &mut bool, context: &RpcContext<'_>) -> RpcResponse {
    match serde_json::from_str::<RpcRequest>(line) {
        Ok(request) => handle_rpc_request(request, authenticated, context),
        Err(_) => rpc_error(None, PARSE_ERROR, "Parse error"),
    }
}

fn handle_rpc_request(
    request: RpcRequest,
    authenticated: &mut bool,
    context: &RpcContext<'_>,
) -> RpcResponse {
    let id = request.id.clone();
    if request.jsonrpc.as_deref() != Some(JSONRPC_VERSION) {
        return rpc_error(id, INVALID_REQUEST, "Invalid request");
    }

    let Some(method) = request.method.as_deref() else {
        return rpc_error(id, INVALID_REQUEST, "Invalid request");
    };
    if method != "rpc.hello" && !*authenticated {
        return rpc_error(id, UNAUTHENTICATED, "Unauthenticated");
    }

    match method {
        "rpc.hello" => handle_hello(id, request.params, authenticated, context),
        "sessions.list" => handle_sessions_list(id, *authenticated, context),
        _ => rpc_error(id, METHOD_NOT_FOUND, "Method not found"),
    }
}

fn handle_hello(
    id: Option<Value>,
    params: Option<Value>,
    authenticated: &mut bool,
    context: &RpcContext<'_>,
) -> RpcResponse {
    let Some(params) = params else {
        return rpc_error(id, INVALID_PARAMS, "Invalid params");
    };
    let params = match serde_json::from_value::<HelloParams>(params) {
        Ok(params) => params,
        Err(_) => return rpc_error(id, INVALID_PARAMS, "Invalid params"),
    };

    if params.token != context.token {
        return rpc_error(id, UNAUTHENTICATED, "Unauthenticated");
    }

    *authenticated = true;
    rpc_result(id, json!({ "ok": true }))
}

fn handle_sessions_list(
    id: Option<Value>,
    authenticated: bool,
    context: &RpcContext<'_>,
) -> RpcResponse {
    if !authenticated {
        return rpc_error(id, UNAUTHENTICATED, "Unauthenticated");
    }

    match providers::load_sessions(
        context.codex_home,
        context.claude_home,
        context.provider_filter,
    ) {
        Ok(sessions) => match serde_json::to_value(sessions) {
            Ok(value) => rpc_result(id, value),
            Err(_) => rpc_error(id, INTERNAL_ERROR, "Internal error"),
        },
        Err(_) => rpc_error(id, INTERNAL_ERROR, "Internal error"),
    }
}

fn send_rpc_request(
    writer: &mut impl Write,
    id: u64,
    method: &str,
    params: Option<Value>,
) -> Result<()> {
    let mut request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "method": method,
        "id": id,
    });
    if let Some(params) = params {
        request["params"] = params;
    }

    serde_json::to_writer(&mut *writer, &request).context("failed to write RPC request")?;
    writer
        .write_all(b"\n")
        .context("failed to write RPC request frame")?;
    writer.flush().context("failed to flush RPC request")?;
    Ok(())
}

fn read_rpc_success(reader: &mut impl BufRead, method: &str) -> Result<Value> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .with_context(|| format!("failed to read {method} RPC response"))?;
    if bytes == 0 {
        return Err(anyhow!(
            "remote closed connection while waiting for {method}"
        ));
    }

    let response: Value = serde_json::from_str(&line)
        .with_context(|| format!("{method} RPC response was not valid JSON"))?;
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("RPC error");
        return Err(anyhow!("{method} failed: {message}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("{method} RPC response did not include result"))
}

fn rpc_result(id: Option<Value>, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: JSONRPC_VERSION,
        result: Some(result),
        error: None,
        id,
    }
}

fn rpc_error(id: Option<Value>, code: i64, message: &str) -> RpcResponse {
    RpcResponse {
        jsonrpc: JSONRPC_VERSION,
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
        }),
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates_remote_config() {
        let config: RemoteConfig = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "work", "addr": "127.0.0.1:8765", "token": "secret" }
                ]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_ok());
        assert_eq!(config.remotes[0].name, "work");
    }

    #[test]
    fn rejects_invalid_remote_config() {
        let config: RemoteConfig = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "", "addr": "127.0.0.1:8765", "token": "secret" }
                ]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_err());
    }

    #[test]
    fn sessions_list_requires_authentication() {
        let options = test_options();
        let context = test_context(&options);
        let mut authenticated = false;

        let response = handle_rpc_line(
            r#"{"jsonrpc":"2.0","method":"sessions.list","id":1}"#,
            &mut authenticated,
            &context,
        );

        assert_eq!(response.error.unwrap().code, UNAUTHENTICATED);
    }

    #[test]
    fn hello_rejects_bad_token() {
        let options = test_options();
        let context = test_context(&options);
        let mut authenticated = false;

        let response = handle_rpc_line(
            r#"{"jsonrpc":"2.0","method":"rpc.hello","params":{"token":"bad"},"id":1}"#,
            &mut authenticated,
            &context,
        );

        assert_eq!(response.error.unwrap().code, UNAUTHENTICATED);
        assert!(!authenticated);
    }

    #[test]
    fn hello_allows_sessions_list() {
        let options = test_options();
        let context = test_context(&options);
        let mut authenticated = false;

        let hello = handle_rpc_line(
            r#"{"jsonrpc":"2.0","method":"rpc.hello","params":{"token":"secret"},"id":1}"#,
            &mut authenticated,
            &context,
        );
        assert!(hello.error.is_none());
        assert!(authenticated);

        let list = handle_rpc_line(
            r#"{"jsonrpc":"2.0","method":"sessions.list","id":2}"#,
            &mut authenticated,
            &context,
        );
        assert!(list.error.is_none());
        assert!(list.result.unwrap().is_array());
    }

    #[test]
    fn malformed_json_returns_parse_error() {
        let options = test_options();
        let context = test_context(&options);
        let mut authenticated = false;

        let response = handle_rpc_line("{bad", &mut authenticated, &context);

        assert_eq!(response.error.unwrap().code, PARSE_ERROR);
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let options = test_options();
        let context = test_context(&options);
        let mut authenticated = true;

        let response = handle_rpc_line(
            r#"{"jsonrpc":"2.0","method":"unknown","id":1}"#,
            &mut authenticated,
            &context,
        );

        assert_eq!(response.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn fetch_remote_sessions_uses_tcp_rpc_and_maps_origin() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader_stream = stream.try_clone().unwrap();
            let mut reader = BufReader::new(reader_stream);
            let mut writer = stream;
            let mut line = String::new();

            reader.read_line(&mut line).unwrap();
            writer
                .write_all(br#"{"jsonrpc":"2.0","result":{"ok":true},"id":1}"#)
                .unwrap();
            writer.write_all(b"\n").unwrap();
            writer.flush().unwrap();

            line.clear();
            reader.read_line(&mut line).unwrap();
            writer
                .write_all(
                    br#"{"jsonrpc":"2.0","result":[{"origin":"Local","provider":"Codex","id":"sid","title":"title","cwd":"/tmp","created_at_ms":1,"updated_at_ms":2,"model":null,"source_path":"/tmp/session","first_user_message":null,"transcript":[],"resume_program":"codex","resume_args":["resume","sid"]}],"id":2}"#,
                )
                .unwrap();
            writer.write_all(b"\n").unwrap();
            writer.flush().unwrap();
        });

        let sessions = fetch_remote_sessions(&RemoteEndpoint {
            name: "work-mac".to_string(),
            addr: addr.to_string(),
            token: "secret".to_string(),
        })
        .unwrap();

        assert_eq!(
            sessions[0].origin,
            SessionOrigin::Remote("work-mac".to_string())
        );
        handle.join().unwrap();
    }

    fn test_options() -> ServeOptions {
        ServeOptions {
            bind: "127.0.0.1:0".to_string(),
            token: "secret".to_string(),
            codex_home: None,
            claude_home: None,
            provider_filter: ProviderFilter::All,
        }
    }

    fn test_context(options: &ServeOptions) -> RpcContext<'_> {
        RpcContext {
            token: &options.token,
            codex_home: options.codex_home.as_deref(),
            claude_home: options.claude_home.as_deref(),
            provider_filter: options.provider_filter,
        }
    }
}
