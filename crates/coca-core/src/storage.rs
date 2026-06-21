use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::Session;

const SCHEMA_VERSION: i64 = 1;

pub fn default_database_path() -> Option<PathBuf> {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .map(|dir| dir.join("coca").join("coca.sqlite3"))
}

pub struct DerivedStore {
    conn: Connection,
}

impl DerivedStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create storage directory {}", parent.display())
            })?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open storage database {}", path.display()))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let store = Self {
            conn: Connection::open_in_memory().context("failed to open in-memory storage")?,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn replace_sessions(&mut self, sessions: &[Session]) -> Result<()> {
        let refreshed_at_ms = now_ms();
        let tx = self
            .conn
            .transaction()
            .context("failed to start session storage transaction")?;
        tx.execute("DELETE FROM sessions", [])
            .context("failed to clear stored sessions")?;
        {
            let mut stmt = tx
                .prepare(
                    r#"
                    INSERT INTO sessions (
                        origin, provider, id, payload_json, title, cwd,
                        created_at_ms, updated_at_ms, model, source_path,
                        first_user_message, transcript_payload,
                        resume_program, resume_args_json, source_hash, refreshed_at_ms
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                    "#,
                )
                .context("failed to prepare session upsert")?;
            for session in sessions {
                let payload_json =
                    serde_json::to_string(session).context("failed to encode session payload")?;
                let transcript_payload = serde_json::to_string(&session.transcript)
                    .context("failed to encode transcript for storage")?;
                let resume_args_json = serde_json::to_string(&session.resume_args)
                    .context("failed to encode resume args for storage")?;
                stmt.execute(params![
                    session.origin.to_string(),
                    session.provider.to_string(),
                    &session.id,
                    payload_json,
                    &session.title,
                    &session.cwd,
                    session.created_at_ms,
                    session.updated_at_ms,
                    &session.model,
                    session.source_path.to_string_lossy().to_string(),
                    &session.first_user_message,
                    transcript_payload,
                    &session.resume_program,
                    resume_args_json,
                    source_hash(session)?,
                    refreshed_at_ms,
                ])
                .context("failed to store session")?;
            }
        }
        tx.commit()
            .context("failed to commit session storage transaction")
    }

    pub fn session_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                let count: i64 = row.get(0)?;
                Ok(count as usize)
            })
            .context("failed to count stored sessions")
    }

    pub fn sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT payload_json, transcript_payload
                FROM sessions
                ORDER BY updated_at_ms DESC
                "#,
            )
            .context("failed to prepare stored sessions query")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("failed to query stored sessions")?;
        let mut sessions = Vec::new();
        for row in rows {
            let (payload_json, transcript_payload) =
                row.context("failed to read stored session")?;
            let mut session: Session = serde_json::from_str(&payload_json)
                .context("failed to decode stored session payload")?;
            session.transcript = serde_json::from_str(&transcript_payload)
                .context("failed to decode stored transcript payload")?;
            sessions.push(session);
        }
        Ok(sessions)
    }

    pub fn session_for(
        &self,
        origin: &str,
        provider: &str,
        id: &str,
    ) -> Result<Option<StoredSession>> {
        self.conn
            .query_row(
                r#"
                SELECT payload_json, transcript_payload, source_hash, refreshed_at_ms
                FROM sessions
                WHERE origin = ?1 AND provider = ?2 AND id = ?3
                "#,
                params![origin, provider, id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .context("failed to load stored session")?
            .map(
                |(payload_json, transcript_payload, source_hash, refreshed_at_ms)| {
                    let mut session: Session = serde_json::from_str(&payload_json)
                        .context("failed to decode stored session payload")?;
                    session.transcript = serde_json::from_str(&transcript_payload)
                        .context("failed to decode stored transcript payload")?;
                    Ok(StoredSession {
                        session,
                        source_hash,
                        refreshed_at_ms,
                    })
                },
            )
            .transpose()
    }

    pub fn summary_for(
        &self,
        origin: &str,
        provider: &str,
        id: &str,
        source_hash: &str,
        generator: &str,
    ) -> Result<Option<StoredSessionSummary>> {
        self.conn
            .query_row(
                r#"
                SELECT title, summary, updated_at_ms
                FROM session_summaries
                WHERE origin = ?1 AND provider = ?2 AND id = ?3 AND source_hash = ?4 AND generator = ?5
                "#,
                params![origin, provider, id, source_hash, generator],
                |row| {
                    Ok(StoredSessionSummary {
                        title: row.get(0)?,
                        summary: row.get(1)?,
                        updated_at_ms: row.get(2)?,
                    })
                },
            )
            .optional()
            .context("failed to load stored session summary")
    }

    pub fn upsert_summary(&self, summary: SessionSummaryInput<'_>) -> Result<()> {
        let updated_at_ms = now_ms();
        self.conn
            .execute(
                r#"
                INSERT INTO session_summaries (
                    origin, provider, id, source_hash, generator, title, summary, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(origin, provider, id, source_hash, generator)
                DO UPDATE SET title = excluded.title, summary = excluded.summary, updated_at_ms = excluded.updated_at_ms
                "#,
                params![
                    summary.origin,
                    summary.provider,
                    summary.id,
                    summary.source_hash,
                    summary.generator,
                    summary.title,
                    summary.summary,
                    updated_at_ms
                ],
            )
            .context("failed to store session summary")?;
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA foreign_keys = ON")
            .context("failed to enable storage foreign keys")?;
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .context("failed to read storage schema version")?;

        match version {
            0 => self
                .conn
                .execute_batch(
                    r#"
                    CREATE TABLE sessions (
                        origin TEXT NOT NULL,
                        provider TEXT NOT NULL,
                        id TEXT NOT NULL,
                        payload_json TEXT NOT NULL,
                        title TEXT NOT NULL,
                        cwd TEXT NOT NULL,
                        created_at_ms INTEGER,
                        updated_at_ms INTEGER,
                        model TEXT,
                        source_path TEXT NOT NULL,
                        first_user_message TEXT,
                        transcript_payload TEXT NOT NULL,
                        resume_program TEXT NOT NULL,
                        resume_args_json TEXT NOT NULL,
                        source_hash TEXT NOT NULL,
                        refreshed_at_ms INTEGER NOT NULL,
                        PRIMARY KEY(origin, provider, id)
                    );

                    CREATE INDEX idx_sessions_updated_at
                        ON sessions(updated_at_ms DESC);

                    CREATE TABLE session_summaries (
                        origin TEXT NOT NULL,
                        provider TEXT NOT NULL,
                        id TEXT NOT NULL,
                        source_hash TEXT NOT NULL,
                        generator TEXT NOT NULL,
                        title TEXT,
                        summary TEXT,
                        updated_at_ms INTEGER NOT NULL,
                        PRIMARY KEY(origin, provider, id, source_hash, generator)
                    );

                    PRAGMA user_version = 1;
                    "#,
                )
                .context("failed to migrate storage schema"),
            SCHEMA_VERSION => Ok(()),
            newer if newer > SCHEMA_VERSION => bail!(
                "storage schema version {newer} is newer than supported version {SCHEMA_VERSION}"
            ),
            older => bail!("unsupported storage schema version {older}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSession {
    pub session: Session,
    pub source_hash: String,
    pub refreshed_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSessionSummary {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSummaryInput<'a> {
    pub origin: &'a str,
    pub provider: &'a str,
    pub id: &'a str,
    pub source_hash: &'a str,
    pub generator: &'a str,
    pub title: Option<&'a str>,
    pub summary: Option<&'a str>,
}

pub fn source_hash(session: &Session) -> Result<String> {
    let json = serde_json::to_string(session).context("failed to encode session for hashing")?;
    Ok(format!("{:016x}", fnv1a64(json.as_bytes())))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChatMessage, ProviderKind, SessionOrigin};

    #[test]
    fn default_database_path_is_coca_owned_sqlite() {
        let path = default_database_path().expect("default database path");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("coca.sqlite3")
        );
        assert_eq!(
            path.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str()),
            Some("coca")
        );
    }

    #[test]
    fn migrates_with_user_version_and_roundtrips_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join("coca.sqlite3");
        let mut store = DerivedStore::open(&path).unwrap();
        let sessions = vec![session("one"), session("two")];
        let expected_hash = source_hash(&sessions[0]).unwrap();

        store.replace_sessions(&sessions).unwrap();

        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let stored = store
            .session_for("local", "codex", "one")
            .unwrap()
            .expect("stored session");

        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(store.session_count().unwrap(), 2);
        assert_eq!(stored.session, sessions[0]);
        assert_eq!(stored.source_hash, expected_hash);
        assert_eq!(store.sessions().unwrap(), sessions);
    }

    #[test]
    fn replace_sessions_clears_previous_derived_rows() {
        let mut store = DerivedStore::open_in_memory().unwrap();

        store
            .replace_sessions(&[session("one"), session("two")])
            .unwrap();
        store.replace_sessions(&[session("one")]).unwrap();

        assert_eq!(store.session_count().unwrap(), 1);
        assert!(store
            .session_for("local", "codex", "two")
            .unwrap()
            .is_none());
    }

    #[test]
    fn summary_cache_is_keyed_by_source_hash_and_generator() {
        let store = DerivedStore::open_in_memory().unwrap();
        let session = session("one");
        let hash = source_hash(&session).unwrap();

        store
            .upsert_summary(SessionSummaryInput {
                origin: "local",
                provider: "codex",
                id: "one",
                source_hash: &hash,
                generator: "ai:v1",
                title: Some("title"),
                summary: Some("summary"),
            })
            .unwrap();

        let stored = store
            .summary_for("local", "codex", "one", &hash, "ai:v1")
            .unwrap()
            .unwrap();
        assert_eq!(stored.title.as_deref(), Some("title"));
        assert_eq!(stored.summary.as_deref(), Some("summary"));
        assert!(store
            .summary_for("local", "codex", "one", "different", "ai:v1")
            .unwrap()
            .is_none());
    }

    fn session(id: &str) -> Session {
        Session {
            origin: SessionOrigin::Local,
            provider: ProviderKind::Codex,
            id: id.to_string(),
            title: format!("session {id}"),
            cwd: "/tmp".to_string(),
            created_at_ms: Some(1),
            updated_at_ms: Some(2),
            model: Some("model".to_string()),
            source_path: format!("/tmp/{id}.jsonl").into(),
            first_user_message: Some("prompt".to_string()),
            transcript: vec![ChatMessage {
                role: "user".to_string(),
                text: "prompt".to_string(),
                timestamp_ms: Some(1),
            }],
            resume_program: "codex".to_string(),
            resume_args: vec!["resume".to_string(), id.to_string()],
        }
    }
}
