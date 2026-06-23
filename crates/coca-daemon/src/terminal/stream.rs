use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use coca_protocol::{
    TerminalClientFrame, TerminalError, TerminalInput, TerminalOpen, TerminalOpened,
    TerminalOutput, TerminalServerFrame,
};

use coca_ipc::{read_json_frame, write_json_frame};

use super::{
    TerminalBackend, TerminalEvent, TerminalLaunchTarget, TerminalManager, TerminalOutputChunk,
    TerminalRuntimeError,
};

pub type SharedTerminalManager<B> = Arc<Mutex<TerminalManager<B>>>;

pub fn handle_stream<B, S>(
    manager: SharedTerminalManager<B>,
    client_id: impl Into<String>,
    mut stream: S,
    launch_resolver: impl Fn(&TerminalOpen) -> Result<TerminalLaunchTarget, TerminalRuntimeError>,
) -> Result<()>
where
    B: TerminalBackend,
    S: Read + Write,
{
    let client_id = client_id.into();
    loop {
        let frame: TerminalClientFrame = match read_json_frame(&mut stream) {
            Ok(frame) => frame,
            Err(err) if is_clean_disconnect(&err) => {
                detach_client(&manager, &client_id);
                return Ok(());
            }
            Err(err) => {
                detach_client(&manager, &client_id);
                return Err(err).context("failed to read terminal client frame");
            }
        };
        let frames = {
            let mut manager = manager.lock().expect("terminal manager mutex poisoned");
            handle_client_frame(&mut manager, &client_id, frame, &launch_resolver)
        };
        for frame in frames {
            write_json_frame(&mut stream, &frame)
                .context("failed to write terminal server frame")?;
        }
    }
}

#[cfg(unix)]
pub fn handle_unix_stream<B>(
    manager: SharedTerminalManager<B>,
    client_id: impl Into<String>,
    mut stream: UnixStream,
    launch_resolver: impl Fn(&TerminalOpen) -> Result<TerminalLaunchTarget, TerminalRuntimeError>,
) -> Result<()>
where
    B: TerminalBackend,
{
    let client_id = client_id.into();
    let mut reader = stream
        .try_clone()
        .context("failed to clone terminal stream reader")?;
    let (frames_tx, frames_rx) = mpsc::channel();
    thread::spawn(move || loop {
        match read_json_frame::<_, TerminalClientFrame>(&mut reader) {
            Ok(frame) => {
                if frames_tx.send(Ok(frame)).is_err() {
                    return;
                }
            }
            Err(err) => {
                let clean = is_clean_disconnect(&err);
                let _ = frames_tx.send(Err((err, clean)));
                return;
            }
        }
    });

    loop {
        match frames_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(frame)) => {
                let frames = {
                    let mut manager = manager.lock().expect("terminal manager mutex poisoned");
                    handle_client_frame(&mut manager, &client_id, frame, &launch_resolver)
                };
                write_server_frames(&mut stream, frames)?;
            }
            Ok(Err((_, clean))) if clean => {
                detach_client(&manager, &client_id);
                return Ok(());
            }
            Ok(Err((err, _))) => {
                detach_client(&manager, &client_id);
                return Err(err).context("failed to read terminal client frame");
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                detach_client(&manager, &client_id);
                return Ok(());
            }
        }

        let frames = {
            let mut manager = manager.lock().expect("terminal manager mutex poisoned");
            manager
                .drain_backend_events()
                .into_iter()
                .map(terminal_event_frame)
                .collect::<Vec<_>>()
        };
        write_server_frames(&mut stream, frames)?;
    }
}

fn detach_client<B>(manager: &SharedTerminalManager<B>, client_id: &str)
where
    B: TerminalBackend,
{
    manager
        .lock()
        .expect("terminal manager mutex poisoned")
        .detach_client(client_id);
}

