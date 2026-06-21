use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use coca_app::{
    AiSettingsUpdate, AppOptions, AppService, SessionRef, SessionsResponse, StreamInfo,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone, Debug)]
pub struct WebOptions {
    pub bind: String,
    pub app: AppOptions,
    pub static_dir: PathBuf,
    pub cache: WebCache,
}

#[derive(Clone, Debug, Default)]
pub struct WebCache {
    sessions: Arc<Mutex<Option<CachedSessions>>>,
    sessions_refreshing: Arc<Mutex<bool>>,
    app: Arc<Mutex<Option<AppOptions>>>,
}

#[derive(Clone, Debug)]
struct CachedSessions {
    loaded_at: Instant,
    response: SessionsResponse,
}

const SESSIONS_CACHE_TTL: Duration = Duration::from_secs(10);

pub fn serve(options: WebOptions) -> Result<()> {
    if options.app.settings.share.token.trim().is_empty() {
        anyhow::bail!("web API token must not be empty");
    }

    let listener = TcpListener::bind(options.bind.trim())
        .with_context(|| format!("failed to bind {}", options.bind))?;
    for stream in listener.incoming() {
        let stream = stream.context("failed to accept web connection")?;
        let options = options.clone();
        thread::spawn(move || {
            if let Err(err) = handle_stream(stream, options) {
                eprintln!("coca web connection failed: {err:#}");
            }
        });
    }
    Ok(())
}

fn handle_stream(mut stream: TcpStream, options: WebOptions) -> Result<()> {
    let reader_stream = stream.try_clone().context("failed to clone stream")?;
    let mut reader = BufReader::new(reader_stream);
    let response = match read_request(&mut reader)? {
        Some(request) => route_request(&request, &options),
        None => Response::text(400, "Bad Request", "Malformed request"),
    };
    write_response(&mut stream, &request_safe_response(&response))
}

fn route_request(request: &Request, options: &WebOptions) -> Response {
    let path = request.path();
    if path.starts_with("/api/") {
        return route_api(request, options);
    }
    route_static(request, &options.static_dir)
}

fn route_api(request: &Request, options: &WebOptions) -> Response {
    if let Some(response) = reject_api_request(request, options) {
        return response;
    }

    let mut app_options = current_app_options(options);
    let mut app = AppService::new(app_options.clone());
    match (request.method.as_str(), request.path()) {
        ("GET", "/api/v1/health") => json_response(json!({
            "ok": true,
            "service": "coca-web",
            "version": env!("CARGO_PKG_VERSION"),
            "stream": StreamInfo::default(),
        })),
        ("GET", "/api/v1/sessions") => cached_sessions(options, &app)
            .map(json_response)
            .unwrap_or_else(error_response),
        ("GET", "/api/v1/session") => {
            let Some(reference) = session_ref_from_query(request) else {
                return Response::text(400, "Bad Request", "missing session reference");
            };
            match app.web_session_detail(&reference) {
                Ok(Some(detail)) => json_response(detail),
                Ok(None) => Response::text(404, "Not Found", "session not found"),
                Err(err) => error_response(err),
            }
        }
        ("GET", "/api/v1/config/summary") => app
            .config_summary(&options.bind)
            .map(json_response)
            .unwrap_or_else(error_response),
        ("PUT", "/api/v1/config/ai") => {
            let Ok(body) = serde_json::from_slice::<AiSettingsUpdate>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid ai config payload");
            };
            match app.update_ai_settings(body) {
                Ok(summary) => {
                    app_options.settings = app.settings();
                    store_app_options(options, app_options);
                    json_response(summary)
                }
                Err(err) => error_response(err),
            }
        }
        ("POST", "/api/v1/share-session") => {
            let Ok(body) = serde_json::from_slice::<ShareSessionRequest>(&request.body) else {
                return Response::text(400, "Bad Request", "invalid share-session payload");
            };
            app.share_session(&body.session)
                .map(json_response)
                .unwrap_or_else(error_response)
        }
        ("GET", "/api/v1/stream") => json_response(json!({
            "ok": false,
            "error": "terminal stream transport is reserved but not implemented",
            "stream": StreamInfo::default(),
        }))
        .with_status(501, "Not Implemented"),
        (_, "/api/v1/health")
        | (_, "/api/v1/sessions")
        | (_, "/api/v1/session")
        | (_, "/api/v1/config/summary")
        | (_, "/api/v1/config/ai")
        | (_, "/api/v1/share-session")
        | (_, "/api/v1/stream") => Response::text(405, "Method Not Allowed", "method not allowed"),
        _ => Response::text(404, "Not Found", "not found"),
    }
}

fn current_app_options(options: &WebOptions) -> AppOptions {
    options
        .cache
        .app
        .lock()
        .expect("web app options mutex poisoned")
        .as_ref()
        .cloned()
        .unwrap_or_else(|| options.app.clone())
}

fn store_app_options(options: &WebOptions, app: AppOptions) {
    *options
        .cache
        .app
        .lock()
        .expect("web app options mutex poisoned") = Some(app);
}

