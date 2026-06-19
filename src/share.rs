use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Local};

use crate::model::{ChatMessage, ProviderFilter, ProviderKind, Session};
use crate::providers;

#[derive(Clone, Debug)]
pub struct ShareServeOptions {
    pub bind: String,
    pub token: String,
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: String,
}

pub fn serve(options: ShareServeOptions) -> Result<()> {
    if options.token.trim().is_empty() {
        anyhow::bail!("--token must not be empty");
    }

    let listener = TcpListener::bind(options.bind.trim())
        .with_context(|| format!("failed to bind {}", options.bind))?;
    for stream in listener.incoming() {
        let stream = stream.context("failed to accept share connection")?;
        let options = options.clone();
        thread::spawn(move || {
            if let Err(err) = handle_stream(stream, options) {
                eprintln!("coca share connection failed: {err:#}");
            }
        });
    }
    Ok(())
}

pub fn build_share_url(base_url: &str, token: &str, session: &Session) -> String {
    format!(
        "{}/s/{}/{}?token={}",
        base_url.trim_end_matches('/'),
        session.provider,
        percent_encode(&session.id),
        percent_encode(token)
    )
}

fn handle_stream(mut stream: TcpStream, options: ShareServeOptions) -> Result<()> {
    let reader_stream = stream
        .try_clone()
        .context("failed to clone share stream for reading")?;
    let mut reader = BufReader::new(reader_stream);
    let request = read_http_request(&mut reader)?;
    let response = match request {
        Some((method, target)) => {
            let sessions = providers::load_sessions(
                options.codex_home.as_deref(),
                options.claude_home.as_deref(),
                options.provider_filter,
            )
            .unwrap_or_default();
            route_request(&method, &target, options.token.trim(), &sessions)
        }
        None => simple_response(400, "Bad Request", "Malformed request"),
    };
    write_http_response(&mut stream, &response)
}

fn read_http_request(reader: &mut impl BufRead) -> Result<Option<(String, String)>> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .context("failed to read HTTP request line")?
        == 0
    {
        return Ok(None);
    }

    let mut parts = line.split_whitespace();
    let Some(method) = parts.next().map(str::to_string) else {
        return Ok(None);
    };
    let Some(target) = parts.next().map(str::to_string) else {
        return Ok(None);
    };

    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .context("failed to read HTTP headers")?
            == 0
        {
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }

    Ok(Some((method, target)))
}

fn write_http_response(writer: &mut impl Write, response: &HttpResponse) -> Result<()> {
    write!(
        writer,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        response.status,
        response.reason,
        response.content_type,
        response.body.len(),
        response.body
    )
    .context("failed to write HTTP response")
}

fn route_request(
    method: &str,
    target: &str,
    expected_token: &str,
    sessions: &[Session],
) -> HttpResponse {
    let include_body = match method {
        "GET" => true,
        "HEAD" => false,
        _ => return simple_response(405, "Method Not Allowed", "Method not allowed"),
    };

    let Ok(route) = parse_share_route(target) else {
        return simple_response(404, "Not Found", "Not found");
    };

    if route.token.as_deref() != Some(expected_token) {
        return simple_response(401, "Unauthorized", "Unauthorized");
    }

    let Some(provider) = provider_from_path(route.provider.as_str()) else {
        return simple_response(404, "Not Found", "Not found");
    };

    let Some(session) = sessions
        .iter()
        .find(|session| session.provider == provider && session.id == route.session_id)
    else {
        return simple_response(404, "Not Found", "Not found");
    };

    let body = if include_body {
        render_session_html(session)
    } else {
        String::new()
    };
    HttpResponse {
        status: 200,
        reason: "OK",
        content_type: "text/html; charset=utf-8",
        body,
    }
}

struct ShareRoute {
    provider: String,
    session_id: String,
    token: Option<String>,
}

fn parse_share_route(target: &str) -> Result<ShareRoute> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut segments = path.trim_start_matches('/').split('/');
    if segments.next() != Some("s") {
        return Err(anyhow!("not a share route"));
    }
    let provider = segments
        .next()
        .ok_or_else(|| anyhow!("missing provider"))?
        .to_string();
    let encoded_id = segments
        .next()
        .ok_or_else(|| anyhow!("missing session id"))?;
    if segments.next().is_some() {
        return Err(anyhow!("extra path segment"));
    }
    let session_id = percent_decode(encoded_id).ok_or_else(|| anyhow!("invalid session id"))?;
    let token = query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(key, _)| *key == "token")
        .and_then(|(_, value)| percent_decode(value));

    Ok(ShareRoute {
        provider,
        session_id,
        token,
    })
}

