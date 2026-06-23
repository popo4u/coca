use std::collections::{BTreeSet, VecDeque};

use coca_protocol::{
    SessionRef, TerminalExitInfo, TerminalId, TerminalModeWire, TerminalSeq,
    TerminalSessionSummary, TerminalSize, TerminalStateWire,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalState {
    Starting,
    Running,
    Detached,
    Exited,
}

impl TerminalState {
    pub fn wire(self) -> TerminalStateWire {
        match self {
            Self::Starting => TerminalStateWire::Starting,
            Self::Running => TerminalStateWire::Running,
            Self::Detached => TerminalStateWire::Detached,
            Self::Exited => TerminalStateWire::Exited,
        }
    }
}

impl From<TerminalState> for TerminalStateWire {
    fn from(state: TerminalState) -> Self {
        state.wire()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalOutputChunk {
    pub terminal_id: TerminalId,
    pub seq: TerminalSeq,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct TerminalSession {
    terminal_id: TerminalId,
    session: SessionRef,
    mode: TerminalModeWire,
    state: TerminalState,
    attached_clients: BTreeSet<String>,
    active_writer: Option<String>,
    last_seq: TerminalSeq,
    size: TerminalSize,
    exit: Option<TerminalExitInfo>,
    scrollback: Scrollback,
}

impl TerminalSession {
    pub fn new(
        terminal_id: TerminalId,
        session: SessionRef,
        mode: TerminalModeWire,
        size: TerminalSize,
        scrollback_capacity: usize,
    ) -> Self {
        Self {
            terminal_id,
            session,
            mode,
            state: TerminalState::Starting,
            attached_clients: BTreeSet::new(),
            active_writer: None,
            last_seq: TerminalSeq(0),
            size,
            exit: None,
            scrollback: Scrollback::new(scrollback_capacity),
        }
    }

    pub fn terminal_id(&self) -> &TerminalId {
        &self.terminal_id
    }

    pub fn state(&self) -> TerminalState {
        self.state
    }

    pub fn is_active_writer(&self, client_id: &str) -> bool {
        self.active_writer.as_deref() == Some(client_id)
    }

    pub fn mark_running(&mut self) {
        if self.state != TerminalState::Exited {
            self.state = if self.attached_clients.is_empty() {
                TerminalState::Detached
            } else {
                TerminalState::Running
            };
        }
    }

    pub fn attach(&mut self, client_id: String, size: TerminalSize) -> bool {
        self.attached_clients.insert(client_id.clone());
        self.size = size;

        if self.state != TerminalState::Exited {
            if self.state != TerminalState::Starting {
                self.state = TerminalState::Running;
            }
            if self.active_writer.is_none() {
                self.active_writer = Some(client_id.clone());
            }
        }

        self.is_active_writer(&client_id)
    }

    pub fn detach(&mut self, client_id: &str) {
        self.attached_clients.remove(client_id);
        if self.active_writer.as_deref() == Some(client_id) {
            self.active_writer = None;
        }
        self.refresh_attached_state();
    }

    pub fn detach_all(&mut self) {
        self.attached_clients.clear();
        self.active_writer = None;
        self.refresh_attached_state();
    }

    pub fn set_size(&mut self, size: TerminalSize) {
        self.size = size;
    }

    pub fn append_output(&mut self, data: Vec<u8>) -> TerminalOutputChunk {
        self.last_seq = TerminalSeq(self.last_seq.0 + 1);
        let chunk = TerminalOutputChunk {
            terminal_id: self.terminal_id.clone(),
            seq: self.last_seq,
            data,
        };
        self.scrollback.push(chunk.clone());
        chunk
    }

    pub fn replay_since(&self, since_seq: Option<TerminalSeq>) -> Vec<TerminalOutputChunk> {
        self.scrollback.replay_since(since_seq)
    }

    pub fn mark_exited(&mut self, exit: TerminalExitInfo) {
        self.state = TerminalState::Exited;
        self.exit = Some(exit);
        self.active_writer = None;
    }

    pub fn summary(&self) -> TerminalSessionSummary {
        TerminalSessionSummary {
            terminal_id: self.terminal_id.clone(),
            session: self.session.clone(),
            mode: self.mode,
            state: self.state.wire(),
            attached_clients: self.attached_clients.len(),
            active_writer: self.active_writer.clone(),
            last_seq: self.last_seq,
            size: self.size,
            exit: self.exit.clone(),
        }
    }

    fn refresh_attached_state(&mut self) {
        if self.state == TerminalState::Exited || self.state == TerminalState::Starting {
            return;
        }
        self.state = if self.attached_clients.is_empty() {
            TerminalState::Detached
        } else {
            TerminalState::Running
        };
    }
}

#[derive(Clone, Debug)]
struct Scrollback {
    capacity: usize,
    chunks: VecDeque<TerminalOutputChunk>,
}

impl Scrollback {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            chunks: VecDeque::new(),
        }
    }

    fn push(&mut self, chunk: TerminalOutputChunk) {
        if self.capacity == 0 {
            return;
        }
        if self.chunks.len() == self.capacity {
            self.chunks.pop_front();
        }
        self.chunks.push_back(chunk);
    }

    fn replay_since(&self, since_seq: Option<TerminalSeq>) -> Vec<TerminalOutputChunk> {
        let after = since_seq.map(|seq| seq.0).unwrap_or(0);
        self.chunks
            .iter()
            .filter(|chunk| chunk.seq.0 > after)
            .cloned()
            .collect()
    }
}
