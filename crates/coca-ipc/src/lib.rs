use std::io::{Read, Write};

use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Serialize};

pub const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

pub fn write_json_frame<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: Write,
    T: Serialize,
{
    let payload = serde_json::to_vec(value).context("failed to serialize JSON-RPC frame")?;
    if payload.len() > MAX_FRAME_LEN as usize {
        return Err(anyhow!("JSON-RPC frame exceeded maximum size"));
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .context("failed to write JSON-RPC frame length")?;
    writer
        .write_all(&payload)
        .context("failed to write JSON-RPC frame payload")?;
    writer.flush().context("failed to flush JSON-RPC frame")
}

pub fn read_json_frame<R, T>(reader: &mut R) -> Result<T>
where
    R: Read,
    T: DeserializeOwned,
{
    let mut len = [0; 4];
    reader
        .read_exact(&mut len)
        .context("failed to read JSON-RPC frame length")?;
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME_LEN {
        return Err(anyhow!("JSON-RPC frame exceeded maximum size"));
    }

    let mut payload = vec![0; len as usize];
    reader
        .read_exact(&mut payload)
        .context("failed to read JSON-RPC frame payload")?;
    serde_json::from_slice(&payload).context("failed to parse JSON-RPC frame")
}

#[cfg(unix)]
pub mod unix {
    use std::fs;
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    use anyhow::{Context, Result};
    use coca_protocol::{JsonRpcRequest, JsonRpcResponse};

    use crate::{read_json_frame, write_json_frame};

    pub fn serve<F>(path: &Path, mut handler: F) -> Result<()>
    where
        F: FnMut(JsonRpcRequest) -> JsonRpcResponse,
    {
        let listener = bind_listener(path)?;
        for stream in listener.incoming() {
            let stream = stream.context("failed to accept IPC connection")?;
            handle_stream(stream, &mut handler)?;
        }
        Ok(())
    }

    pub fn serve_one<F>(path: &Path, mut handler: F) -> Result<()>
    where
        F: FnMut(JsonRpcRequest) -> JsonRpcResponse,
    {
        let listener = bind_listener(path)?;
        let (stream, _) = listener
            .accept()
            .context("failed to accept IPC connection")?;
        handle_stream(stream, &mut handler)
    }

    pub fn roundtrip(path: &Path, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut stream = UnixStream::connect(path)?;
        write_json_frame(&mut stream, request)?;
        read_json_frame(&mut stream)
    }

    fn bind_listener(path: &Path) -> Result<UnixListener> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create IPC socket directory {}", parent.display())
            })?;
        }
        remove_stale_socket(path)?;
        UnixListener::bind(path)
            .with_context(|| format!("failed to bind IPC socket {}", path.display()))
    }

    fn remove_stale_socket(path: &Path) -> Result<()> {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return Ok(());
        };
        if metadata.file_type().is_socket() {
            fs::remove_file(path)
                .with_context(|| format!("failed to remove stale IPC socket {}", path.display()))?;
        }
        Ok(())
    }

    fn handle_stream<F>(mut stream: UnixStream, handler: &mut F) -> Result<()>
    where
        F: FnMut(JsonRpcRequest) -> JsonRpcResponse,
    {
        let request = read_json_frame(&mut stream)?;
        let response = handler(request);
        write_json_frame(&mut stream, &response)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use coca_protocol::{methods, JsonRpcRequest, JsonRpcResponse};
    use serde_json::json;

    use super::*;

    #[test]
    fn json_frame_roundtrips_request() {
        let request = JsonRpcRequest::new(1, methods::CORE_PING, None);
        let mut buffer = Vec::new();

        write_json_frame(&mut buffer, &request).unwrap();
        let decoded: JsonRpcRequest = read_json_frame(&mut Cursor::new(buffer)).unwrap();

        assert_eq!(decoded, request);
    }

    #[test]
    fn json_frame_roundtrips_response() {
        let response = JsonRpcResponse::success(1, json!({"ok": true}));
        let mut buffer = Vec::new();

        write_json_frame(&mut buffer, &response).unwrap();
        let decoded: JsonRpcResponse = read_json_frame(&mut Cursor::new(buffer)).unwrap();

        assert_eq!(decoded, response);
    }

    #[cfg(unix)]
    #[test]
    fn unix_roundtrip_uses_json_rpc_frames() {
        use coca_protocol::{JsonRpcResponse, RpcId};

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("core.sock");
        let server_socket = socket.clone();
        let handle = std::thread::spawn(move || {
            super::unix::serve_one(&server_socket, |request| {
                JsonRpcResponse::success(request.id, json!({"pong": true}))
            })
            .unwrap();
        });

        wait_for_socket(&socket);
        let request = JsonRpcRequest::new(RpcId::Number(1), methods::CORE_PING, None);
        let response = super::unix::roundtrip(&socket, &request).unwrap();

        assert_eq!(response.result.unwrap(), json!({"pong": true}));
        handle.join().unwrap();
    }

    #[cfg(unix)]
    fn wait_for_socket(path: &std::path::Path) {
        for _ in 0..100 {
            if path.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