fn provider_from_path(provider: &str) -> Option<ProviderKind> {
    match provider {
        "codex" => Some(ProviderKind::Codex),
        "claude" => Some(ProviderKind::Claude),
        _ => None,
    }
}

fn render_session_html(session: &Session) -> String {
    let transcript = render_transcript(session);
    let first_prompt = transcript
        .first_prompt
        .as_deref()
        .map(render_first_prompt)
        .unwrap_or_default();
    let messages = render_entries(&transcript.entries);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{
  color-scheme: dark;
  --bg: #11110f;
  --surface: #171814;
  --surface-2: #202117;
  --ink: #f0ead8;
  --muted: #9e9a89;
  --faint: #6e6a5e;
  --line: #34362b;
  --line-strong: #4c4d3d;
  --codex: #c9a84d;
  --user: #89c997;
  --assistant: #d8c27a;
  --notice: #7fb4d8;
  --shadow: rgba(0, 0, 0, .36);
}}
* {{ box-sizing: border-box; }}
html {{ scroll-behavior: smooth; }}
body {{
  margin: 0;
  background:
    linear-gradient(180deg, rgba(201, 168, 77, .07), transparent 260px),
    radial-gradient(circle at top right, rgba(127, 180, 216, .08), transparent 360px),
    var(--bg);
  color: var(--ink);
  font-family: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  line-height: 1.58;
}}
.topbar {{
  position: sticky;
  top: 0;
  z-index: 10;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 24px;
  align-items: center;
  padding: 14px clamp(18px, 4vw, 44px);
  background: rgba(17, 17, 15, .86);
  border-bottom: 1px solid var(--line);
  backdrop-filter: blur(16px);
}}
.brand {{
  display: flex;
  align-items: baseline;
  gap: 12px;
  min-width: 0;
}}
.brand strong {{
  color: var(--codex);
  font-size: 13px;
  letter-spacing: .14em;
  text-transform: uppercase;
}}
.brand span {{
  min-width: 0;
  overflow: hidden;
  color: var(--muted);
  font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  text-overflow: ellipsis;
  white-space: nowrap;
}}
.status {{
  display: flex;
  gap: 8px;
  align-items: center;
  color: var(--muted);
  font-size: 12px;
}}
.dot {{
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--user);
  box-shadow: 0 0 18px rgba(137, 201, 151, .65);
}}
.content {{
  width: min(1240px, calc(100vw - 32px));
  margin: 0 auto;
  padding: 28px 0 64px;
}}
.layout {{
  display: grid;
  grid-template-columns: minmax(0, 860px) 300px;
  gap: 20px;
  align-items: start;
}}
.main-column {{
  min-width: 0;
  display: grid;
  gap: 14px;
}}
.side-rail {{
  position: sticky;
  top: 72px;
  min-width: 0;
  display: grid;
  gap: 12px;
}}
.session-head {{
  margin-bottom: 24px;
}}
.title-panel,
.prompt-panel,
.meta-panel {{
  border: 1px solid var(--line);
  background: rgba(23, 24, 20, .88);
  box-shadow: 0 18px 48px var(--shadow);
}}
.title-panel {{
  padding: 22px 24px 20px;
}}
.eyebrow {{
  display: flex;
  gap: 10px;
  align-items: center;
  margin-bottom: 12px;
  color: var(--codex);
  font-size: 12px;
  letter-spacing: .12em;
  text-transform: uppercase;
}}
.provider-pill {{
  display: inline-flex;
  align-items: center;
  height: 22px;
  padding: 0 9px;
  border: 1px solid rgba(201, 168, 77, .42);
  color: var(--codex);
  background: rgba(201, 168, 77, .08);
  border-radius: 999px;
  font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  letter-spacing: 0;
  text-transform: none;
}}
h1 {{
  max-width: 920px;
  margin: 0;
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  font-size: clamp(21px, 2.5vw, 29px);
  line-height: 1.24;
  font-weight: 680;
  letter-spacing: 0;
}}
.summary {{
  display: flex;
  flex-wrap: wrap;
  gap: 10px 16px;
  margin-top: 18px;
  color: var(--muted);
  font-size: 13px;
}}
.summary code {{
  color: var(--ink);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}}
