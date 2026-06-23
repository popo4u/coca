use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use base64::prelude::{Engine as _, BASE64_STANDARD};
use coca_protocol::{
    TerminalClientFrame, TerminalClose, TerminalInput, TerminalOpen, TerminalOutput,
    TerminalResize, TerminalServerFrame,
};
use coca_protocol::{TerminalExitInfo, TerminalId, TerminalSessionSummary, TerminalSize};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use super::TerminalRuntimeError;

const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REMOTE_IO_TIMEOUT: Duration = Duration::from_secs(5);

pub trait TerminalBackend {
    fn open(
        &mut self,
        terminal: &TerminalSessionSummary,
        target: &TerminalLaunchTarget,
    ) -> Result<(), TerminalRuntimeError>;
    fn input(&mut self, terminal_id: &TerminalId, data: &[u8]) -> Result<(), TerminalRuntimeError>;
    fn resize(
        &mut self,
        terminal_id: &TerminalId,
        size: TerminalSize,
    ) -> Result<(), TerminalRuntimeError>;
    fn kill(&mut self, terminal_id: &TerminalId) -> Result<(), TerminalRuntimeError>;
    fn drain_events(&mut self) -> Vec<TerminalBackendEvent>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalLaunchTarget {
    Local(LocalTerminalLaunchTarget),
    Remote(RemoteTerminalLaunchTarget),
}

impl TerminalLaunchTarget {
    pub fn local(program: String, args: Vec<String>, cwd: Option<PathBuf>) -> Self {
        Self::Local(LocalTerminalLaunchTarget { program, args, cwd })
    }

