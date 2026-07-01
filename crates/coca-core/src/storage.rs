use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::{distributions::Alphanumeric, Rng};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::model::Session;

const SCHEMA_VERSION: i64 = 3;

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

    pub fn user_count(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM auth_users", [], |row| {
                let count: i64 = row.get(0)?;
                Ok(count as usize)
            })
            .context("failed to count auth users")
    }

    pub fn create_user(&self, input: NewUser<'_>) -> Result<StoredUser> {
        let now = now_ms();
        let email = normalize_email(input.email);
        self.conn
            .execute(
                r#"
                INSERT INTO auth_users (
                    id, email, password_hash, display_name, created_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    input.id,
                    email,
                    input.password_hash,
                    input.display_name,
                    now,
                    now,
                ],
            )
            .context("failed to create auth user")?;
        self.user_by_id(input.id)?
            .context("created auth user was not found")
    }

    pub fn user_by_id(&self, id: &str) -> Result<Option<StoredUser>> {
        self.conn
            .query_row(
                r#"
                SELECT id, email, display_name, created_at_ms, updated_at_ms
                FROM auth_users
                WHERE id = ?1
                "#,
                params![id],
                stored_user_from_row,
            )
            .optional()
            .context("failed to load auth user")
    }

    pub fn user_by_email(&self, email: &str) -> Result<Option<StoredUserWithPassword>> {
        self.conn
            .query_row(
                r#"
                SELECT id, email, password_hash, display_name, created_at_ms, updated_at_ms
                FROM auth_users
                WHERE email = ?1
                "#,
                params![normalize_email(email)],
                stored_user_with_password_from_row,
            )
            .optional()
            .context("failed to load auth user by email")
    }

    pub fn update_user_profile(
        &self,
        user_id: &str,
        display_name: Option<&str>,
    ) -> Result<StoredUser> {
        let now = now_ms();
        self.conn
            .execute(
                r#"
                UPDATE auth_users
                SET display_name = ?2, updated_at_ms = ?3
                WHERE id = ?1
                "#,
                params![user_id, display_name, now],
            )
            .context("failed to update auth user profile")?;
        self.user_by_id(user_id)?.context("auth user was not found")
    }

    pub fn update_user_password_hash(&self, user_id: &str, password_hash: &str) -> Result<()> {
        let now = now_ms();
        self.conn
            .execute(
                r#"
                UPDATE auth_users
                SET password_hash = ?2, updated_at_ms = ?3
                WHERE id = ?1
                "#,
                params![user_id, password_hash, now],
            )
            .context("failed to update auth user password")?;
        Ok(())
    }

    pub fn create_device_session(
        &self,
        input: NewDeviceSession<'_>,
    ) -> Result<StoredDeviceSession> {
        let now = now_ms();
        self.conn
            .execute(
                r#"
                INSERT INTO auth_device_sessions (
                    id, user_id, token_hash, label, scopes_json, created_at_ms, last_seen_at_ms, revoked_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
                "#,
                params![
                    input.id,
                    input.user_id,
                    input.token_hash,
                    input.label,
                    input.scopes_json,
                    now,
                    now,
                ],
            )
            .context("failed to create auth device session")?;
        self.device_session_by_id(input.user_id, input.id)?
            .context("created auth device session was not found")
    }

    pub fn validate_device_session(
        &self,
        token_hash: &str,
    ) -> Result<Option<StoredAuthCredential>> {
        let Some((session, user)) = self
            .conn
            .query_row(
                r#"
                SELECT
                    s.id, s.user_id, s.label, s.created_at_ms, s.last_seen_at_ms, s.revoked_at_ms,
                    s.scopes_json,
                    u.id, u.email, u.display_name, u.created_at_ms, u.updated_at_ms
                FROM auth_device_sessions s
                JOIN auth_users u ON u.id = s.user_id
                WHERE s.token_hash = ?1 AND s.revoked_at_ms IS NULL
                "#,
                params![token_hash],
                |row| {
                    Ok((
                        StoredDeviceSession {
                            id: row.get(0)?,
                            user_id: row.get(1)?,
                            label: row.get(2)?,
                            created_at_ms: row.get(3)?,
                            last_seen_at_ms: row.get(4)?,
                            revoked_at_ms: row.get(5)?,
                            scopes_json: row.get(6)?,
                        },
                        StoredUser {
                            id: row.get(7)?,
                            email: row.get(8)?,
                            display_name: row.get(9)?,
                            created_at_ms: row.get(10)?,
                            updated_at_ms: row.get(11)?,
                        },
                    ))
                },
            )
            .optional()
            .context("failed to validate auth device session")?
        else {
            return Ok(None);
        };
        self.touch_device_session(&session.id)?;
        Ok(Some(StoredAuthCredential {
            user,
            credential_id: session.id,
            credential_kind: AuthCredentialKind::DeviceSession,
            scopes_json: session.scopes_json,
        }))
    }

    pub fn list_device_sessions(&self, user_id: &str) -> Result<Vec<StoredDeviceSession>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT id, user_id, label, created_at_ms, last_seen_at_ms, revoked_at_ms, scopes_json
                FROM auth_device_sessions
                WHERE user_id = ?1
                ORDER BY created_at_ms DESC
                "#,
            )
            .context("failed to prepare auth device sessions query")?;
        let rows = stmt
            .query_map(params![user_id], stored_device_session_from_row)
            .context("failed to query auth device sessions")?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read auth device sessions")
    }

    pub fn revoke_device_session(&self, user_id: &str, session_id: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                r#"
                UPDATE auth_device_sessions
                SET revoked_at_ms = ?3
                WHERE user_id = ?1 AND id = ?2 AND revoked_at_ms IS NULL
                "#,
                params![user_id, session_id, now_ms()],
            )
            .context("failed to revoke auth device session")?;
        Ok(changed > 0)
    }

    pub fn create_access_token(&self, input: NewAccessToken<'_>) -> Result<StoredAccessToken> {
        let now = now_ms();
        self.conn
            .execute(
                r#"
                INSERT INTO auth_access_tokens (
                    id, user_id, name, token_hash, scopes_json, created_at_ms, last_used_at_ms, revoked_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)
                "#,
                params![
                    input.id,
                    input.user_id,
                    input.name,
                    input.token_hash,
                    input.scopes_json,
                    now
                ],
            )
            .context("failed to create auth access token")?;
        self.access_token_by_id(input.user_id, input.id)?
            .context("created auth access token was not found")
    }

    pub fn validate_access_token(&self, token_hash: &str) -> Result<Option<StoredAuthCredential>> {
        let Some((token, user)) = self
            .conn
            .query_row(
                r#"
                SELECT
                    t.id, t.user_id, t.name, t.created_at_ms, t.last_used_at_ms, t.revoked_at_ms,
                    t.scopes_json,
                    u.id, u.email, u.display_name, u.created_at_ms, u.updated_at_ms
                FROM auth_access_tokens t
                JOIN auth_users u ON u.id = t.user_id
                WHERE t.token_hash = ?1 AND t.revoked_at_ms IS NULL
                "#,
                params![token_hash],
                |row| {
                    Ok((
                        StoredAccessToken {
                            id: row.get(0)?,
                            user_id: row.get(1)?,
                            name: row.get(2)?,
                            created_at_ms: row.get(3)?,
                            last_used_at_ms: row.get(4)?,
                            revoked_at_ms: row.get(5)?,
                            scopes_json: row.get(6)?,
                        },
                        StoredUser {
                            id: row.get(7)?,
                            email: row.get(8)?,
                            display_name: row.get(9)?,
                            created_at_ms: row.get(10)?,
                            updated_at_ms: row.get(11)?,
                        },
                    ))
                },
            )
            .optional()
            .context("failed to validate auth access token")?
        else {
            return Ok(None);
        };
        self.touch_access_token(&token.id)?;
        Ok(Some(StoredAuthCredential {
            user,
            credential_id: token.id,
            credential_kind: AuthCredentialKind::AccessToken,
            scopes_json: token.scopes_json,
        }))
    }

    pub fn list_access_tokens(&self, user_id: &str) -> Result<Vec<StoredAccessToken>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT id, user_id, name, created_at_ms, last_used_at_ms, revoked_at_ms, scopes_json
                FROM auth_access_tokens
                WHERE user_id = ?1
                ORDER BY created_at_ms DESC
                "#,
            )
            .context("failed to prepare auth access tokens query")?;
        let rows = stmt
            .query_map(params![user_id], stored_access_token_from_row)
            .context("failed to query auth access tokens")?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read auth access tokens")
    }

    pub fn revoke_access_token(&self, user_id: &str, token_id: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                r#"
                UPDATE auth_access_tokens
                SET revoked_at_ms = ?3
                WHERE user_id = ?1 AND id = ?2 AND revoked_at_ms IS NULL
                "#,
                params![user_id, token_id, now_ms()],
            )
            .context("failed to revoke auth access token")?;
        Ok(changed > 0)
    }

    pub fn create_share_link(&self, input: NewShareLink<'_>) -> Result<StoredShareLink> {
        let now = now_ms();
        self.conn
            .execute(
                r#"
                INSERT INTO auth_share_links (
                    id, creator_user_id, origin, provider, session_id, token_hash,
                    created_at_ms, last_used_at_ms, expires_at_ms, revoked_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL)
                "#,
                params![
                    input.id,
                    input.creator_user_id,
                    input.origin,
                    input.provider,
                    input.session_id,
                    input.token_hash,
                    now,
                    input.expires_at_ms,
                ],
            )
            .context("failed to create auth share link")?;
        self.share_link_by_id(input.id)?
            .context("created auth share link was not found")
    }

    pub fn validate_share_link(
        &self,
        share_id: &str,
        token_hash: &str,
    ) -> Result<Option<StoredShareLink>> {
        let Some(link) = self
            .conn
            .query_row(
                r#"
                SELECT
                    id, creator_user_id, origin, provider, session_id,
                    created_at_ms, last_used_at_ms, expires_at_ms, revoked_at_ms
                FROM auth_share_links
                WHERE id = ?1
                    AND token_hash = ?2
                    AND revoked_at_ms IS NULL
                    AND expires_at_ms > ?3
                "#,
                params![share_id, token_hash, now_ms()],
                stored_share_link_from_row,
            )
            .optional()
            .context("failed to validate auth share link")?
        else {
            return Ok(None);
        };
        self.touch_share_link(&link.id)?;
        Ok(Some(link))
    }

    pub fn list_share_links(&self, user_id: &str) -> Result<Vec<StoredShareLink>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT
                    id, creator_user_id, origin, provider, session_id,
                    created_at_ms, last_used_at_ms, expires_at_ms, revoked_at_ms
                FROM auth_share_links
                WHERE creator_user_id = ?1
                ORDER BY created_at_ms DESC
                "#,
            )
            .context("failed to prepare auth share links query")?;
        let rows = stmt
            .query_map(params![user_id], stored_share_link_from_row)
            .context("failed to query auth share links")?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read auth share links")
    }

    pub fn revoke_share_link(&self, user_id: &str, share_id: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                r#"
                UPDATE auth_share_links
                SET revoked_at_ms = ?3
                WHERE creator_user_id = ?1 AND id = ?2 AND revoked_at_ms IS NULL
                "#,
                params![user_id, share_id, now_ms()],
            )
            .context("failed to revoke auth share link")?;
        Ok(changed > 0)
    }

    fn device_session_by_id(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<StoredDeviceSession>> {
        self.conn
            .query_row(
                r#"
                SELECT id, user_id, label, created_at_ms, last_seen_at_ms, revoked_at_ms, scopes_json
                FROM auth_device_sessions
                WHERE user_id = ?1 AND id = ?2
                "#,
                params![user_id, session_id],
                stored_device_session_from_row,
            )
            .optional()
            .context("failed to load auth device session")
    }

    fn access_token_by_id(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<StoredAccessToken>> {
        self.conn
            .query_row(
                r#"
                SELECT id, user_id, name, created_at_ms, last_used_at_ms, revoked_at_ms, scopes_json
                FROM auth_access_tokens
                WHERE user_id = ?1 AND id = ?2
                "#,
                params![user_id, token_id],
                stored_access_token_from_row,
            )
            .optional()
            .context("failed to load auth access token")
    }

    fn share_link_by_id(&self, share_id: &str) -> Result<Option<StoredShareLink>> {
        self.conn
            .query_row(
                r#"
                SELECT
                    id, creator_user_id, origin, provider, session_id,
                    created_at_ms, last_used_at_ms, expires_at_ms, revoked_at_ms
                FROM auth_share_links
                WHERE id = ?1
                "#,
                params![share_id],
                stored_share_link_from_row,
            )
            .optional()
            .context("failed to load auth share link")
    }

    fn touch_device_session(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE auth_device_sessions SET last_seen_at_ms = ?2 WHERE id = ?1",
                params![session_id, now_ms()],
            )
            .context("failed to update auth device session last_seen")?;
        Ok(())
    }

    fn touch_access_token(&self, token_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE auth_access_tokens SET last_used_at_ms = ?2 WHERE id = ?1",
                params![token_id, now_ms()],
            )
            .context("failed to update auth access token last_used")?;
        Ok(())
    }

    fn touch_share_link(&self, share_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE auth_share_links SET last_used_at_ms = ?2 WHERE id = ?1",
                params![share_id, now_ms()],
            )
            .context("failed to update auth share link last_used")?;
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
            0 => {
                self.conn
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
                    "#,
                    )
                    .context("failed to migrate storage schema to version 1")?;
                self.migrate_auth_schema()
            }
            1 | 2 => self.migrate_auth_schema(),
            SCHEMA_VERSION => Ok(()),
            newer if newer > SCHEMA_VERSION => bail!(
                "storage schema version {newer} is newer than supported version {SCHEMA_VERSION}"
            ),
            older => bail!("unsupported storage schema version {older}"),
        }
    }

    fn migrate_auth_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
                DROP TABLE IF EXISTS auth_share_links;
                DROP TABLE IF EXISTS auth_access_tokens;
                DROP TABLE IF EXISTS auth_device_sessions;
                DROP TABLE IF EXISTS auth_users;
                "#,
            )
            .context("failed to reset auth schema")?;
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE auth_users (
                    id TEXT PRIMARY KEY,
                    email TEXT NOT NULL UNIQUE,
                    password_hash TEXT NOT NULL,
                    display_name TEXT,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE auth_device_sessions (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    token_hash TEXT NOT NULL UNIQUE,
                    label TEXT,
                    scopes_json TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    last_seen_at_ms INTEGER NOT NULL,
                    revoked_at_ms INTEGER,
                    FOREIGN KEY(user_id) REFERENCES auth_users(id) ON DELETE CASCADE
                );

                CREATE INDEX idx_auth_device_sessions_user
                    ON auth_device_sessions(user_id, revoked_at_ms, created_at_ms DESC);

                CREATE TABLE auth_access_tokens (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    token_hash TEXT NOT NULL UNIQUE,
                    scopes_json TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    last_used_at_ms INTEGER,
                    revoked_at_ms INTEGER,
                    FOREIGN KEY(user_id) REFERENCES auth_users(id) ON DELETE CASCADE
                );

                CREATE INDEX idx_auth_access_tokens_user
                    ON auth_access_tokens(user_id, revoked_at_ms, created_at_ms DESC);

                CREATE TABLE auth_share_links (
                    id TEXT PRIMARY KEY,
                    creator_user_id TEXT NOT NULL,
                    origin TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    token_hash TEXT NOT NULL UNIQUE,
                    created_at_ms INTEGER NOT NULL,
                    last_used_at_ms INTEGER,
                    expires_at_ms INTEGER NOT NULL,
                    revoked_at_ms INTEGER,
                    FOREIGN KEY(creator_user_id) REFERENCES auth_users(id) ON DELETE CASCADE
                );

                CREATE INDEX idx_auth_share_links_creator
                    ON auth_share_links(creator_user_id, revoked_at_ms, created_at_ms DESC);

                PRAGMA user_version = 3;
                "#,
            )
            .context("failed to migrate storage schema to version 3")
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredUser {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredUserWithPassword {
    pub user: StoredUser,
    pub password_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewUser<'a> {
    pub id: &'a str,
    pub email: &'a str,
    pub password_hash: &'a str,
    pub display_name: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredDeviceSession {
    pub id: String,
    pub user_id: String,
    pub label: Option<String>,
    pub created_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
    pub scopes_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewDeviceSession<'a> {
    pub id: &'a str,
    pub user_id: &'a str,
    pub token_hash: &'a str,
    pub label: Option<&'a str>,
    pub scopes_json: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAccessToken {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
    pub scopes_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewAccessToken<'a> {
    pub id: &'a str,
    pub user_id: &'a str,
    pub name: &'a str,
    pub token_hash: &'a str,
    pub scopes_json: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredShareLink {
    pub id: String,
    pub creator_user_id: String,
    pub origin: String,
    pub provider: String,
    pub session_id: String,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub expires_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewShareLink<'a> {
    pub id: &'a str,
    pub creator_user_id: &'a str,
    pub origin: &'a str,
    pub provider: &'a str,
    pub session_id: &'a str,
    pub token_hash: &'a str,
    pub expires_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthCredentialKind {
    DeviceSession,
    AccessToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAuthCredential {
    pub user: StoredUser,
    pub credential_id: String,
    pub credential_kind: AuthCredentialKind,
    pub scopes_json: String,
}

pub fn source_hash(session: &Session) -> Result<String> {
    let json = serde_json::to_string(session).context("failed to encode session for hashing")?;
    Ok(format!("{:016x}", fnv1a64(json.as_bytes())))
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| anyhow::anyhow!("failed to hash password: {err}"))
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn generate_token(prefix: &str) -> String {
    let secret = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect::<String>();
    format!("{prefix}_{secret}")
}

pub fn generate_id(prefix: &str) -> String {
    let secret = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .map(char::from)
        .collect::<String>();
    format!("{prefix}_{secret}")
}

pub fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn stored_user_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredUser> {
    Ok(StoredUser {
        id: row.get(0)?,
        email: row.get(1)?,
        display_name: row.get(2)?,
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
    })
}

fn stored_user_with_password_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredUserWithPassword> {
    Ok(StoredUserWithPassword {
        user: StoredUser {
            id: row.get(0)?,
            email: row.get(1)?,
            display_name: row.get(3)?,
            created_at_ms: row.get(4)?,
            updated_at_ms: row.get(5)?,
        },
        password_hash: row.get(2)?,
    })
}

fn stored_device_session_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredDeviceSession> {
    Ok(StoredDeviceSession {
        id: row.get(0)?,
        user_id: row.get(1)?,
        label: row.get(2)?,
        created_at_ms: row.get(3)?,
        last_seen_at_ms: row.get(4)?,
        revoked_at_ms: row.get(5)?,
        scopes_json: row.get(6)?,
    })
}

fn stored_access_token_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAccessToken> {
    Ok(StoredAccessToken {
        id: row.get(0)?,
        user_id: row.get(1)?,
        name: row.get(2)?,
        created_at_ms: row.get(3)?,
        last_used_at_ms: row.get(4)?,
        revoked_at_ms: row.get(5)?,
        scopes_json: row.get(6)?,
    })
}

fn stored_share_link_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredShareLink> {
    Ok(StoredShareLink {
        id: row.get(0)?,
        creator_user_id: row.get(1)?,
        origin: row.get(2)?,
        provider: row.get(3)?,
        session_id: row.get(4)?,
        created_at_ms: row.get(5)?,
        last_used_at_ms: row.get(6)?,
        expires_at_ms: row.get(7)?,
        revoked_at_ms: row.get(8)?,
    })
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

    #[test]
    fn auth_schema_migrates_with_user_version() {
        let store = DerivedStore::open_in_memory().unwrap();

        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let auth_tables: usize = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'auth_%'",
                [],
                |row| {
                    let count: i64 = row.get(0)?;
                    Ok(count as usize)
                },
            )
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(auth_tables, 4);
    }

    #[test]
    fn password_hashes_verify_without_storing_plaintext() {
        let password_hash = hash_password("correct horse battery staple").unwrap();

        assert_ne!(password_hash, "correct horse battery staple");
        assert!(password_hash.starts_with("$argon2"));
        assert!(verify_password(
            "correct horse battery staple",
            &password_hash
        ));
        assert!(!verify_password("wrong", &password_hash));
    }

    #[test]
    fn auth_users_normalize_email_and_enforce_unique_key() {
        let store = DerivedStore::open_in_memory().unwrap();
        let password_hash = hash_password("password").unwrap();

        let user = store
            .create_user(NewUser {
                id: "usr_1",
                email: " USER@Example.COM ",
                password_hash: &password_hash,
                display_name: Some("User"),
            })
            .unwrap();
        let duplicate = store.create_user(NewUser {
            id: "usr_2",
            email: "user@example.com",
            password_hash: &password_hash,
            display_name: None,
        });

        assert_eq!(user.email, "user@example.com");
        assert!(duplicate.is_err());
        assert_eq!(store.user_count().unwrap(), 1);
        assert_eq!(
            store
                .user_by_email(" user@example.com ")
                .unwrap()
                .unwrap()
                .user
                .id,
            "usr_1"
        );
    }

    #[test]
    fn device_sessions_store_hashes_and_revoke() {
        let store = auth_store_with_user();
        let token = generate_token("coca_sess");
        let hash = token_hash(&token);
        let session = store
            .create_device_session(NewDeviceSession {
                id: "dev_1",
                user_id: "usr_1",
                token_hash: &hash,
                label: Some("Browser"),
                scopes_json: r#"["sessions.read"]"#,
            })
            .unwrap();

        let valid = store
            .validate_device_session(&token_hash(&token))
            .unwrap()
            .unwrap();
        let rows = store.list_device_sessions("usr_1").unwrap();
        assert_eq!(session.id, "dev_1");
        assert_eq!(valid.credential_kind, AuthCredentialKind::DeviceSession);
        assert_eq!(valid.user.email, "user@example.com");
        assert_eq!(valid.scopes_json, r#"["sessions.read"]"#);
        assert_eq!(rows.len(), 1);
        assert!(!format!("{rows:?}").contains(&token));

        assert!(store.revoke_device_session("usr_1", "dev_1").unwrap());
        assert!(store
            .validate_device_session(&token_hash(&token))
            .unwrap()
            .is_none());
    }

    #[test]
    fn access_tokens_store_hashes_and_revoke() {
        let store = auth_store_with_user();
        let token = generate_token("coca_pat");
        let hash = token_hash(&token);
        let access = store
            .create_access_token(NewAccessToken {
                id: "tok_1",
                user_id: "usr_1",
                name: "CI",
                token_hash: &hash,
                scopes_json: r#"["sessions.read"]"#,
            })
            .unwrap();

        let valid = store
            .validate_access_token(&token_hash(&token))
            .unwrap()
            .unwrap();
        let rows = store.list_access_tokens("usr_1").unwrap();
        assert_eq!(access.id, "tok_1");
        assert_eq!(valid.credential_kind, AuthCredentialKind::AccessToken);
        assert_eq!(valid.user.id, "usr_1");
        assert_eq!(valid.scopes_json, r#"["sessions.read"]"#);
        assert_eq!(rows[0].name, "CI");
        assert!(!format!("{rows:?}").contains(&token));

        assert!(store.revoke_access_token("usr_1", "tok_1").unwrap());
        assert!(store
            .validate_access_token(&token_hash(&token))
            .unwrap()
            .is_none());
    }

    #[test]
    fn share_links_store_hashes_expire_and_revoke() {
        let store = auth_store_with_user();
        let token = generate_token("coca_share");
        let hash = token_hash(&token);
        let expires_at_ms = now_ms() + 60_000;
        let link = store
            .create_share_link(NewShareLink {
                id: "shr_1",
                creator_user_id: "usr_1",
                origin: "local",
                provider: "codex",
                session_id: "sid",
                token_hash: &hash,
                expires_at_ms,
            })
            .unwrap();

        let valid = store
            .validate_share_link("shr_1", &token_hash(&token))
            .unwrap()
            .unwrap();
        let rows = store.list_share_links("usr_1").unwrap();

        assert_eq!(link.id, "shr_1");
        assert_eq!(valid.session_id, "sid");
        assert_eq!(rows.len(), 1);
        assert!(!format!("{rows:?}").contains(&token));

        assert!(store.revoke_share_link("usr_1", "shr_1").unwrap());
        assert!(store
            .validate_share_link("shr_1", &token_hash(&token))
            .unwrap()
            .is_none());
    }

    fn auth_store_with_user() -> DerivedStore {
        let store = DerivedStore::open_in_memory().unwrap();
        let password_hash = hash_password("password").unwrap();
        store
            .create_user(NewUser {
                id: "usr_1",
                email: "user@example.com",
                password_hash: &password_hash,
                display_name: Some("User"),
            })
            .unwrap();
        store
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
