use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;

use anyhow::{Context, Result};

use crate::http::{
    read_http_request, simple_response, write_http_response, HttpRequest, HttpResponse,
};
use crate::model::{ProviderFilter, Session};
use crate::providers;

#[derive(Clone, Debug)]
pub struct CoreOptions {
    pub bind: String,
    pub token: String,
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
}

pub fn serve(options: CoreOptions) -> Result<()> {
    if options.token.trim().is_empty() {
        anyhow::bail!("core token must not be empty");
    }

    let listener = TcpListener::bind(options.bind.trim())
        .with_context(|| format!("failed to bind {}", options.bind))?;
    for stream in listener.incoming() {
        let stream = stream.context("failed to accept core connection")?;
        let options = options.clone();
        thread::spawn(move || {
            if let Err(err) = handle_stream(stream, options) {
                eprintln!("coca core connection failed: {err:#}");
            }
        });
    }
    Ok(())
}

fn handle_stream(mut stream: TcpStream, options: CoreOptions) -> Result<()> {
    let reader_stream = stream
        .try_clone()
        .context("failed to clone core stream for reading")?;
    let mut reader = BufReader::new(reader_stream);
    let response = match read_http_request(&mut reader)? {
        Some(request) => route_request(&request, &options),
        None => simple_response(400, "Bad Request", "Malformed request"),
    };
    write_http_response(&mut stream, &response)
}

fn route_request(request: &HttpRequest, options: &CoreOptions) -> HttpResponse {
    let path = request
        .target
        .split_once('?')
        .map_or(request.target.as_str(), |(path, _)| path);
    if path == "/api/sessions" {
        return route_sessions_api(request, options);
    }
    simple_response(404, "Not Found", "Not found")
}

fn route_sessions_api(request: &HttpRequest, options: &CoreOptions) -> HttpResponse {
    if request.method != "GET" {
        return simple_response(405, "Method Not Allowed", "Method not allowed");
    }
    if bearer_token(request).as_deref() != Some(options.token.trim()) {
        return simple_response(401, "Unauthorized", "Unauthorized");
    }

    match serde_json::to_string(&load_local_sessions(options)) {
        Ok(body) => HttpResponse::new(200, "OK", "application/json; charset=utf-8", body),
        Err(_) => simple_response(500, "Internal Server Error", "Internal error"),
    }
}

fn load_local_sessions(options: &CoreOptions) -> Vec<Session> {
    providers::load_sessions(
        options.codex_home.as_deref(),
        options.claude_home.as_deref(),
        options.provider_filter,
    )
    .unwrap_or_default()
}

fn bearer_token(request: &HttpRequest) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| value.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_api_requires_bearer_token() {
        let options = test_options();
        let request = request("GET", "/api/sessions", &[]);

        let response = route_request(&request, &options);

        assert_eq!(response.status, 401);
    }

    #[test]
    fn sessions_api_returns_json_for_valid_bearer_token() {
        let options = test_options();
        let request = request(
            "GET",
            "/api/sessions",
            &[("Authorization", "Bearer secret")],
        );

        let response = route_request(&request, &options);

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        assert!(serde_json::from_str::<Vec<Session>>(&response.body).is_ok());
    }

    #[test]
    fn sessions_api_rejects_unsupported_methods() {
        let options = test_options();
        let request = request(
            "POST",
            "/api/sessions",
            &[("Authorization", "Bearer secret")],
        );

        let response = route_request(&request, &options);

        assert_eq!(response.status, 405);
    }

    fn test_options() -> CoreOptions {
        CoreOptions {
            bind: "127.0.0.1:0".to_string(),
            token: "secret".to_string(),
            codex_home: None,
            claude_home: None,
            provider_filter: ProviderFilter::All,
        }
    }

    fn request(method: &str, target: &str, headers: &[(&str, &str)]) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            target: target.to_string(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        }
    }
}