fn cached_sessions(options: &WebOptions, app: &AppService) -> Result<SessionsResponse> {
    if let Some(cached) = options
        .cache
        .sessions
        .lock()
        .expect("web sessions cache mutex poisoned")
        .as_ref()
        .cloned()
    {
        let age = cached.loaded_at.elapsed();
        if age < SESSIONS_CACHE_TTL {
            return Ok(cached.response);
        }

        start_sessions_refresh(options, app.clone());
        let mut response = cached.response;
        response.warnings.push(format!(
            "serving cached sessions from {}s ago while refreshing in background",
            age.as_secs()
        ));
        return Ok(response);
    }

    match app.stored_web_sessions() {
        Ok(Some(response)) => {
            store_stale_sessions_cache(options, response.clone());
            start_sessions_refresh(options, app.clone());
            return Ok(response);
        }
        Ok(None) => {}
        Err(err) => eprintln!("coca web stored sessions cache unavailable: {err:#}"),
    }

    let response = app.web_sessions()?;
    store_sessions_cache(options, response.clone());
    Ok(response)
}

fn start_sessions_refresh(options: &WebOptions, app: AppService) {
    {
        let mut refreshing = options
            .cache
            .sessions_refreshing
            .lock()
            .expect("web sessions refresh mutex poisoned");
        if *refreshing {
            return;
        }
        *refreshing = true;
    }

    let cache = options.cache.clone();
    thread::spawn(move || {
        match app.web_sessions() {
            Ok(response) => {
                *cache
                    .sessions
                    .lock()
                    .expect("web sessions cache mutex poisoned") = Some(CachedSessions {
                    loaded_at: Instant::now(),
                    response,
                });
            }
            Err(err) => eprintln!("coca web sessions refresh failed: {err:#}"),
        }
        *cache
            .sessions_refreshing
            .lock()
            .expect("web sessions refresh mutex poisoned") = false;
    });
}

fn store_sessions_cache(options: &WebOptions, response: SessionsResponse) {
    store_sessions_cache_at(options, Instant::now(), response);
}

fn store_stale_sessions_cache(options: &WebOptions, response: SessionsResponse) {
    store_sessions_cache_at(
        options,
        Instant::now() - SESSIONS_CACHE_TTL - Duration::from_secs(1),
        response,
    );
}

fn store_sessions_cache_at(options: &WebOptions, loaded_at: Instant, response: SessionsResponse) {
    *options
        .cache
        .sessions
        .lock()
        .expect("web sessions cache mutex poisoned") = Some(CachedSessions {
        loaded_at,
        response,
    });
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

fn reject_api_request(request: &Request, options: &WebOptions) -> Option<Response> {
    let expected = options.app.settings.share.token.trim();
    let token = request.bearer_token().or_else(|| {
        request
            .query_param("token")
            .and_then(|value| percent_decode(&value))
    });
    if token.as_deref() != Some(expected) {
        return Some(Response::text(401, "Unauthorized", "unauthorized"));
    }
    None
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

    fn bearer_token(&self) -> Option<String> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .and_then(|(_, value)| value.strip_prefix("Bearer "))
            .map(str::trim)
            .map(str::to_string)
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
    use coca_core::model::ProviderFilter;
    use coca_core::settings::Settings;

    #[test]
    fn api_requires_token() {
        let options = web_options();
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
        let options = web_options();
        let request = Request {
            method: "GET".to_string(),
            target: "/api/v1/health?token=secret".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let response = route_request(&request, &options);
        let body = String::from_utf8(response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("terminal.open"));
        assert!(body.contains("terminal.output"));
    }

    #[test]
    fn put_ai_config_requires_authentication() {
        let options = web_options();
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
        let mut options = web_options();
        options.app.settings.ai.api_key = "sk-existing".to_string();
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

        let summary = route_request(
            &Request {
                method: "GET".to_string(),
                target: "/api/v1/config/summary?token=secret".to_string(),
                headers: Vec::new(),
                body: Vec::new(),
            },
            &options,
        );
        let summary_body = String::from_utf8(summary.body).unwrap();

        assert_eq!(summary.status, 200);
        assert!(summary_body.contains("https://example.test/v1"));
        assert!(summary_body.contains("\"api_key_configured\":true"));
        assert!(!summary_body.contains("sk-existing"));
    }

    #[test]
    fn put_ai_config_clear_flag_clears_api_key() {
        let mut options = web_options();
        options.app.settings.ai.api_key = "sk-existing".to_string();
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
    }

    #[test]
    fn static_routes_do_not_render_rust_html_when_assets_are_missing() {
        let options = web_options();
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

    fn web_options() -> WebOptions {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        settings.share.token = "secret".to_string();
        WebOptions {
            bind: "127.0.0.1:0".to_string(),
            app: AppOptions {
                settings,
                settings_path: None,
                codex_home: None,
                claude_home: None,
                provider_filter: ProviderFilter::All,
                database_path: None,
            },
            static_dir: PathBuf::from("__missing_static_dir__"),
            cache: WebCache::default(),
        }
    }
}
