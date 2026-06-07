use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::Result;
use serde_json::Value;
use walkdir::WalkDir;

use crate::model::{truncate_for_title, ChatMessage, ProviderKind, Session};

pub fn load_claude_sessions(claude_home: &Path) -> Result<Vec<Session>> {
    let projects = claude_home.join("projects");
    if !projects.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in WalkDir::new(projects)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        if path
            .components()
            .any(|part| part.as_os_str() == "subagents")
        {
            continue;
        }
        if let Some(session) = parse_claude_session(path)? {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

fn parse_claude_session(path: &Path) -> Result<Option<Session>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut session_id = None;
    let mut cwd = None;
    let mut version = None;
    let mut model = None;
    let mut git_branch = None;
    let mut first_user_message = None;
    let mut last_prompt = None;
    let mut created_at_ms = None;
    let mut updated_at_ms = None;
    let mut messages = Vec::new();
    let mut is_sidechain = false;

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if value
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            is_sidechain = true;
        }

        if session_id.is_none() {
            session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if cwd.is_none() {
            cwd = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if version.is_none() {
            version = value
                .get("version")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if git_branch.is_none() {
            git_branch = value
                .get("gitBranch")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }

        if value.get("type").and_then(Value::as_str) == Some("last-prompt") {
            if let Some(prompt) = value.get("lastPrompt").and_then(Value::as_str) {
                last_prompt = non_empty(prompt.to_string());
            }
            continue;
        }

        let timestamp_ms = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms);
        if let Some(ts) = timestamp_ms {
            created_at_ms = Some(created_at_ms.map_or(ts, |current: i64| current.min(ts)));
            updated_at_ms = Some(updated_at_ms.map_or(ts, |current: i64| current.max(ts)));
        }

        let Some(message) = extract_claude_message(&value) else {
            continue;
        };
        if model.is_none() {
            model = message.model.clone();
        }
        if message.role == "user" && first_user_message.is_none() {
            first_user_message = Some(message.text.clone());
        }
        messages.push(ChatMessage {
            role: message.role,
            text: message.text,
            timestamp_ms,
        });
    }

    if is_sidechain {
        return Ok(None);
    }

    let Some(id) = session_id.or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
    }) else {
        return Ok(None);
    };

    let first_user_message = first_user_message.or(last_prompt);
    let title = first_user_message
        .as_deref()
        .map(|text| truncate_for_title(text, 80))
        .unwrap_or_else(|| id.clone());
    let model = model.or(version).map(|model_or_version| {
        if let Some(branch) = git_branch {
            format!("{model_or_version} · {branch}")
        } else {
            model_or_version
        }
    });

    Ok(Some(Session {
        provider: ProviderKind::Claude,
        id: id.clone(),
        title,
        cwd: cwd.unwrap_or_default(),
        created_at_ms,
        updated_at_ms,
        model,
        source_path: path.to_path_buf(),
        first_user_message,
        transcript: messages,
        resume_program: "claude".to_string(),
        resume_args: vec!["--resume".to_string(), id],
    }))
}

struct ExtractedMessage {
    role: String,
    text: String,
    model: Option<String>,
}

fn extract_claude_message(value: &Value) -> Option<ExtractedMessage> {
    let role = value.get("type")?.as_str()?.to_string();
    if role != "user" && role != "assistant" {
        return None;
    }

    let message = value.get("message")?;
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let text = message
        .get("content")
        .and_then(extract_content_text)
        .or_else(|| value.get("content").and_then(extract_content_text))?;

    Some(ExtractedMessage { role, text, model })
}

fn extract_content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => non_empty(text.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.as_str()),
                    Value::Object(_) => item.get("text").and_then(Value::as_str),
                    _ => None,
                })
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>();
            non_empty(parts.join("\n"))
        }
        _ => None,
    }
}

fn parse_rfc3339_ms(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn non_empty(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_claude_session() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"parentUuid":null,"isSidechain":false,"type":"user","message":{{"role":"user","content":"hello"}},"timestamp":"2026-05-18T12:45:36.896Z","cwd":"/tmp/work","sessionId":"sid","version":"2.1.131","gitBranch":"main"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"model":"claude","content":[{{"type":"text","text":"answer"}}]}},"timestamp":"2026-05-18T12:45:37.896Z","sessionId":"sid"}}"#
        )
        .unwrap();

        let session = parse_claude_session(&path).unwrap().unwrap();
        assert_eq!(session.id, "sid");
        assert_eq!(session.cwd, "/tmp/work");
        assert_eq!(session.title, "hello");
        assert_eq!(session.model.as_deref(), Some("claude · main"));
        assert_eq!(session.resume_args, vec!["--resume", "sid"]);
    }

    #[test]
    fn keeps_full_claude_transcript() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = File::create(&path).unwrap();
        for idx in 0..8 {
            writeln!(
                file,
                r#"{{"type":"user","message":{{"role":"user","content":"prompt {idx}"}},"timestamp":"2026-05-18T12:45:36.896Z","cwd":"/tmp/work","sessionId":"sid"}}"#
            )
            .unwrap();
        }

        let session = parse_claude_session(&path).unwrap().unwrap();
        assert_eq!(session.transcript.len(), 8);
        assert_eq!(session.transcript[0].text, "prompt 0");
        assert_eq!(session.transcript[7].text, "prompt 7");
    }

    #[test]
    fn excludes_subagents_and_sidechains() {
        let dir = tempdir().unwrap();
        let projects = dir.path().join("projects").join("project");
        fs::create_dir_all(projects.join("subagents")).unwrap();
        let main = projects.join("main.jsonl");
        let side = projects.join("side.jsonl");
        let sub = projects.join("subagents").join("sub.jsonl");
        fs::write(
            &main,
            r#"{"type":"last-prompt","lastPrompt":"main","sessionId":"main"}"#,
        )
        .unwrap();
        fs::write(
            &side,
            r#"{"isSidechain":true,"type":"user","message":{"content":"side"},"sessionId":"side"}"#,
        )
        .unwrap();
        fs::write(
            &sub,
            r#"{"type":"last-prompt","lastPrompt":"sub","sessionId":"sub"}"#,
        )
        .unwrap();

        let sessions = load_claude_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "main");
    }
}