    pub fn remote(
        base_url: String,
        read_token: String,
        terminal_token: String,
        open: TerminalOpen,
    ) -> Self {
        Self::Remote(RemoteTerminalLaunchTarget {
            base_url,
            read_token,
            terminal_token,
            open,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTerminalLaunchTarget {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteTerminalLaunchTarget {
    pub base_url: String,
    pub read_token: String,
    pub terminal_token: String,
    pub open: TerminalOpen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalBackendEvent {
    Output {
        terminal_id: TerminalId,
        data: Vec<u8>,
    },
    Exit {
        terminal_id: TerminalId,
        exit: TerminalExitInfo,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedInput {
    pub terminal_id: TerminalId,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedResize {
    pub terminal_id: TerminalId,
    pub size: TerminalSize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FakeTerminalBackend {
    opened: Vec<RecordedOpen>,
    inputs: Vec<RecordedInput>,
    resizes: Vec<RecordedResize>,
    killed: Vec<TerminalId>,
    events: VecDeque<TerminalBackendEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedOpen {
    pub terminal: TerminalSessionSummary,
    pub target: TerminalLaunchTarget,
}

impl FakeTerminalBackend {
    pub fn opened(&self) -> &[RecordedOpen] {
        &self.opened
    }

    pub fn inputs(&self) -> &[RecordedInput] {
        &self.inputs
    }

    pub fn resizes(&self) -> &[RecordedResize] {
        &self.resizes
    }

    pub fn killed(&self) -> &[TerminalId] {
        &self.killed
    }

    pub fn emit_output(&mut self, terminal_id: TerminalId, data: impl Into<Vec<u8>>) {
        self.events.push_back(TerminalBackendEvent::Output {
            terminal_id,
            data: data.into(),
        });
    }

    pub fn emit_exit(&mut self, terminal_id: TerminalId, exit: TerminalExitInfo) {
        self.events
            .push_back(TerminalBackendEvent::Exit { terminal_id, exit });
    }
}

impl TerminalBackend for FakeTerminalBackend {
    fn open(
        &mut self,
        terminal: &TerminalSessionSummary,
        target: &TerminalLaunchTarget,
    ) -> Result<(), TerminalRuntimeError> {
        self.opened.push(RecordedOpen {
            terminal: terminal.clone(),
            target: target.clone(),
        });
        Ok(())
    }

    fn input(&mut self, terminal_id: &TerminalId, data: &[u8]) -> Result<(), TerminalRuntimeError> {
        self.inputs.push(RecordedInput {
            terminal_id: terminal_id.clone(),
            data: data.to_vec(),
        });
        Ok(())
    }

    fn resize(
        &mut self,
        terminal_id: &TerminalId,
        size: TerminalSize,
    ) -> Result<(), TerminalRuntimeError> {
        self.resizes.push(RecordedResize {
            terminal_id: terminal_id.clone(),
            size,
        });
        Ok(())
    }

    fn kill(&mut self, terminal_id: &TerminalId) -> Result<(), TerminalRuntimeError> {
        self.killed.push(terminal_id.clone());
        Ok(())
    }

    fn drain_events(&mut self) -> Vec<TerminalBackendEvent> {
        self.events.drain(..).collect()
    }
}

pub struct PortablePtyTerminalBackend {
    sessions: HashMap<String, PortablePtySession>,
    events_tx: Sender<TerminalBackendEvent>,
    events_rx: Receiver<TerminalBackendEvent>,
}

impl PortablePtyTerminalBackend {
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            sessions: HashMap::new(),
            events_tx,
            events_rx,
        }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use coca_protocol::{
        SessionRef, TerminalModeWire, TerminalOpened, TerminalSeq, TerminalStateWire,
    };
    use std::net::TcpListener;

    #[test]
    fn remote_backend_proxies_open_input_and_output() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request_for_test(&mut stream);
            assert!(request.starts_with(
                "GET /api/v1/terminal/ws?token=read-token&terminal_token=terminal-token HTTP/1.1"
            ));
            write!(
                stream,
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: test\r\n\r\n"
            )
            .unwrap();

            let open: TerminalClientFrame = read_ws_client_json_for_test(&mut stream);
            match open {
                TerminalClientFrame::Open(open) => {
                    assert_eq!(open.session.origin, "local");
                    assert_eq!(open.session.id, "sid");
                }
                other => panic!("expected open frame, got {other:?}"),
            }
            write_ws_server_json_for_test(
                &mut stream,
                &TerminalServerFrame::Opened(TerminalOpened {
                    terminal: remote_summary(),
                }),
            );

            let input: TerminalClientFrame = read_ws_client_json_for_test(&mut stream);
            match input {
                TerminalClientFrame::Input(input) => {
                    assert_eq!(input.terminal_id.0, "remote-1");
                    assert_eq!(
                        BASE64_STANDARD.decode(input.data_b64.as_bytes()).unwrap(),
                        b"ping"
                    );
                }
                other => panic!("expected input frame, got {other:?}"),
            }
            write_ws_server_json_for_test(
                &mut stream,
                &TerminalServerFrame::Output(TerminalOutput {
                    terminal_id: TerminalId("remote-1".to_string()),
                    seq: TerminalSeq(1),
                    data_b64: BASE64_STANDARD.encode(b"pong"),
                }),
            );
        });

        let mut backend = RemoteTerminalBackend::new();
        let local = local_summary();
        let target = RemoteTerminalLaunchTarget {
            base_url: format!("http://{addr}"),
            read_token: "read-token".to_string(),
            terminal_token: "terminal-token".to_string(),
            open: TerminalOpen {
                session: SessionRef {
                    origin: "local".to_string(),
                    provider: "codex".to_string(),
                    id: "sid".to_string(),
                },
                mode: TerminalModeWire::Resume,
                size: TerminalSize { cols: 80, rows: 24 },
            },
        };

        backend.open(&local, &target).unwrap();
        backend.input(&local.terminal_id, b"ping").unwrap();

        let output = wait_for_remote_output(&mut backend);
        assert_eq!(
            output,
            TerminalBackendEvent::Output {
                terminal_id: local.terminal_id,
                data: b"pong".to_vec(),
            }
        );
        handle.join().unwrap();
    }

    fn wait_for_remote_output(backend: &mut RemoteTerminalBackend) -> TerminalBackendEvent {
        for _ in 0..100 {
            if let Some(event) = backend.drain_events().into_iter().next() {
                return event;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("remote output was not received");
    }

    fn local_summary() -> TerminalSessionSummary {
        TerminalSessionSummary {
            terminal_id: TerminalId("local-1".to_string()),
            session: SessionRef {
                origin: "work".to_string(),
                provider: "codex".to_string(),
                id: "sid".to_string(),
            },
            mode: TerminalModeWire::Resume,
            state: TerminalStateWire::Starting,
            attached_clients: 1,
            active_writer: Some("client".to_string()),
            last_seq: TerminalSeq(0),
            size: TerminalSize { cols: 80, rows: 24 },
            exit: None,
        }
    }

    fn remote_summary() -> TerminalSessionSummary {
        TerminalSessionSummary {
            terminal_id: TerminalId("remote-1".to_string()),
            session: SessionRef {
                origin: "local".to_string(),
                provider: "codex".to_string(),
                id: "sid".to_string(),
            },
            mode: TerminalModeWire::Resume,
            state: TerminalStateWire::Running,
            attached_clients: 1,
            active_writer: Some("client".to_string()),
            last_seq: TerminalSeq(0),
            size: TerminalSize { cols: 80, rows: 24 },
            exit: None,
        }
    }

    fn read_http_request_for_test(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut byte = [0; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        String::from_utf8(request).unwrap()
    }

    fn read_ws_client_json_for_test<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> T {
        let mut header = [0; 2];
        stream.read_exact(&mut header).unwrap();
        assert_eq!(header[0] & 0x0F, 0x1);
        assert!(header[1] & 0x80 != 0);
        let mut len = usize::from(header[1] & 0x7F);
        if len == 126 {
            let mut extended = [0; 2];
            stream.read_exact(&mut extended).unwrap();
            len = usize::from(u16::from_be_bytes(extended));
        } else if len == 127 {
            let mut extended = [0; 8];
            stream.read_exact(&mut extended).unwrap();
            len = u64::from_be_bytes(extended) as usize;
        }
        let mut mask = [0; 4];
        stream.read_exact(&mut mask).unwrap();
        let mut payload = vec![0; len];
        stream.read_exact(&mut payload).unwrap();
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
        serde_json::from_slice(&payload).unwrap()
    }

    fn write_ws_server_json_for_test(stream: &mut TcpStream, frame: &TerminalServerFrame) {
        let payload = serde_json::to_vec(frame).unwrap();
        stream.write_all(&[0x81]).unwrap();
        match payload.len() {
            len @ 0..=125 => stream.write_all(&[len as u8]).unwrap(),
            len @ 126..=65535 => {
                stream.write_all(&[126]).unwrap();
                stream.write_all(&(len as u16).to_be_bytes()).unwrap();
            }
            len => {
                stream.write_all(&[127]).unwrap();
                stream.write_all(&(len as u64).to_be_bytes()).unwrap();
            }
        }
        stream.write_all(&payload).unwrap();
        stream.flush().unwrap();
    }
}

impl Default for PortablePtyTerminalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PortablePtyTerminalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortablePtyTerminalBackend")
            .field("sessions", &self.sessions.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl TerminalBackend for PortablePtyTerminalBackend {
    fn open(
        &mut self,
        terminal: &TerminalSessionSummary,
        target: &TerminalLaunchTarget,
    ) -> Result<(), TerminalRuntimeError> {
        let TerminalLaunchTarget::Local(target) = target else {
            return Err(TerminalRuntimeError::backend(
                "portable PTY backend cannot open remote terminal targets",
            ));
        };
        let size = pty_size(terminal.size);
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|err| TerminalRuntimeError::backend(format!("failed to open PTY: {err:#}")))?;

        let reader = pair.master.try_clone_reader().map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to clone PTY reader: {err:#}"))
        })?;
        let writer = pair.master.take_writer().map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to take PTY writer: {err:#}"))
        })?;
        let mut command = CommandBuilder::new(&target.program);
        command.args(&target.args);
        if let Some(cwd) = &target.cwd {
            command.cwd(cwd);
        }
        let mut child = pair.slave.spawn_command(command).map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to spawn provider command: {err:#}"))
        })?;
        drop(pair.slave);

        let terminal_id = terminal.terminal_id.clone();
        let reader_tx = self.events_tx.clone();
        thread::spawn(move || read_pty_output(terminal_id, reader, reader_tx));

        let terminal_id = terminal.terminal_id.clone();
        let exit_tx = self.events_tx.clone();
        let killer = child.clone_killer();
        thread::spawn(move || {
            let exit = match child.wait() {
                Ok(status) => TerminalExitInfo {
                    code: i32::try_from(status.exit_code()).ok(),
                    signal: status.signal().map(str::to_string),
                },
                Err(err) => TerminalExitInfo {
                    code: None,
                    signal: Some(format!("wait failed: {err}")),
                },
            };
            let _ = exit_tx.send(TerminalBackendEvent::Exit { terminal_id, exit });
        });

        self.sessions.insert(
            terminal.terminal_id.0.clone(),
            PortablePtySession {
                master: pair.master,
                writer,
                killer,
            },
        );
        Ok(())
    }

    fn input(&mut self, terminal_id: &TerminalId, data: &[u8]) -> Result<(), TerminalRuntimeError> {
        let session = self.session_mut(terminal_id)?;
        session.writer.write_all(data).map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to write PTY input: {err}"))
        })?;
        session.writer.flush().map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to flush PTY input: {err}"))
        })
    }

    fn resize(
        &mut self,
        terminal_id: &TerminalId,
        size: TerminalSize,
    ) -> Result<(), TerminalRuntimeError> {
        self.session(terminal_id)?
            .master
            .resize(pty_size(size))
            .map_err(|err| TerminalRuntimeError::backend(format!("failed to resize PTY: {err:#}")))
    }

    fn kill(&mut self, terminal_id: &TerminalId) -> Result<(), TerminalRuntimeError> {
        self.session_mut(terminal_id)?.killer.kill().map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to kill PTY child: {err}"))
        })
    }

    fn drain_events(&mut self) -> Vec<TerminalBackendEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            if let TerminalBackendEvent::Exit { terminal_id, .. } = &event {
                self.sessions.remove(&terminal_id.0);
            }
            events.push(event);
        }
        events
    }
}

