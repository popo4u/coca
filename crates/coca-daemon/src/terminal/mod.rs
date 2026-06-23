mod backend;
mod session;
mod stream;

use std::collections::HashMap;
use std::fmt;

use coca_protocol::{
    TerminalAttach, TerminalExitInfo, TerminalId, TerminalListResult, TerminalOpen, TerminalSeq,
    TerminalSessionSummary, TerminalSize,
};

pub use backend::{
    DaemonTerminalBackend, FakeTerminalBackend, LocalTerminalLaunchTarget,
    PortablePtyTerminalBackend, RecordedInput, RecordedOpen, RecordedResize,
    RemoteTerminalLaunchTarget, TerminalBackend, TerminalBackendEvent, TerminalLaunchTarget,
};
pub use session::{TerminalOutputChunk, TerminalSession, TerminalState};
#[cfg(unix)]
pub use stream::handle_unix_stream;
pub use stream::{handle_client_frame, handle_stream, terminal_output_frame};

const DEFAULT_SCROLLBACK_CHUNKS: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalAttachment {
    pub terminal: TerminalSessionSummary,
    pub replay: Vec<TerminalOutputChunk>,
    pub active_writer: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalEvent {
    Output(TerminalOutputChunk),
    Exit {
        terminal_id: TerminalId,
        exit: TerminalExitInfo,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalRuntimeErrorCode {
    NotFound,
    NotActiveWriter,
    Exited,
    Backend,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRuntimeError {
    code: TerminalRuntimeErrorCode,
    message: String,
}

impl TerminalRuntimeError {
    pub fn code(&self) -> TerminalRuntimeErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn not_found(terminal_id: &TerminalId) -> Self {
        Self {
            code: TerminalRuntimeErrorCode::NotFound,
            message: format!("terminal not found: {}", terminal_id.0),
        }
    }

    pub fn not_active_writer(terminal_id: &TerminalId, client_id: &str) -> Self {
        Self {
            code: TerminalRuntimeErrorCode::NotActiveWriter,
            message: format!(
                "client {client_id} is not the active writer for terminal {}",
                terminal_id.0
            ),
        }
    }

    pub fn exited(terminal_id: &TerminalId) -> Self {
        Self {
            code: TerminalRuntimeErrorCode::Exited,
            message: format!("terminal has exited: {}", terminal_id.0),
        }
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self {
            code: TerminalRuntimeErrorCode::Backend,
            message: message.into(),
        }
    }
}

impl fmt::Display for TerminalRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for TerminalRuntimeError {}

#[derive(Clone, Debug)]
pub struct TerminalManager<B = FakeTerminalBackend> {
    sessions: HashMap<String, TerminalSession>,
    backend: B,
    next_terminal_id: u64,
    scrollback_capacity: usize,
}

impl TerminalManager<FakeTerminalBackend> {
    pub fn new() -> Self {
        Self::with_backend(FakeTerminalBackend::default())
    }

    pub fn with_scrollback_capacity(scrollback_capacity: usize) -> Self {
        Self::with_backend_and_scrollback_capacity(
            FakeTerminalBackend::default(),
            scrollback_capacity,
        )
    }
}

impl Default for TerminalManager<FakeTerminalBackend> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B> TerminalManager<B>
where
    B: TerminalBackend,
{
    pub fn with_backend(backend: B) -> Self {
        Self::with_backend_and_scrollback_capacity(backend, DEFAULT_SCROLLBACK_CHUNKS)
    }

    pub fn with_backend_and_scrollback_capacity(backend: B, scrollback_capacity: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            backend,
            next_terminal_id: 1,
            scrollback_capacity,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn open(
        &mut self,
        client_id: impl Into<String>,
        request: TerminalOpen,
        target: TerminalLaunchTarget,
    ) -> Result<TerminalAttachment, TerminalRuntimeError> {
        let client_id = client_id.into();
        let terminal_id = self.next_terminal_id();
        let mut session = TerminalSession::new(
            terminal_id.clone(),
            request.session,
            request.mode,
            request.size,
            self.scrollback_capacity,
        );
        session.attach(client_id.clone(), request.size);

        self.backend.open(&session.summary(), &target)?;
        session.mark_running();

        let attachment = TerminalAttachment {
            terminal: session.summary(),
            replay: Vec::new(),
            active_writer: session.is_active_writer(&client_id),
        };
        self.sessions.insert(terminal_id.0, session);
        Ok(attachment)
    }

    pub fn attach(
        &mut self,
        client_id: impl Into<String>,
        request: TerminalAttach,
    ) -> Result<TerminalAttachment, TerminalRuntimeError> {
        let client_id = client_id.into();
        let session = self.session_mut(&request.terminal_id)?;
        let active_writer = session.attach(client_id, request.size);
        let replay = session.replay_since(request.since_seq);
        Ok(TerminalAttachment {
            terminal: session.summary(),
            replay,
            active_writer,
        })
    }

    pub fn input(
        &mut self,
        client_id: &str,
        terminal_id: &TerminalId,
        data: impl AsRef<[u8]>,
    ) -> Result<(), TerminalRuntimeError> {
        self.ensure_writer(client_id, terminal_id)?;
        self.backend.input(terminal_id, data.as_ref())
    }

    pub fn resize(
        &mut self,
        client_id: &str,
        terminal_id: &TerminalId,
        size: TerminalSize,
    ) -> Result<(), TerminalRuntimeError> {
        self.ensure_writer(client_id, terminal_id)?;
        self.backend.resize(terminal_id, size)?;
        self.session_mut(terminal_id)?.set_size(size);
        Ok(())
    }

    pub fn detach(
        &mut self,
        client_id: &str,
        terminal_id: &TerminalId,
    ) -> Result<TerminalSessionSummary, TerminalRuntimeError> {
        let session = self.session_mut(terminal_id)?;
        session.detach(client_id);
        Ok(session.summary())
    }

    pub fn detach_client(&mut self, client_id: &str) -> Vec<TerminalSessionSummary> {
        let mut summaries = Vec::new();
        for session in self.sessions.values_mut() {
            session.detach(client_id);
            summaries.push(session.summary());
        }
        summaries
    }

    pub fn close(
        &mut self,
        terminal_id: &TerminalId,
        kill: bool,
    ) -> Result<TerminalSessionSummary, TerminalRuntimeError> {
        if kill {
            return self.kill(terminal_id);
        }

        let session = self.session_mut(terminal_id)?;
        session.detach_all();
        Ok(session.summary())
    }

    pub fn kill(
        &mut self,
        terminal_id: &TerminalId,
    ) -> Result<TerminalSessionSummary, TerminalRuntimeError> {
        if self.session(terminal_id)?.state() == TerminalState::Exited {
            return Ok(self.session(terminal_id)?.summary());
        }

        self.backend.kill(terminal_id)?;
        let exit = TerminalExitInfo {
            code: None,
            signal: Some("killed".to_string()),
        };
        let session = self.session_mut(terminal_id)?;
        session.mark_exited(exit);
        Ok(session.summary())
    }

    pub fn list(&self) -> TerminalListResult {
        let mut terminals = self
            .sessions
            .values()
            .map(TerminalSession::summary)
            .collect::<Vec<_>>();
        terminals.sort_by(|left, right| left.terminal_id.0.cmp(&right.terminal_id.0));
        TerminalListResult { terminals }
    }

    pub fn get(&self, terminal_id: &TerminalId) -> Option<TerminalSessionSummary> {
        self.sessions
            .get(terminal_id.0.as_str())
            .map(TerminalSession::summary)
    }

    pub fn replay_since(
        &self,
        terminal_id: &TerminalId,
        since_seq: Option<TerminalSeq>,
    ) -> Result<Vec<TerminalOutputChunk>, TerminalRuntimeError> {
        Ok(self.session(terminal_id)?.replay_since(since_seq))
    }

    pub fn drain_backend_events(&mut self) -> Vec<TerminalEvent> {
        let events = self.backend.drain_events();
        let mut terminal_events = Vec::new();

        for event in events {
            match event {
                TerminalBackendEvent::Output { terminal_id, data } => {
                    if let Ok(chunk) = self.record_output(&terminal_id, data) {
                        terminal_events.push(TerminalEvent::Output(chunk));
                    }
                }
                TerminalBackendEvent::Exit { terminal_id, exit } => {
                    if let Ok(session) = self.session_mut(&terminal_id) {
                        session.mark_exited(exit.clone());
                        terminal_events.push(TerminalEvent::Exit { terminal_id, exit });
                    }
                }
            }
        }

        terminal_events
    }

    fn next_terminal_id(&mut self) -> TerminalId {
        let id = self.next_terminal_id;
        self.next_terminal_id += 1;
        TerminalId(format!("terminal-{id}"))
    }

    fn record_output(
        &mut self,
        terminal_id: &TerminalId,
        data: Vec<u8>,
    ) -> Result<TerminalOutputChunk, TerminalRuntimeError> {
        Ok(self.session_mut(terminal_id)?.append_output(data))
    }

    fn ensure_writer(
        &self,
        client_id: &str,
        terminal_id: &TerminalId,
    ) -> Result<(), TerminalRuntimeError> {
        let session = self.session(terminal_id)?;
        if session.state() == TerminalState::Exited {
            return Err(TerminalRuntimeError::exited(terminal_id));
        }
        if !session.is_active_writer(client_id) {
            return Err(TerminalRuntimeError::not_active_writer(
                terminal_id,
                client_id,
            ));
        }
        Ok(())
    }

    fn session(&self, terminal_id: &TerminalId) -> Result<&TerminalSession, TerminalRuntimeError> {
        self.sessions
            .get(terminal_id.0.as_str())
            .ok_or_else(|| TerminalRuntimeError::not_found(terminal_id))
    }

    fn session_mut(
        &mut self,
        terminal_id: &TerminalId,
    ) -> Result<&mut TerminalSession, TerminalRuntimeError> {
        self.sessions
            .get_mut(terminal_id.0.as_str())
            .ok_or_else(|| TerminalRuntimeError::not_found(terminal_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coca_protocol::{SessionRef, TerminalModeWire, TerminalStateWire};

    #[test]
    fn state_maps_to_wire_state() {
        assert_eq!(TerminalState::Starting.wire(), TerminalStateWire::Starting);
        assert_eq!(TerminalState::Running.wire(), TerminalStateWire::Running);
        assert_eq!(TerminalState::Detached.wire(), TerminalStateWire::Detached);
        assert_eq!(TerminalState::Exited.wire(), TerminalStateWire::Exited);
    }

    #[test]
    fn detach_keeps_session_available() {
        let mut manager = TerminalManager::new();
        let terminal_id = open_terminal(&mut manager, "client-a");

        let summary = manager.detach("client-a", &terminal_id).unwrap();

        assert_eq!(summary.state, TerminalStateWire::Detached);
        assert_eq!(summary.attached_clients, 0);
        assert_eq!(summary.active_writer, None);
        assert_eq!(manager.list().terminals.len(), 1);
        assert_eq!(
            manager.get(&terminal_id).unwrap().state,
            TerminalStateWire::Detached
        );
    }

    #[test]
    fn kill_marks_session_exited_and_records_backend_kill() {
        let mut manager = TerminalManager::new();
        let terminal_id = open_terminal(&mut manager, "client-a");

        let summary = manager.kill(&terminal_id).unwrap();

        assert_eq!(summary.state, TerminalStateWire::Exited);
        assert_eq!(
            summary.exit,
            Some(TerminalExitInfo {
                code: None,
                signal: Some("killed".to_string()),
            })
        );
        assert_eq!(
            manager.backend().killed(),
            std::slice::from_ref(&terminal_id)
        );
        assert_eq!(
            manager.get(&terminal_id).unwrap().state,
            TerminalStateWire::Exited
        );
    }

    #[test]
    fn replay_uses_since_seq_and_scrollback_capacity() {
        let mut manager = TerminalManager::with_scrollback_capacity(2);
        let terminal_id = open_terminal(&mut manager, "client-a");

        manager
            .backend_mut()
            .emit_output(terminal_id.clone(), b"one".to_vec());
        manager
            .backend_mut()
            .emit_output(terminal_id.clone(), b"two".to_vec());
        manager
            .backend_mut()
            .emit_output(terminal_id.clone(), b"three".to_vec());

        let events = manager.drain_backend_events();
        assert_eq!(
            events,
            vec![
                TerminalEvent::Output(TerminalOutputChunk {
                    terminal_id: terminal_id.clone(),
                    seq: TerminalSeq(1),
                    data: b"one".to_vec(),
                }),
                TerminalEvent::Output(TerminalOutputChunk {
                    terminal_id: terminal_id.clone(),
                    seq: TerminalSeq(2),
                    data: b"two".to_vec(),
                }),
                TerminalEvent::Output(TerminalOutputChunk {
                    terminal_id: terminal_id.clone(),
                    seq: TerminalSeq(3),
                    data: b"three".to_vec(),
                }),
            ]
        );

        let replay = manager
            .attach(
                "client-b",
                TerminalAttach {
                    terminal_id: terminal_id.clone(),
                    since_seq: Some(TerminalSeq(1)),
                    size: size(),
                },
            )
            .unwrap()
            .replay;

        assert_eq!(
            replay,
            vec![
                TerminalOutputChunk {
                    terminal_id: terminal_id.clone(),
                    seq: TerminalSeq(2),
                    data: b"two".to_vec(),
                },
                TerminalOutputChunk {
                    terminal_id,
                    seq: TerminalSeq(3),
                    data: b"three".to_vec(),
                },
            ]
        );
    }

    #[test]
    fn only_active_writer_can_input_or_resize() {
        let mut manager = TerminalManager::new();
        let terminal_id = open_terminal(&mut manager, "writer");
        let attach = manager
            .attach(
                "viewer",
                TerminalAttach {
                    terminal_id: terminal_id.clone(),
                    since_seq: None,
                    size: size(),
                },
            )
            .unwrap();
        assert!(!attach.active_writer);

        let input_error = manager.input("viewer", &terminal_id, b"no").unwrap_err();
        assert_eq!(
            input_error.code(),
            TerminalRuntimeErrorCode::NotActiveWriter
        );

        let resize_error = manager
            .resize(
                "viewer",
                &terminal_id,
                TerminalSize {
                    cols: 120,
                    rows: 30,
                },
            )
            .unwrap_err();
        assert_eq!(
            resize_error.code(),
            TerminalRuntimeErrorCode::NotActiveWriter
        );

        manager.input("writer", &terminal_id, b"yes").unwrap();
        manager
            .resize(
                "writer",
                &terminal_id,
                TerminalSize {
                    cols: 100,
                    rows: 40,
                },
            )
            .unwrap();

        assert_eq!(
            manager.backend().inputs(),
            &[RecordedInput {
                terminal_id: terminal_id.clone(),
                data: b"yes".to_vec(),
            }]
        );
        assert_eq!(
            manager.backend().resizes(),
            &[RecordedResize {
                terminal_id,
                size: TerminalSize {
                    cols: 100,
                    rows: 40
                },
            }]
        );
    }

    #[test]
    fn disconnecting_client_releases_active_writer_for_reattach() {
        let mut manager = TerminalManager::new();
        let terminal_id = open_terminal(&mut manager, "writer");

        manager.detach_client("writer");
        let attach = manager
            .attach(
                "new-writer",
                TerminalAttach {
                    terminal_id: terminal_id.clone(),
                    since_seq: None,
                    size: size(),
                },
            )
            .unwrap();

        assert!(attach.active_writer);
        assert_eq!(
            manager.get(&terminal_id).unwrap().active_writer.as_deref(),
            Some("new-writer")
        );
    }

    #[test]
    fn open_uses_trusted_launch_target() {
        let mut manager = TerminalManager::new();
        let target = launch_target();

        let terminal_id = manager
            .open("client-a", open_request(), target.clone())
            .unwrap()
            .terminal
            .terminal_id;

        let opened = manager.backend().opened();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].terminal.terminal_id, terminal_id);
        assert_eq!(opened[0].target, target);
    }

    fn open_terminal(
        manager: &mut TerminalManager<FakeTerminalBackend>,
        client_id: &str,
    ) -> TerminalId {
        manager
            .open(client_id, open_request(), launch_target())
            .unwrap()
            .terminal
            .terminal_id
    }

    fn open_request() -> TerminalOpen {
        TerminalOpen {
            session: SessionRef {
                origin: "local".to_string(),
                provider: "codex".to_string(),
                id: "session-1".to_string(),
            },
            mode: TerminalModeWire::Resume,
            size: size(),
        }
    }

    fn size() -> TerminalSize {
        TerminalSize { cols: 80, rows: 24 }
    }

    fn launch_target() -> TerminalLaunchTarget {
        TerminalLaunchTarget::local(
            "codex".to_string(),
            vec!["resume".to_string(), "session-1".to_string()],
            None,
        )
    }
}
