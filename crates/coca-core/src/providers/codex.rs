use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::model::{truncate_for_title, ChatMessage, ProviderKind, Session, SessionOrigin};

pub fn load_codex_sessions(codex_home: &Path) -> Result<Vec<Session>> {
    let db_path = codex_home.join("state_5.sqlite");
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut stmt = conn.prepare(
        "select id, rollout_path, created_at_ms, updated_at_ms, cwd, title, model, \
         first_user_message, preview \
         from threads \
         where archived = 0 and source = 'cli' \
         order by coalesce(updated_at_ms, updated_at * 1000) desc",
    )?;

    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let rollout_path: String = row.get(1)?;
        let created_at_ms: Option<i64> = row.get(2)?;
        let updated_at_ms: Option<i64> = row.get(3)?;
        let cwd: String = row.get(4)?;
        let title: String = row.get(5)?;
        let model: Option<String> = row.get(6)?;
        let first_user_message: String = row.get(7)?;
        let preview: String = row.get(8)?;
        Ok((
            id,
            rollout_path,
            created_at_ms,
            updated_at_ms,
            cwd,
            title,
            model,
            first_user_message,
            preview,
        ))
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let (
            id,
            rollout_path,
            created_at_ms,
            updated_at_ms,
            cwd,
            title,
            model,
            db_first_user_message,
            preview,
        ) = row?;
        let source_path = Path::new(&rollout_path).to_path_buf();
        let transcript = parse_codex_rollout(&source_path).unwrap_or_default();
        let first_user_message = non_empty(db_first_user_message).or(transcript.first_user_message);
        let mut messages = transcript.messages;

        if messages.is_empty() {
            if let Some(text) = non_empty(preview.clone()) {
                messages.push(ChatMessage {
                    role: "preview".to_string(),
                    text,
                    timestamp_ms: updated_at_ms,
                });
            }
        }

        let display_title = if title.trim().is_empty() {
            first_user_message
                .as_deref()
                .map(|text| truncate_for_title(text, 80))
                .unwrap_or_else(|| id.clone())
        } else {
            title
        };

        sessions.push(Session {
            origin: SessionOrigin::Local,
            provider: ProviderKind::Codex,
            id: id.clone(),
            title: display_title,
            cwd,
            created_at_ms,
            updated_at_ms,
            model,
            source_path,
            first_user_message,
            transcript: messages,
            resume_program: "codex".to_string(),
            resume_args: vec!["resume".to_string(), id],
        });
    }

    Ok(sessions)
}

#[derive(Default)]
struct CodexTranscript {
    first_user_message: Option<String>,
    messages: Vec<ChatMessage>,
}

fn parse_codex_rollout(path: &Path) -> Result<CodexTranscript> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut first_user_message = None;
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let timestamp_ms = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms);
        let Some(message) = extract_codex_message(&value) else {
            continue;
        };

        if message.role == "user"
            && message
                .text
                .trim_start()
                .starts_with("<environment_context>")
        {
            continue;
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

    Ok(CodexTranscript {
        first_user_message,
        messages,
    })
}

struct ExtractedMessage {
    role: String,
    text: String,
}

fn extract_codex_message(value: &Value) -> Option<ExtractedMessage> {
    if value.get("type")?.as_str()? != "response_item" {
        return None;
    }
    let payload = value.get("payload")?;
    if payload.get("type")?.as_str()? != "message" {
        return None;
    }
    let role = payload.get("role")?.as_str()?.to_string();
    if role != "user" && role != "assistant" {
        return None;
    }
    let text = extract_content_text(payload.get("content")?)?;
    Some(ExtractedMessage { role, text })
}

fn extract_content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => non_empty(text.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .or_else(|| item.get("input_text"))
                        .and_then(Value::as_str)
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
    use std::io::Write;

    use rusqlite::params;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_codex_rollout_messages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-06-07T03:57:13.721Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"first prompt"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-06-07T03:57:14.721Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"answer"}}]}}}}"#
        )
        .unwrap();

        let transcript = parse_codex_rollout(&path).unwrap();
        assert_eq!(
            transcript.first_user_message.as_deref(),
            Some("first prompt")
        );
        assert_eq!(transcript.messages.len(), 2);
        assert_eq!(transcript.messages[1].text, "answer");
    }

    #[test]
    fn keeps_full_codex_transcript() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let mut file = File::create(&path).unwrap();
        for idx in 0..8 {
            writeln!(
                file,
                r#"{{"timestamp":"2026-06-07T03:57:13.721Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"prompt {idx}"}}]}}}}"#
            )
            .unwrap();
        }

        let transcript = parse_codex_rollout(&path).unwrap();
        assert_eq!(transcript.messages.len(), 8);
        assert_eq!(transcript.messages[0].text, "prompt 0");
        assert_eq!(transcript.messages[7].text, "prompt 7");
    }

    #[test]
    fn loads_codex_sessions_from_sqlite() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("state_5.sqlite");
        let rollout_path = dir.path().join("rollout.jsonl");
        File::create(&rollout_path).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "create table threads (
                id text primary key,
                rollout_path text not null,
                created_at_ms integer,
                updated_at_ms integer,
                cwd text not null,
                title text not null,
                model text,
                first_user_message text not null default '',
                preview text not null default '',
                archived integer not null default 0,
                source text not null,
                updated_at integer not null default 0
            );",
        )
        .unwrap();
        conn.execute(
            "insert into threads values (?1, ?2, 1, 2, '/tmp/work', '', 'gpt', 'hello', 'preview', 0, 'cli', 2)",
            params!["abc", rollout_path.to_string_lossy()],
        )
        .unwrap();

        let sessions = load_codex_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider, ProviderKind::Codex);
        assert_eq!(sessions[0].title, "hello");
        assert_eq!(sessions[0].resume_args, vec!["resume", "abc"]);
    }
}