pub struct DaemonTerminalBackend {
    local: PortablePtyTerminalBackend,
    remote: RemoteTerminalBackend,
}

impl DaemonTerminalBackend {
    pub fn new() -> Self {
        Self {
            local: PortablePtyTerminalBackend::new(),
            remote: RemoteTerminalBackend::new(),
        }
    }
}

impl Default for DaemonTerminalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DaemonTerminalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonTerminalBackend")
            .field("local", &self.local)
            .field("remote", &self.remote)
            .finish()
    }
}

impl TerminalBackend for DaemonTerminalBackend {
    fn open(
        &mut self,
        terminal: &TerminalSessionSummary,
        target: &TerminalLaunchTarget,
    ) -> Result<(), TerminalRuntimeError> {
        match target {
            TerminalLaunchTarget::Local(_) => self.local.open(terminal, target),
            TerminalLaunchTarget::Remote(target) => self.remote.open(terminal, target),
        }
    }

    fn input(&mut self, terminal_id: &TerminalId, data: &[u8]) -> Result<(), TerminalRuntimeError> {
        if self.remote.contains(terminal_id) {
            self.remote.input(terminal_id, data)
        } else {
            self.local.input(terminal_id, data)
        }
    }

    fn resize(
        &mut self,
        terminal_id: &TerminalId,
        size: TerminalSize,
    ) -> Result<(), TerminalRuntimeError> {
        if self.remote.contains(terminal_id) {
            self.remote.resize(terminal_id, size)
        } else {
            self.local.resize(terminal_id, size)
        }
    }

