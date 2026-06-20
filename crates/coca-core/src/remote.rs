use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::model::{Session, SessionOrigin};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RemoteConfig {
    pub remotes: Vec<RemoteEndpoint>,
}

impl RemoteConfig {
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
            if remote.base_url.trim().is_empty() {
                anyhow::bail!("remote {name} base_url must not be empty");
            }
            if remote.token.trim().is_empty() {
                anyhow::bail!("remote {name} token must not be empty");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEndpoint {
    pub name: String,
    pub base_url: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
struct RemoteEndpointWire {
    name: String,
    base_url: Option<String>,
    addr: Option<String>,
    token: String,
}

impl<'de> Deserialize<'de> for RemoteEndpoint {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RemoteEndpointWire::deserialize(deserializer)?;
        let base_url = wire
            .base_url
            .or_else(|| wire.addr.map(normalize_legacy_addr))
            .unwrap_or_default();
        Ok(Self {
            name: wire.name,
            base_url,
            token: wire.token,
        })
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
    let base = parse_http_base_url(remote.base_url.trim())
        .with_context(|| format!("invalid remote base_url for {}", remote.name))?;
    let mut stream = TcpStream::connect(&base.addr)
        .with_context(|| format!("failed to connect to remote {}", remote.name))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .context("failed to set remote read timeout")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .context("failed to set remote write timeout")?;

    let target = base.target("/api/sessions");
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        base.host_header,
        remote.token.trim()
    )
    .context("failed to write remote sessions request")?;
    stream.flush().context("failed to flush remote request")?;

    let response = read_http_response(stream).context("failed to read remote sessions response")?;
    if response.status != 200 {
        anyhow::bail!(
            "remote sessions request failed with HTTP {}",
            response.status
        );
    }
    let mut sessions: Vec<Session> = serde_json::from_str(&response.body)
        .context("remote sessions response had invalid shape")?;
    for session in &mut sessions {
        session.origin = SessionOrigin::Remote(remote.name.clone());
    }
    Ok(sessions)
}

fn normalize_legacy_addr(addr: String) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr
    } else {
        format!("http://{addr}")
    }
}

struct HttpBase {
    addr: String,
    host_header: String,
    path_prefix: String,
}

impl HttpBase {
    fn target(&self, path: &str) -> String {
        let prefix = self.path_prefix.trim_end_matches('/');
        if prefix.is_empty() {
            path.to_string()
        } else {
            format!("{prefix}{path}")
        }
    }
}

fn parse_http_base_url(base_url: &str) -> Result<HttpBase> {
    let rest = base_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("only http:// remote core URLs are supported"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.trim().is_empty() {
        anyhow::bail!("remote core URL host must not be empty");
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => {
            let port: u16 = port.parse().context("remote core URL port was invalid")?;
            (host.to_string(), port)
        }
        _ => (authority.to_string(), 80),
    };
    let path_prefix = if path.is_empty() {
        String::new()
    } else {
        format!("/{path}")
    };

    Ok(HttpBase {
        addr: format!("{host}:{port}"),
        host_header: authority.to_string(),
        path_prefix,
    })
}

struct HttpClientResponse {
    status: u16,
    body: String,
}

fn read_http_response(stream: TcpStream) -> Result<HttpClientResponse> {
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader
        .read_to_string(&mut response)
        .context("failed to read HTTP response")?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .or_else(|| response.split_once("\n\n"))
        .ok_or_else(|| anyhow!("HTTP response did not include a header terminator"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| anyhow!("HTTP response did not include a status line"))?;
    let mut parts = status_line.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        anyhow::bail!("HTTP response status line was invalid");
    }
    let status = parts
        .next()
        .ok_or_else(|| anyhow!("HTTP response status line did not include status"))?
        .parse()
        .context("HTTP response status was invalid")?;
    Ok(HttpClientResponse {
        status,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderKind;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_and_validates_remote_config() {
        let config: RemoteConfig = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "work", "base_url": "http://127.0.0.1:8787", "token": "secret" }
                ]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_ok());
        assert_eq!(config.remotes[0].name, "work");
        assert_eq!(config.remotes[0].base_url, "http://127.0.0.1:8787");
    }

    #[test]
    fn parses_legacy_addr_as_http_base_url() {
        let config: RemoteConfig = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "work", "addr": "127.0.0.1:8765", "token": "secret" }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(config.remotes[0].base_url, "http://127.0.0.1:8765");
    }

    #[test]
    fn rejects_invalid_remote_config() {
        let config: RemoteConfig = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "", "base_url": "http://127.0.0.1:8787", "token": "secret" }
                ]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_err());
    }

    #[test]
    fn fetch_remote_sessions_uses_http_bearer_and_maps_origin() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader_stream = stream.try_clone().unwrap();
            let mut reader = BufReader::new(reader_stream);
            let mut writer = stream;
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line == "\n" {
                    break;
                }
                request.push_str(&line);
            }
            assert!(request.starts_with("GET /api/sessions HTTP/1.1"));
            assert!(request.contains("Authorization: Bearer secret"));

            let body = r#"[{"origin":"Local","provider":"Codex","id":"sid","title":"title","cwd":"/tmp","created_at_ms":1,"updated_at_ms":2,"model":null,"source_path":"/tmp/session","first_user_message":null,"transcript":[],"resume_program":"codex","resume_args":["resume","sid"]}]"#;
            write!(
                writer,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            writer.flush().unwrap();
        });

        let sessions = fetch_remote_sessions(&RemoteEndpoint {
            name: "work-mac".to_string(),
            base_url: format!("http://{addr}"),
            token: "secret".to_string(),
        })
        .unwrap();

        assert_eq!(sessions[0].provider, ProviderKind::Codex);
        assert_eq!(
            sessions[0].origin,
            SessionOrigin::Remote("work-mac".to_string())
        );
        handle.join().unwrap();
    }
}