.prompt-panel {{
  display: grid;
  grid-template-columns: 118px minmax(0, 1fr);
  gap: 18px;
  padding: 18px;
  border-left: 3px solid var(--user);
}}
.prompt-label {{
  color: var(--muted);
  font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}}
.prompt-label strong {{
  display: inline-flex;
  align-items: center;
  height: 24px;
  margin-bottom: 10px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  background: rgba(0, 0, 0, .14);
  color: var(--user);
  font-weight: 400;
}}
.prompt-label span {{
  display: block;
  color: var(--faint);
}}
.prompt-body {{
  min-width: 0;
}}
.prompt-text {{
  margin: 0;
  color: #f5edda;
  font-size: 16px;
  line-height: 1.7;
}}
.prompt-text p {{
  margin: 0 0 12px;
}}
.prompt-text p:last-child {{ margin-bottom: 0; }}
.meta-panel {{
  display: grid;
  padding: 8px;
  box-shadow: none;
}}
.meta-row {{
  min-width: 0;
  display: grid;
  grid-template-columns: 86px minmax(0, 1fr);
  gap: 12px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--line);
}}
.meta-row:last-child {{ border-bottom: 0; }}
.meta-row dt {{
  color: var(--faint);
  font-size: 11px;
  text-transform: uppercase;
}}
.meta-row dd {{
  min-width: 0;
  margin: 0;
  overflow: hidden;
  color: var(--ink);
  font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  text-overflow: ellipsis;
  white-space: nowrap;
}}
.transcript {{
  display: grid;
  gap: 14px;
}}
.message {{
  display: grid;
  grid-template-columns: 118px minmax(0, 1fr);
  gap: 18px;
  padding: 18px;
  border: 1px solid var(--line);
  background: rgba(23, 24, 20, .78);
}}
.message.user {{
  border-left: 3px solid var(--user);
}}
.message.assistant {{
  border-left: 3px solid var(--assistant);
}}
.message.event,
.message.context {{
  border-left: 3px solid var(--notice);
}}
.rail {{
  color: var(--muted);
  font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}}
.rail .role {{
  display: inline-flex;
  align-items: center;
  height: 24px;
  margin-bottom: 10px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  background: rgba(0, 0, 0, .14);
}}
.message.user .role {{ color: var(--user); }}
.message.assistant .role {{ color: var(--assistant); }}
.message.event .role,
.message.context .role {{ color: var(--notice); }}
.rail .time {{
  display: block;
  color: var(--faint);
  line-height: 1.35;
}}
.body {{
  min-width: 0;
  color: var(--ink);
}}
.body p {{
  margin: 0 0 12px;
}}
.body p:last-child {{ margin-bottom: 0; }}
.body pre,
.code-block,
.structured-block {{
  margin: 0;
  overflow: auto;
  border: 1px solid var(--line);
  background: #0c0d0b;
  color: #e8e0c6;
  font: 13px/1.55 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  white-space: pre;
}}
.body pre {{
  padding: 14px 16px;
}}
.code-block {{
  margin: 10px 0;
}}
.structured-block {{
  margin: 10px 0;
  border-color: rgba(127, 180, 216, .32);
  background: rgba(8, 17, 22, .82);
}}
.block-head {{
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid rgba(127, 180, 216, .22);
  color: var(--notice);
  font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}}
.block-body {{
  padding: 12px;
  overflow: auto;
  white-space: pre;
}}
.assistant-text {{
  max-width: 78ch;
}}
.empty {{
  border: 1px solid var(--line);
  padding: 18px;
  color: var(--muted);
  background: rgba(23, 24, 20, .78);
}}
@media (max-width: 980px) {{
  .topbar {{
    grid-template-columns: 1fr;
    gap: 6px;
  }}
  .layout {{
    grid-template-columns: 1fr;
  }}
  .side-rail {{
    position: static;
    order: -1;
  }}
}}
@media (max-width: 840px) {{
  .prompt-panel,
  .message {{
    grid-template-columns: 1fr;
    gap: 10px;
  }}
  .rail,
  .prompt-label {{
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }}
}}
</style>
</head>
<body>
<header class="topbar">
  <div class="brand">
    <strong>coca share</strong>
    <span>{id}</span>
  </div>
  <div class="status"><span class="dot"></span><span>read-only session</span></div>