    fn kill(&mut self, terminal_id: &TerminalId) -> Result<(), TerminalRuntimeError> {
        if self.remote.contains(terminal_id) {
            self.remote.kill(terminal_id)
        } else {
            self.local.kill(terminal_id)
        }
    }

    fn drain_events(&mut self) -> Vec<TerminalBackendEvent> {
        let mut events = self.local.drain_events();
        events.extend(self.remote.drain_events());
        events
    }
}

pub struct RemoteTerminalBackend {
    sessions: HashMap<String, RemoteTerminalSession>,
    events_tx: Sender<TerminalBackendEvent>,
    events_rx: Receiver<TerminalBackendEvent>,
}

impl RemoteTerminalBackend {
    fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            sessions: HashMap::new(),
            events_tx,
            events_rx,
        }
    }

    fn open(
        &mut self,
        terminal: &TerminalSessionSummary,
        target: &RemoteTerminalLaunchTarget,
    ) -> Result<(), TerminalRuntimeError> {
        let mut stream = connect_remote_websocket(target)?;
        write_ws_client_json(&mut stream, &TerminalClientFrame::Open(target.open.clone()))?;
        let opened = loop {
            match read_ws_server_json(&mut stream)? {
                TerminalServerFrame::Opened(opened) => break opened,
                TerminalServerFrame::Output(output) => {
                    self.events_tx
                        .send(remote_output_event(&terminal.terminal_id, output)?)
                        .map_err(|err| {
                            TerminalRuntimeError::backend(format!(
                                "failed to queue remote output: {err}"
                            ))
                        })?;
                }
                TerminalServerFrame::Exit(exit) => {
                    return Err(TerminalRuntimeError::backend(format!(
                        "remote terminal exited before open completed: {:?}",
                        exit.exit
                    )));
                }
                TerminalServerFrame::Error(error) => {
                    return Err(TerminalRuntimeError::backend(format!(
                        "remote terminal open failed: {}",
                        error.message
                    )));
                }
            }
        };

        let reader = stream.try_clone().map_err(|err| {
            TerminalRuntimeError::backend(format!(
                "failed to clone remote terminal websocket: {err}"
            ))
        })?;
        let local_terminal_id = terminal.terminal_id.clone();
        let reader_tx = self.events_tx.clone();
        thread::spawn(move || read_remote_websocket(local_terminal_id, reader, reader_tx));

        self.sessions.insert(
            terminal.terminal_id.0.clone(),
            RemoteTerminalSession {
                writer: stream,
                remote_terminal_id: opened.terminal.terminal_id,
            },
        );
        Ok(())
    }

    fn contains(&self, terminal_id: &TerminalId) -> bool {
        self.sessions.contains_key(terminal_id.0.as_str())
    }

    fn input(&mut self, terminal_id: &TerminalId, data: &[u8]) -> Result<(), TerminalRuntimeError> {
        let session = self.session_mut(terminal_id)?;
        let frame = TerminalClientFrame::Input(TerminalInput {
            terminal_id: session.remote_terminal_id.clone(),
            data_b64: BASE64_STANDARD.encode(data),
        });
        write_ws_client_json(&mut session.writer, &frame)
    }

    fn resize(
        &mut self,
        terminal_id: &TerminalId,
        size: TerminalSize,
    ) -> Result<(), TerminalRuntimeError> {
        let session = self.session_mut(terminal_id)?;
        let frame = TerminalClientFrame::Resize(TerminalResize {
            terminal_id: session.remote_terminal_id.clone(),
            size,
        });
        write_ws_client_json(&mut session.writer, &frame)
    }

    fn kill(&mut self, terminal_id: &TerminalId) -> Result<(), TerminalRuntimeError> {
        let session = self.session_mut(terminal_id)?;
        let frame = TerminalClientFrame::Close(TerminalClose {
            terminal_id: session.remote_terminal_id.clone(),
            kill: true,
        });
        write_ws_client_json(&mut session.writer, &frame)
    }

    fn drain_events(&mut self) -> Vec<TerminalBackendEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            if let TerminalBackendEvent::Exit { terminal_id, .. } = &event {
                self.sessions.remove(&terminal_id.0);
            }
            events.push(event);
        }
        events
    }

    fn session_mut(
        &mut self,
        terminal_id: &TerminalId,
    ) -> Result<&mut RemoteTerminalSession, TerminalRuntimeError> {
        self.sessions
            .get_mut(terminal_id.0.as_str())
            .ok_or_else(|| TerminalRuntimeError::not_found(terminal_id))
    }
}