pub fn handle_client_frame<B>(
    manager: &mut TerminalManager<B>,
    client_id: &str,
    frame: TerminalClientFrame,
    launch_resolver: impl Fn(&TerminalOpen) -> Result<TerminalLaunchTarget, TerminalRuntimeError>,
) -> Vec<TerminalServerFrame>
where
    B: TerminalBackend,
{
    let mut frames = match frame {
        TerminalClientFrame::Open(request) => launch_resolver(&request)
            .and_then(|target| manager.open(client_id.to_string(), request, target))
            .map(|attachment| {
                let mut frames = vec![TerminalServerFrame::Opened(TerminalOpened {
                    terminal: attachment.terminal,
                })];
                frames.extend(attachment.replay.into_iter().map(terminal_output_frame));
                frames
            })
            .unwrap_or_else(|err| vec![error_frame(err)]),
        TerminalClientFrame::Attach(request) => manager
            .attach(client_id.to_string(), request)
            .map(|attachment| {
                let mut frames = vec![TerminalServerFrame::Opened(TerminalOpened {
                    terminal: attachment.terminal,
                })];
                frames.extend(attachment.replay.into_iter().map(terminal_output_frame));
                frames
            })
            .unwrap_or_else(|err| vec![error_frame(err)]),
        TerminalClientFrame::Input(request) => handle_input(manager, client_id, request),
        TerminalClientFrame::Resize(request) => manager
            .resize(client_id, &request.terminal_id, request.size)
            .map(|()| Vec::new())
            .unwrap_or_else(|err| vec![error_frame(err)]),
        TerminalClientFrame::Detach(request) => manager
            .detach(client_id, &request.terminal_id)
            .map(|summary| {
                vec![TerminalServerFrame::Opened(TerminalOpened {
                    terminal: summary,
                })]
            })
            .unwrap_or_else(|err| vec![error_frame(err)]),
        TerminalClientFrame::Close(request) => manager
            .close(&request.terminal_id, request.kill)
            .map(|summary| {
                vec![TerminalServerFrame::Opened(TerminalOpened {
                    terminal: summary,
                })]
            })
            .unwrap_or_else(|err| vec![error_frame(err)]),
    };

    frames.extend(
        manager
            .drain_backend_events()
            .into_iter()
            .map(terminal_event_frame),
    );
    frames
}

fn write_server_frames(stream: &mut impl Write, frames: Vec<TerminalServerFrame>) -> Result<()> {
    for frame in frames {
        write_json_frame(stream, &frame).context("failed to write terminal server frame")?;
    }
    Ok(())
}

pub fn terminal_output_frame(chunk: TerminalOutputChunk) -> TerminalServerFrame {
    TerminalServerFrame::Output(TerminalOutput {
        terminal_id: chunk.terminal_id,
        seq: chunk.seq,
        data_b64: BASE64_STANDARD.encode(chunk.data),
    })
}

fn handle_input<B>(
    manager: &mut TerminalManager<B>,
    client_id: &str,
    request: TerminalInput,
) -> Vec<TerminalServerFrame>
where
    B: TerminalBackend,
{
    let bytes = match BASE64_STANDARD.decode(request.data_b64.as_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => {
            return vec![TerminalServerFrame::Error(TerminalError {
                request_id: None,
                terminal_id: Some(request.terminal_id),
                code: "invalid_base64".to_string(),
                message: "terminal input was not valid base64".to_string(),
                action: Some("Retry with a valid terminal input frame.".to_string()),
                detail: None,
            })];
        }
    };

    manager
        .input(client_id, &request.terminal_id, bytes)
        .map(|()| Vec::new())
        .unwrap_or_else(|err| vec![error_frame(err)])
}

fn terminal_event_frame(event: TerminalEvent) -> TerminalServerFrame {
    match event {
        TerminalEvent::Output(chunk) => terminal_output_frame(chunk),
        TerminalEvent::Exit { terminal_id, exit } => {
            TerminalServerFrame::Exit(coca_protocol::TerminalExit { terminal_id, exit })
        }
    }
}

fn error_frame(error: TerminalRuntimeError) -> TerminalServerFrame {
    TerminalServerFrame::Error(TerminalError {
        request_id: None,
        terminal_id: None,
        code: terminal_runtime_error_code(error.code()).to_string(),
        message: error.message().to_string(),
        action: Some(terminal_runtime_error_action(error.code()).to_string()),
        detail: None,
    })
}

fn terminal_runtime_error_code(code: crate::terminal::TerminalRuntimeErrorCode) -> &'static str {
    match code {
        crate::terminal::TerminalRuntimeErrorCode::NotFound => "not_found",
        crate::terminal::TerminalRuntimeErrorCode::NotActiveWriter => "not_active_writer",
        crate::terminal::TerminalRuntimeErrorCode::Exited => "exited",
        crate::terminal::TerminalRuntimeErrorCode::Backend => "backend",
    }
}

fn terminal_runtime_error_action(code: crate::terminal::TerminalRuntimeErrorCode) -> &'static str {
    match code {
        crate::terminal::TerminalRuntimeErrorCode::NotFound => {
            "Refresh the terminal list and attach to an existing terminal."
        }
        crate::terminal::TerminalRuntimeErrorCode::NotActiveWriter => {
            "Attach as the active writer before sending input or resize events."
        }
        crate::terminal::TerminalRuntimeErrorCode::Exited => {
            "Open a new terminal session or attach to a running terminal."
        }
        crate::terminal::TerminalRuntimeErrorCode::Backend => {
            "Check the daemon logs and retry the terminal action."
        }
    }
}