</header>
<main class="content">
  <div class="layout">
    <div class="main-column">
      <section class="session-head" id="top">
        <div class="title-panel">
          <div class="eyebrow">
            <span class="provider-pill">{provider}</span>
            <span>agent transcript</span>
          </div>
          <h1 title="{title}">{title}</h1>
          <div class="summary">
            <span>model <code>{model}</code></span>
            <span>updated <code>{updated}</code></span>
            <span>workspace <code>{cwd}</code></span>
          </div>
        </div>
      </section>
      {first_prompt}
      <section class="transcript" aria-label="Transcript">{messages}</section>
    </div>
    <aside class="side-rail" aria-label="Session metadata">
      <dl class="meta-panel">
        <div class="meta-row"><dt>Provider</dt><dd>{provider}</dd></div>
        <div class="meta-row"><dt>Model</dt><dd>{model}</dd></div>
        <div class="meta-row"><dt>Session</dt><dd title="{id}">{id}</dd></div>
        <div class="meta-row"><dt>CWD</dt><dd title="{cwd}">{cwd}</dd></div>
        <div class="meta-row"><dt>Created</dt><dd>{created}</dd></div>
        <div class="meta-row"><dt>Updated</dt><dd>{updated}</dd></div>
      </dl>
    </aside>
  </div>
</main>
</body>
</html>"#,
        title = html_escape(&session.title),
        provider = html_escape(&session.provider.to_string()),
        id = html_escape(&session.id),
        cwd = html_escape(&session.cwd),
        model = html_escape(session.model.as_deref().unwrap_or("-")),
        created = html_escape(&format_time(session.created_at_ms)),
        updated = html_escape(&format_time(session.updated_at_ms)),
        first_prompt = first_prompt,
        messages = messages
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderedTranscript {
    first_prompt: Option<String>,
    entries: Vec<RenderedEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderedEntry {
    role: String,
    display_role: String,
    timestamp_ms: Option<i64>,
    blocks: Vec<RenderedBlock>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RenderedBlock {
    Text(String),
    Preformatted {
        kind: &'static str,
        label: &'static str,
        text: String,
    },
}

fn render_transcript(session: &Session) -> RenderedTranscript {
    let first_prompt = session
        .first_user_message
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_string);
    let mut skipped_prompt = false;
    let mut entries = Vec::new();

    for message in &session.transcript {
        if !skipped_prompt
            && message.role == "user"
            && first_prompt
                .as_deref()
                .is_some_and(|prompt| prompt == message.text.trim())
        {
            skipped_prompt = true;
            continue;
        }

        entries.push(render_entry_model(message));
    }

    RenderedTranscript {
        first_prompt,
        entries,
    }
}

fn render_entry_model(message: &ChatMessage) -> RenderedEntry {
    let blocks = render_blocks(&message.text);
    let role = entry_role(&message.role, &blocks);
    let display_role = match role.as_str() {
        "assistant" => "assistant",
        "context" => "context",
        "event" => "event",
        "user" => "user",
        _ => role.as_str(),
    }
    .to_string();

    RenderedEntry {
        role,
        display_role,
        timestamp_ms: message.timestamp_ms,
        blocks,
    }
}

fn entry_role(source_role: &str, blocks: &[RenderedBlock]) -> String {
    if blocks.iter().any(|block| {
        matches!(
            block,
            RenderedBlock::Preformatted {
                kind: "environment-context",
                ..
            }
        )
    }) {
        return "context".to_string();
    }

    if blocks.iter().any(|block| {
        matches!(
            block,
            RenderedBlock::Preformatted {
                kind: "subagent-notification",
                ..
            }
        )
    }) {
        return "event".to_string();
    }

    match source_role {
        "assistant" | "user" => source_role.to_string(),
        "system" | "tool" | "developer" => "event".to_string(),
        other if other.trim().is_empty() => "event".to_string(),
        other => other.to_string(),
    }
}

fn render_blocks(text: &str) -> Vec<RenderedBlock> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![RenderedBlock::Text(String::new())];
    }

    if looks_like_environment_context(trimmed) {
        return vec![RenderedBlock::Preformatted {
            kind: "environment-context",
            label: "environment context",
            text: text.to_string(),
        }];
    }

    if looks_like_subagent_notification(trimmed) {
        return vec![RenderedBlock::Preformatted {
            kind: "subagent-notification",
            label: "subagent notification",
            text: text.to_string(),
        }];
    }

    if looks_preformatted(trimmed) {
        return vec![RenderedBlock::Preformatted {
            kind: "preformatted",
            label: "preformatted",
            text: text.to_string(),
        }];
    }

    vec![RenderedBlock::Text(text.to_string())]
}