impl std::fmt::Debug for RemoteTerminalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteTerminalBackend")
            .field("sessions", &self.sessions.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

struct RemoteTerminalSession {
    writer: TcpStream,
    remote_terminal_id: TerminalId,
}

struct PortablePtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

fn read_pty_output(
    terminal_id: TerminalId,
    mut reader: Box<dyn Read + Send>,
    events_tx: Sender<TerminalBackendEvent>,
) {
    let mut buffer = [0; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(n) => {
                if events_tx
                    .send(TerminalBackendEvent::Output {
                        terminal_id: terminal_id.clone(),
                        data: buffer[..n].to_vec(),
                    })
                    .is_err()
                {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn connect_remote_websocket(
    target: &RemoteTerminalLaunchTarget,
) -> Result<TcpStream, TerminalRuntimeError> {
    let base = parse_http_base_url(target.base_url.trim())?;
    let addr = base
        .addr
        .to_socket_addrs()
        .map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to resolve remote terminal host: {err}"))
        })?
        .next()
        .ok_or_else(|| {
            TerminalRuntimeError::backend("remote terminal host resolved to no addresses")
        })?;
    let mut stream = TcpStream::connect_timeout(&addr, REMOTE_CONNECT_TIMEOUT).map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to connect remote terminal gateway: {err}"))
    })?;
    stream
        .set_read_timeout(Some(REMOTE_IO_TIMEOUT))
        .map_err(|err| {
            TerminalRuntimeError::backend(format!(
                "failed to set remote terminal read timeout: {err}"
            ))
        })?;
    stream
        .set_write_timeout(Some(REMOTE_IO_TIMEOUT))
        .map_err(|err| {
            TerminalRuntimeError::backend(format!(
                "failed to set remote terminal write timeout: {err}"
            ))
        })?;

    let key = websocket_key();
    let target_path = base.target(&format!(
        "/api/v1/terminal/ws?token={}&terminal_token={}",
        percent_encode_query_value(target.read_token.trim()),
        percent_encode_query_value(target.terminal_token.trim())
    ));
    write!(
        stream,
        "GET {target_path} HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\n\r\n",
        base.host_header
    )
    .map_err(|err| TerminalRuntimeError::backend(format!("failed to write remote terminal websocket request: {err}")))?;
    stream.flush().map_err(|err| {
        TerminalRuntimeError::backend(format!(
            "failed to flush remote terminal websocket request: {err}"
        ))
    })?;

    let response = read_http_head(&mut stream)?;
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .unwrap_or_default();
    if status != 101 {
        return Err(TerminalRuntimeError::backend(format!(
            "remote terminal websocket upgrade failed with HTTP {status}"
        )));
    }
    stream.set_read_timeout(None).map_err(|err| {
        TerminalRuntimeError::backend(format!(
            "failed to clear remote terminal read timeout: {err}"
        ))
    })?;
    Ok(stream)
}

fn read_remote_websocket(
    local_terminal_id: TerminalId,
    mut reader: TcpStream,
    events_tx: Sender<TerminalBackendEvent>,
) {
    loop {
        let frame = match read_ws_server_json(&mut reader) {
            Ok(frame) => frame,
            Err(_) => return,
        };
        let event = match frame {
            TerminalServerFrame::Output(output) => remote_output_event(&local_terminal_id, output),
            TerminalServerFrame::Exit(exit) => Ok(TerminalBackendEvent::Exit {
                terminal_id: local_terminal_id.clone(),
                exit: exit.exit,
            }),
            TerminalServerFrame::Error(error) => Ok(TerminalBackendEvent::Output {
                terminal_id: local_terminal_id.clone(),
                data: format!("\r\n[coca] remote terminal error: {}\r\n", error.message)
                    .into_bytes(),
            }),
            TerminalServerFrame::Opened(_) => continue,
        };
        let Ok(event) = event else {
            return;
        };
        if events_tx.send(event).is_err() {
            return;
        }
    }
}

fn remote_output_event(
    local_terminal_id: &TerminalId,
    output: TerminalOutput,
) -> Result<TerminalBackendEvent, TerminalRuntimeError> {
    let data = BASE64_STANDARD
        .decode(output.data_b64.as_bytes())
        .map_err(|_| {
            TerminalRuntimeError::backend("remote terminal output was not valid base64")
        })?;
    Ok(TerminalBackendEvent::Output {
        terminal_id: local_terminal_id.clone(),
        data,
    })
}

fn write_ws_client_json(
    stream: &mut TcpStream,
    frame: &TerminalClientFrame,
) -> Result<(), TerminalRuntimeError> {
    let payload = serde_json::to_vec(frame).map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to encode remote terminal frame: {err}"))
    })?;
    write_ws_client_frame(stream, 0x1, &payload)
}