fn is_clean_disconnect(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|err| {
                matches!(
                    err.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                )
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::FakeTerminalBackend;
    use coca_protocol::{
        SessionRef, TerminalAttach, TerminalClientFrame, TerminalInput, TerminalModeWire,
        TerminalOpen, TerminalSeq, TerminalSize, TerminalStateWire,
    };

    #[test]
    fn open_input_and_replay_roundtrip_through_frames() {
        let mut manager = TerminalManager::<FakeTerminalBackend>::new();

        let opened = handle_client_frame(
            &mut manager,
            "writer",
            TerminalClientFrame::Open(TerminalOpen {
                session: session_ref(),
                mode: TerminalModeWire::Resume,
                size: size(),
            }),
            trusted_target,
        );
        let terminal_id = match &opened[0] {
            TerminalServerFrame::Opened(opened) => opened.terminal.terminal_id.clone(),
            other => panic!("expected opened frame, got {other:?}"),
        };

        let input = handle_client_frame(
            &mut manager,
            "writer",
            TerminalClientFrame::Input(TerminalInput {
                terminal_id: terminal_id.clone(),
                data_b64: BASE64_STANDARD.encode(b"hello"),
            }),
            trusted_target,
        );
        assert!(input.is_empty());
        assert_eq!(manager.backend().inputs()[0].data, b"hello");

        manager
            .backend_mut()
            .emit_output(terminal_id.clone(), b"world".to_vec());
        let attach = handle_client_frame(
            &mut manager,
            "viewer",
            TerminalClientFrame::Attach(TerminalAttach {
                terminal_id: terminal_id.clone(),
                since_seq: Some(TerminalSeq(0)),
                size: size(),
            }),
            trusted_target,
        );

        assert!(matches!(attach[0], TerminalServerFrame::Opened(_)));
        assert_eq!(
            attach[1],
            TerminalServerFrame::Output(TerminalOutput {
                terminal_id,
                seq: TerminalSeq(1),
                data_b64: BASE64_STANDARD.encode(b"world"),
            })
        );
    }

    #[test]
    fn non_writer_input_returns_error_frame() {
        let mut manager = TerminalManager::<FakeTerminalBackend>::new();
        let opened = handle_client_frame(
            &mut manager,
            "writer",
            TerminalClientFrame::Open(TerminalOpen {
                session: session_ref(),
                mode: TerminalModeWire::Resume,
                size: size(),
            }),
            trusted_target,
        );
        let terminal_id = match &opened[0] {
            TerminalServerFrame::Opened(opened) => opened.terminal.terminal_id.clone(),
            other => panic!("expected opened frame, got {other:?}"),
        };
        let _ = handle_client_frame(
            &mut manager,
            "viewer",
            TerminalClientFrame::Attach(TerminalAttach {
                terminal_id: terminal_id.clone(),
                since_seq: None,
                size: size(),
            }),
            trusted_target,
        );

        let frames = handle_client_frame(
            &mut manager,
            "viewer",
            TerminalClientFrame::Input(TerminalInput {
                terminal_id,
                data_b64: BASE64_STANDARD.encode(b"no"),
            }),
            trusted_target,
        );

        match &frames[0] {
            TerminalServerFrame::Error(error) => {
                assert_eq!(error.code, "not_active_writer");
                assert!(error
                    .action
                    .as_deref()
                    .unwrap_or_default()
                    .contains("active writer"));
            }
            other => panic!("expected error frame, got {other:?}"),
        }
    }

    #[test]
    fn detach_reports_detached_state() {
        let mut manager = TerminalManager::<FakeTerminalBackend>::new();
        let opened = handle_client_frame(
            &mut manager,
            "writer",
            TerminalClientFrame::Open(TerminalOpen {
                session: session_ref(),
                mode: TerminalModeWire::Resume,
                size: size(),
            }),
            trusted_target,
        );
        let terminal_id = match &opened[0] {
            TerminalServerFrame::Opened(opened) => opened.terminal.terminal_id.clone(),
            other => panic!("expected opened frame, got {other:?}"),
        };

        let frames = handle_client_frame(
            &mut manager,
            "writer",
            TerminalClientFrame::Detach(coca_protocol::TerminalDetach { terminal_id }),
            trusted_target,
        );

        match &frames[0] {
            TerminalServerFrame::Opened(opened) => {
                assert_eq!(opened.terminal.state, TerminalStateWire::Detached)
            }
            other => panic!("expected opened frame, got {other:?}"),
        }
    }

    fn session_ref() -> SessionRef {
        SessionRef {
            origin: "local".to_string(),
            provider: "codex".to_string(),
            id: "sid".to_string(),
        }
    }

    fn size() -> TerminalSize {
        TerminalSize { cols: 80, rows: 24 }
    }

    fn trusted_target(_: &TerminalOpen) -> Result<TerminalLaunchTarget, TerminalRuntimeError> {
        Ok(TerminalLaunchTarget::local(
            "codex".to_string(),
            vec!["resume".to_string(), "sid".to_string()],
            None,
        ))
    }
}