fn looks_like_environment_context(trimmed: &str) -> bool {
    trimmed.starts_with("<environment_context>")
        || trimmed.contains("<environment_context>")
        || trimmed.starts_with("# AGENTS.md instructions")
}

fn looks_like_subagent_notification(trimmed: &str) -> bool {
    trimmed.starts_with("<subagent_notification>") || trimmed.contains("<subagent_notification>")
}

fn looks_preformatted(trimmed: &str) -> bool {
    looks_jsonish(trimmed)
        || trimmed.contains("```")
        || trimmed.lines().count() >= 14
        || trimmed
            .lines()
            .any(|line| line.starts_with("    ") || line.starts_with('\t'))
}

fn looks_jsonish(trimmed: &str) -> bool {
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn render_first_prompt(prompt: &str) -> String {
    format!(
        r#"<section class="prompt-panel" aria-label="First prompt">
        <div class="prompt-label"><strong>user</strong><span>first prompt</span></div>
        <div class="prompt-body"><div class="prompt-text">{}</div></div>
      </section>"#,
        render_text(prompt)
    )
}

fn render_entries(entries: &[RenderedEntry]) -> String {
    if entries.is_empty() {
        return r#"<p class="empty">No transcript text was reconstructed.</p>"#.to_string();
    }

    entries
        .iter()
        .map(render_entry)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_entry(entry: &RenderedEntry) -> String {
    let blocks = entry
        .blocks
        .iter()
        .map(render_block)
        .collect::<Vec<_>>()
        .join("\n");
    let time = format_time(entry.timestamp_ms);
    format!(
        r#"<article class="message {role}">
        <aside class="rail"><span class="role">{display_role}</span><span class="time">{time}</span></aside>
        <div class="body">{blocks}</div>
      </article>"#,
        role = html_attr(&entry.role),
        display_role = html_escape(&entry.display_role),
        time = html_escape(&time),
        blocks = blocks
    )
}

fn render_block(block: &RenderedBlock) -> String {
    match block {
        RenderedBlock::Text(text) => render_text(text),
        RenderedBlock::Preformatted { kind, label, text } if *kind == "preformatted" => {
            format!(
                r#"<pre class="code-block" aria-label="{label}">{text}</pre>"#,
                label = html_escape(label),
                text = html_escape(text)
            )
        }
        RenderedBlock::Preformatted { kind, label, text } => {
            format!(
                r#"<div class="structured-block {kind}">
          <div class="block-head"><span>{label}</span></div>
          <div class="block-body">{text}</div>
        </div>"#,
                kind = html_attr(kind),
                label = html_escape(label),
                text = html_escape(text)
            )
        }
    }
}

fn render_text(text: &str) -> String {
    let paragraphs = text
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| format!("<p>{}</p>", html_escape(paragraph).replace('\n', "<br>")))
        .collect::<Vec<_>>();

    if paragraphs.is_empty() {
        "<p></p>".to_string()
    } else {
        paragraphs.join("\n")
    }
}

fn simple_response(status: u16, reason: &'static str, message: &str) -> HttpResponse {
    HttpResponse {
        status,
        reason,
        content_type: "text/plain; charset=utf-8",
        body: message.to_string(),
    }
}

fn format_time(timestamp_ms: Option<i64>) -> String {
    let Some(timestamp_ms) = timestamp_ms else {
        return "-".to_string();
    };
    let Some(dt) = DateTime::from_timestamp_millis(timestamp_ms) else {
        return "-".to_string();
    };
    dt.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn html_attr(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
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
    use crate::model::{ChatMessage, SessionOrigin};

    #[test]
    fn builds_share_url_with_encoded_id_and_token() {
        let mut session = session(ProviderKind::Codex, "id with/slash", "title");
        session.id = "id with/slash".to_string();

        assert_eq!(
            build_share_url("http://host:8787/", "tok en/1", &session),
            "http://host:8787/s/codex/id%20with%2Fslash?token=tok%20en%2F1"
        );
    }

    #[test]
    fn renders_valid_session_route() {
        let sessions = vec![session(ProviderKind::Claude, "sid", "<title>")];
        let response = route_request("GET", "/s/claude/sid?token=secret", "secret", &sessions);

        assert_eq!(response.status, 200);
        assert!(response.body.contains("&lt;title&gt;"));
        assert!(response.body.contains("hello &lt;world&gt;"));
        assert!(!response.body.contains("source_path"));
    }

    #[test]
    fn renders_first_prompt_separately_without_duplication() {
        let mut session = session(ProviderKind::Codex, "sid", "short title");
        session.first_user_message = Some("hello <world>".to_string());
        session.transcript = vec![
            ChatMessage {
                role: "user".to_string(),
                text: "hello <world>".to_string(),
                timestamp_ms: Some(1),
            },
            ChatMessage {
                role: "assistant".to_string(),
                text: "answer".to_string(),
                timestamp_ms: Some(2),
            },
        ];

        let body = render_session_html(&session);

        assert!(body.contains("first prompt"));
        assert_eq!(body.matches("hello &lt;world&gt;").count(), 1);
        assert!(body.contains(r#"class="message assistant""#));
        assert!(body.contains("<p>answer</p>"));
    }

    #[test]
    fn keeps_context_like_first_message_in_transcript() {
        let mut session = session(ProviderKind::Codex, "sid", "short title");
        session.first_user_message = Some("real prompt".to_string());
        session.transcript = vec![ChatMessage {
            role: "user".to_string(),
            text: "<environment_context>\n<cwd>/tmp/work</cwd>\n</environment_context>".to_string(),
            timestamp_ms: Some(1),
        }];

        let body = render_session_html(&session);

        assert!(body.contains(r#"class="message context""#));
        assert!(body.contains(r#"structured-block environment-context"#));
        assert!(body.contains("environment context"));
        assert!(body.contains("&lt;cwd&gt;/tmp/work&lt;/cwd&gt;"));
    }

    #[test]
    fn renders_subagent_notification_as_structured_event() {
        let mut session = session(ProviderKind::Codex, "sid", "short title");
        session.first_user_message = Some("real prompt".to_string());
        session.transcript = vec![ChatMessage {
            role: "assistant".to_string(),
            text: "<subagent_notification>\nfinished\n</subagent_notification>".to_string(),
            timestamp_ms: Some(1),
        }];

        let body = render_session_html(&session);

        assert!(body.contains(r#"class="message event""#));
        assert!(body.contains(r#"structured-block subagent-notification"#));
        assert!(body.contains("subagent notification"));
    }

    #[test]
    fn escapes_title_attribute() {
        let body = render_session_html(&session(
            ProviderKind::Codex,
            "sid",
            r#"quote "title" <tag>"#,
        ));

        assert!(body.contains(r#"title="quote &quot;title&quot; &lt;tag&gt;""#));
    }

    #[test]
    fn rejects_missing_or_invalid_token() {
        let sessions = vec![session(ProviderKind::Codex, "sid", "title")];

        assert_eq!(
            route_request("GET", "/s/codex/sid", "secret", &sessions).status,
            401
        );
        assert_eq!(
            route_request("GET", "/s/codex/sid?token=bad", "secret", &sessions).status,
            401
        );
    }

    #[test]
    fn returns_not_found_for_unknown_provider_or_session() {
        let sessions = vec![session(ProviderKind::Codex, "sid", "title")];

        assert_eq!(
            route_request("GET", "/s/unknown/sid?token=secret", "secret", &sessions).status,
            404
        );
        assert_eq!(
            route_request("GET", "/s/codex/missing?token=secret", "secret", &sessions).status,
            404
        );
    }

    #[test]
    fn rejects_unsupported_methods() {
        let sessions = vec![session(ProviderKind::Codex, "sid", "title")];

        assert_eq!(
            route_request("POST", "/s/codex/sid?token=secret", "secret", &sessions).status,
            405
        );
    }

    fn session(provider: ProviderKind, id: &str, title: &str) -> Session {
        Session {
            origin: SessionOrigin::Local,
            provider,
            id: id.to_string(),
            title: title.to_string(),
            cwd: "/tmp/work".to_string(),
            created_at_ms: Some(1),
            updated_at_ms: Some(2),
            model: Some("model".to_string()),
            source_path: "/tmp/session".into(),
            first_user_message: Some(title.to_string()),
            transcript: vec![ChatMessage {
                role: "user".to_string(),
                text: "hello <world>".to_string(),
                timestamp_ms: Some(1),
            }],
            resume_program: provider.to_string(),
            resume_args: vec!["resume".to_string(), id.to_string()],
        }
    }
}