fn read_ws_server_json(
    stream: &mut TcpStream,
) -> Result<TerminalServerFrame, TerminalRuntimeError> {
    let frame = read_ws_server_frame(stream)?;
    serde_json::from_slice(&frame.payload).map_err(|err| {
        TerminalRuntimeError::backend(format!("remote terminal frame was invalid JSON: {err}"))
    })
}

struct WsFrame {
    payload: Vec<u8>,
}

fn read_ws_server_frame(stream: &mut TcpStream) -> Result<WsFrame, TerminalRuntimeError> {
    let mut header = [0; 2];
    stream.read_exact(&mut header).map_err(|err| {
        TerminalRuntimeError::backend(format!(
            "failed to read remote websocket frame header: {err}"
        ))
    })?;
    if header[0] & 0x80 == 0 {
        return Err(TerminalRuntimeError::backend(
            "fragmented remote websocket frames are not supported",
        ));
    }
    let opcode = header[0] & 0x0F;
    if opcode == 0x8 {
        return Err(TerminalRuntimeError::backend("remote websocket closed"));
    }
    if !matches!(opcode, 0x1 | 0x2) {
        return Err(TerminalRuntimeError::backend(format!(
            "unsupported remote websocket opcode {opcode}"
        )));
    }
    let masked = header[1] & 0x80 != 0;
    if masked {
        return Err(TerminalRuntimeError::backend(
            "remote websocket server frames must not be masked",
        ));
    }
    let mut len = u64::from(header[1] & 0x7F);
    if len == 126 {
        let mut extended = [0; 2];
        stream.read_exact(&mut extended).map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to read remote websocket length: {err}"))
        })?;
        len = u64::from(u16::from_be_bytes(extended));
    } else if len == 127 {
        let mut extended = [0; 8];
        stream.read_exact(&mut extended).map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to read remote websocket length: {err}"))
        })?;
        len = u64::from_be_bytes(extended);
    }
    if len > coca_ipc::MAX_FRAME_LEN as u64 {
        return Err(TerminalRuntimeError::backend(
            "remote websocket frame exceeded maximum size",
        ));
    }
    let mut payload = vec![0; len as usize];
    stream.read_exact(&mut payload).map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to read remote websocket payload: {err}"))
    })?;
    Ok(WsFrame { payload })
}

fn write_ws_client_frame(
    stream: &mut TcpStream,
    opcode: u8,
    payload: &[u8],
) -> Result<(), TerminalRuntimeError> {
    if payload.len() > coca_ipc::MAX_FRAME_LEN as usize {
        return Err(TerminalRuntimeError::backend(
            "remote terminal frame exceeded maximum size",
        ));
    }
    let mask = websocket_mask();
    stream.write_all(&[0x80 | opcode]).map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to write websocket frame header: {err}"))
    })?;
    match payload.len() {
        len @ 0..=125 => stream.write_all(&[0x80 | len as u8]).map_err(|err| {
            TerminalRuntimeError::backend(format!("failed to write websocket frame length: {err}"))
        })?,
        len @ 126..=65535 => {
            stream.write_all(&[0x80 | 126]).map_err(|err| {
                TerminalRuntimeError::backend(format!(
                    "failed to write websocket frame length: {err}"
                ))
            })?;
            stream
                .write_all(&(len as u16).to_be_bytes())
                .map_err(|err| {
                    TerminalRuntimeError::backend(format!(
                        "failed to write websocket frame length: {err}"
                    ))
                })?;
        }
        len => {
            stream.write_all(&[0x80 | 127]).map_err(|err| {
                TerminalRuntimeError::backend(format!(
                    "failed to write websocket frame length: {err}"
                ))
            })?;
            stream
                .write_all(&(len as u64).to_be_bytes())
                .map_err(|err| {
                    TerminalRuntimeError::backend(format!(
                        "failed to write websocket frame length: {err}"
                    ))
                })?;
        }
    }
    stream.write_all(&mask).map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to write websocket frame mask: {err}"))
    })?;
    let mut masked = payload.to_vec();
    for (index, byte) in masked.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    stream.write_all(&masked).map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to write websocket frame payload: {err}"))
    })?;
    stream.flush().map_err(|err| {
        TerminalRuntimeError::backend(format!("failed to flush websocket frame: {err}"))
    })
}

fn read_http_head(stream: &mut TcpStream) -> Result<String, TerminalRuntimeError> {
    let mut head = Vec::new();
    let mut byte = [0; 1];
    while !head.ends_with(b"\r\n\r\n") && !head.ends_with(b"\n\n") {
        stream.read_exact(&mut byte).map_err(|err| {
            TerminalRuntimeError::backend(format!(
                "failed to read remote websocket upgrade response: {err}"
            ))
        })?;
        head.push(byte[0]);
        if head.len() > 16 * 1024 {
            return Err(TerminalRuntimeError::backend(
                "remote websocket upgrade response was too large",
            ));
        }
    }
    String::from_utf8(head).map_err(|err| {
        TerminalRuntimeError::backend(format!(
            "remote websocket upgrade response was not UTF-8: {err}"
        ))
    })
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

fn parse_http_base_url(base_url: &str) -> Result<HttpBase, TerminalRuntimeError> {
    let rest = base_url.strip_prefix("http://").ok_or_else(|| {
        TerminalRuntimeError::backend("only http:// remote terminal URLs are supported")
    })?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.trim().is_empty() {
        return Err(TerminalRuntimeError::backend(
            "remote terminal URL host must not be empty",
        ));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => {
            let port = port.parse::<u16>().map_err(|err| {
                TerminalRuntimeError::backend(format!(
                    "remote terminal URL port was invalid: {err}"
                ))
            })?;
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

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn websocket_key() -> String {
    BASE64_STANDARD.encode(format!("coca-{}-{}", std::process::id(), monotonic_seed()))
}

fn websocket_mask() -> [u8; 4] {
    monotonic_seed().to_be_bytes()
}

fn monotonic_seed() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(1);
    let counter = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or_default();
    counter ^ nanos
}

impl PortablePtyTerminalBackend {
    fn session(
        &self,
        terminal_id: &TerminalId,
    ) -> Result<&PortablePtySession, TerminalRuntimeError> {
        self.sessions
            .get(terminal_id.0.as_str())
            .ok_or_else(|| TerminalRuntimeError::not_found(terminal_id))
    }

    fn session_mut(
        &mut self,
        terminal_id: &TerminalId,
    ) -> Result<&mut PortablePtySession, TerminalRuntimeError> {
        self.sessions
            .get_mut(terminal_id.0.as_str())
            .ok_or_else(|| TerminalRuntimeError::not_found(terminal_id))
    }
}
