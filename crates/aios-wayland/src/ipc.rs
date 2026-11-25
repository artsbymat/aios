//! Single-instance Unix domain socket IPC server and fast CLI client (`$XDG_RUNTIME_DIR/aios.sock`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use aios_core::{AiosError, Result};
use aios_daemon::{DaemonIpcCommand, DaemonIpcResponse, DmenuRequest, DmenuStreamMessage};

const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// An incoming command from a CLI client paired with a one-shot response channel
/// and an optional streaming receiver for live `aios dmenu` stdin batches.
#[derive(Debug)]
pub struct IpcEnvelope {
    pub command: DaemonIpcCommand,
    reply_tx: std::sync::mpsc::SyncSender<DaemonIpcResponse>,
    pub dmenu_stream_rx: Option<async_channel::Receiver<DmenuStreamMessage>>,
}

impl IpcEnvelope {
    /// Sends the response back to the waiting CLI client.
    pub fn respond(self, response: DaemonIpcResponse) {
        let _ = self.reply_tx.send(response);
    }
}

/// Active bidirectional IPC handle for streaming `aios dmenu` batches while awaiting selection.
#[derive(Debug)]
pub struct DmenuIpcSession {
    socket_path: PathBuf,
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl DmenuIpcSession {
    /// Sends one incremental [`DmenuStreamMessage`] (`Batch` or `Eof`) to the open daemon window.
    /// Returns `Ok(false)` if the daemon already closed the connection (e.g. user selected early).
    pub fn send_stream(&mut self, msg: &DmenuStreamMessage) -> Result<bool> {
        let mut encoded = serde_json::to_string(msg)
            .map_err(|e| AiosError::InvalidConfig(format!("failed to encode dmenu stream: {e}")))?;
        encoded.push('\n');
        match self.writer.write_all(encoded.as_bytes()) {
            Ok(()) => {
                let _ = self.writer.flush();
                Ok(true)
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                ) =>
            {
                Ok(false)
            }
            Err(source) => Err(AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            }),
        }
    }

    /// Blocks until the daemon sends back `DmenuSelected` or `DmenuCancelled`.
    pub fn wait_response(mut self) -> Result<DaemonIpcResponse> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;
        if line.trim().is_empty() {
            return Ok(DaemonIpcResponse::DmenuCancelled);
        }
        serde_json::from_str(line.trim())
            .map_err(|e| AiosError::InvalidConfig(format!("failed to decode IPC response: {e}")))
    }

    /// Splits into independent write (for background stdin streaming) and read (for awaiting choice) halves.
    pub fn split(self) -> (DmenuIpcWriter, DmenuIpcReader) {
        (
            DmenuIpcWriter {
                socket_path: self.socket_path.clone(),
                writer: self.writer,
            },
            DmenuIpcReader {
                socket_path: self.socket_path,
                reader: self.reader,
            },
        )
    }
}

#[derive(Debug)]
pub struct DmenuIpcWriter {
    socket_path: PathBuf,
    writer: UnixStream,
}

impl DmenuIpcWriter {
    pub fn send_stream(&mut self, msg: &DmenuStreamMessage) -> Result<bool> {
        let mut encoded = serde_json::to_string(msg)
            .map_err(|e| AiosError::InvalidConfig(format!("failed to encode dmenu stream: {e}")))?;
        encoded.push('\n');
        match self.writer.write_all(encoded.as_bytes()) {
            Ok(()) => {
                let _ = self.writer.flush();
                Ok(true)
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                ) =>
            {
                Ok(false)
            }
            Err(source) => Err(AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            }),
        }
    }
}

#[derive(Debug)]
pub struct DmenuIpcReader {
    socket_path: PathBuf,
    reader: BufReader<UnixStream>,
}

impl DmenuIpcReader {
    pub fn wait_response(mut self) -> Result<DaemonIpcResponse> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;
        if line.trim().is_empty() {
            return Ok(DaemonIpcResponse::DmenuCancelled);
        }
        serde_json::from_str(line.trim())
            .map_err(|e| AiosError::InvalidConfig(format!("failed to decode IPC response: {e}")))
    }
}

/// Fast synchronous IPC client used by `aios toggle`, `aios show`, `aios hide`, `aios open`.
#[derive(Debug, Clone)]
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    /// Returns `true` if a live AIOS daemon is currently accepting commands on `socket_path`.
    #[must_use]
    pub fn is_daemon_running(&self) -> bool {
        matches!(
            self.send(&DaemonIpcCommand::Ping),
            Ok(DaemonIpcResponse::Pong { .. })
        )
    }

    /// Sends one JSON command frame and waits for one JSON response frame.
    pub fn send(&self, command: &DaemonIpcCommand) -> Result<DaemonIpcResponse> {
        let mut stream =
            UnixStream::connect(&self.socket_path).map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;
        let is_interactive = matches!(command, DaemonIpcCommand::Dmenu(_));
        let _ = stream.set_read_timeout(if is_interactive {
            None
        } else {
            Some(IO_TIMEOUT)
        });
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

        let mut payload = serde_json::to_string(command)
            .map_err(|e| AiosError::InvalidConfig(format!("failed to encode IPC command: {e}")))?;
        payload.push('\n');
        stream
            .write_all(payload.as_bytes())
            .map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;
        stream.flush().map_err(|source| AiosError::IoPath {
            path: self.socket_path.clone(),
            source,
        })?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;

        let response: DaemonIpcResponse = serde_json::from_str(line.trim())
            .map_err(|e| AiosError::InvalidConfig(format!("failed to decode IPC response: {e}")))?;
        Ok(response)
    }

    /// Opens a streaming bidirectional `dmenu` IPC session so the daemon shows the picker
    /// immediately while stdin batches continue streaming in the background.
    pub fn start_dmenu_stream(&self, request: &DmenuRequest) -> Result<DmenuIpcSession> {
        let mut writer =
            UnixStream::connect(&self.socket_path).map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;
        let _ = writer.set_read_timeout(None);
        let _ = writer.set_write_timeout(Some(IO_TIMEOUT));

        let mut payload = serde_json::to_string(&DaemonIpcCommand::Dmenu(request.clone()))
            .map_err(|e| {
                AiosError::InvalidConfig(format!("failed to encode dmenu command: {e}"))
            })?;
        payload.push('\n');
        writer
            .write_all(payload.as_bytes())
            .map_err(|source| AiosError::IoPath {
                path: self.socket_path.clone(),
                source,
            })?;
        writer.flush().map_err(|source| AiosError::IoPath {
            path: self.socket_path.clone(),
            source,
        })?;

        let read_stream = writer.try_clone().map_err(|source| AiosError::IoPath {
            path: self.socket_path.clone(),
            source,
        })?;
        let _ = read_stream.set_read_timeout(None);

        Ok(DmenuIpcSession {
            socket_path: self.socket_path.clone(),
            writer,
            reader: BufReader::new(read_stream),
        })
    }
}

/// Single-instance Unix socket server owned by `aios daemon`.
///
/// Rejects duplicate daemon startup when an active instance responds to `Ping`,
/// and cleans up stale socket files left behind after an abrupt crash.
#[derive(Debug)]
pub struct IpcServer {
    socket_path: PathBuf,
    stop_flag: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl IpcServer {
    /// Binds `socket_path` exclusively and returns the server handle plus an async receiver
    /// for incoming [`IpcEnvelope`] requests.
    pub fn bind(
        socket_path: impl Into<PathBuf>,
    ) -> Result<(Self, async_channel::Receiver<IpcEnvelope>)> {
        let socket_path = socket_path.into();

        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AiosError::IoPath {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        if socket_path.exists() {
            let probe = IpcClient::new(&socket_path);
            if probe.is_daemon_running() {
                return Err(AiosError::InvalidConfig(format!(
                    "an AIOS daemon is already running at {}",
                    socket_path.display()
                )));
            }
            tracing::warn!(
                socket = %socket_path.display(),
                "removing stale AIOS IPC socket"
            );
            std::fs::remove_file(&socket_path).map_err(|source| AiosError::IoPath {
                path: socket_path.clone(),
                source,
            })?;
        }

        let listener = UnixListener::bind(&socket_path).map_err(|source| AiosError::IoPath {
            path: socket_path.clone(),
            source,
        })?;

        let (tx, rx) = async_channel::unbounded();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);
        let thread_path = socket_path.clone();

        let accept_thread = thread::Builder::new()
            .name("aios-ipc-server".into())
            .spawn(move || accept_loop(listener, thread_path, thread_stop, tx))
            .map_err(|source| AiosError::IoPath {
                path: socket_path.clone(),
                source,
            })?;

        Ok((
            Self {
                socket_path,
                stop_flag,
                accept_thread: Some(accept_thread),
            },
            rx,
        ))
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        // Wake the blocking `accept()` immediately so the accept thread exits with 0ms delay.
        let _ = UnixStream::connect(&self.socket_path);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn accept_loop(
    listener: UnixListener,
    socket_path: PathBuf,
    stop_flag: Arc<AtomicBool>,
    tx: async_channel::Sender<IpcEnvelope>,
) {
    while !stop_flag.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                handle_connection(stream, &tx);
            }
            Err(err) => {
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                tracing::error!(
                    socket = %socket_path.display(),
                    error = %err,
                    "IPC accept error"
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn handle_connection(mut stream: UnixStream, tx: &async_channel::Sender<IpcEnvelope>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }

    let response = match serde_json::from_str::<DaemonIpcCommand>(line.trim()) {
        Ok(DaemonIpcCommand::Ping) => DaemonIpcResponse::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Ok(command @ DaemonIpcCommand::Dmenu(_)) => {
            let is_streaming = matches!(&command, DaemonIpcCommand::Dmenu(r) if r.streaming);
            let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);

            let dmenu_stream_rx = if is_streaming {
                let (stream_tx, stream_rx) = async_channel::bounded::<DmenuStreamMessage>(64);
                let _ = reader.get_ref().set_read_timeout(None);
                let _ = thread::Builder::new()
                    .name("aios-ipc-dmenu-stream".into())
                    .spawn(move || {
                        let mut buf = String::new();
                        loop {
                            buf.clear();
                            match reader.read_line(&mut buf) {
                                Ok(0) | Err(_) => {
                                    let _ = stream_tx.send_blocking(DmenuStreamMessage::Eof);
                                    break;
                                }
                                Ok(_) => {
                                    let trimmed = buf.trim();
                                    if trimmed.is_empty() {
                                        continue;
                                    }
                                    match serde_json::from_str::<DmenuStreamMessage>(trimmed) {
                                        Ok(DmenuStreamMessage::Eof) => {
                                            let _ =
                                                stream_tx.send_blocking(DmenuStreamMessage::Eof);
                                            break;
                                        }
                                        Ok(msg) => {
                                            if stream_tx.send_blocking(msg).is_err() {
                                                break;
                                            }
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                        }
                    });
                Some(stream_rx)
            } else {
                None
            };

            let envelope = IpcEnvelope {
                command,
                reply_tx,
                dmenu_stream_rx,
            };
            if tx.send_blocking(envelope).is_err() {
                DaemonIpcResponse::Error {
                    message: "AIOS UI host is shutting down".to_string(),
                }
            } else {
                let _ = thread::Builder::new()
                    .name("aios-ipc-dmenu-wait".into())
                    .spawn(move || {
                        let response = reply_rx.recv().unwrap_or(DaemonIpcResponse::DmenuCancelled);
                        write_ipc_response(&mut stream, &response);
                    });
                return;
            }
        }
        Ok(command) => {
            let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
            let envelope = IpcEnvelope {
                command,
                reply_tx,
                dmenu_stream_rx: None,
            };
            if tx.send_blocking(envelope).is_err() {
                DaemonIpcResponse::Error {
                    message: "AIOS UI host is shutting down".to_string(),
                }
            } else {
                reply_rx
                    .recv_timeout(IO_TIMEOUT)
                    .unwrap_or_else(|_| DaemonIpcResponse::Error {
                        message: "timed out waiting for AIOS host response".to_string(),
                    })
            }
        }
        Err(err) => DaemonIpcResponse::Error {
            message: format!("invalid IPC command: {err}"),
        },
    };

    write_ipc_response(&mut stream, &response);
}

fn write_ipc_response(stream: &mut UnixStream, response: &DaemonIpcResponse) {
    if let Ok(mut encoded) = serde_json::to_string(response) {
        encoded.push('\n');
        let _ = stream.write_all(encoded.as_bytes());
        let _ = stream.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ipc_round_trip_and_single_instance_lock() {
        let tmp = TempDir::new().expect("temp dir");
        let sock = tmp.path().join("aios.sock");

        let (_server, rx) = IpcServer::bind(&sock).expect("bind server");
        let client = IpcClient::new(&sock);

        // Ping responds directly from accept loop.
        assert!(client.is_daemon_running());

        // Second bind must reject duplicate daemon while first is alive.
        let dup = IpcServer::bind(&sock);
        assert!(dup.is_err(), "duplicate daemon bind must fail");

        // Toggle forwards envelope to receiver and delivers response back to client.
        let handler = thread::spawn(move || {
            let env = rx.recv_blocking().expect("receive toggle");
            assert_eq!(env.command, DaemonIpcCommand::Toggle);
            env.respond(DaemonIpcResponse::Ok);

            let env2 = rx.recv_blocking().expect("receive open");
            assert_eq!(
                env2.command,
                DaemonIpcCommand::Open {
                    uri: "aios://demo/grid".to_string()
                }
            );
            env2.respond(DaemonIpcResponse::Ok);
        });

        assert_eq!(
            client.send(&DaemonIpcCommand::Toggle).expect("toggle"),
            DaemonIpcResponse::Ok
        );
        assert_eq!(
            client
                .send(&DaemonIpcCommand::Open {
                    uri: "aios://demo/grid".to_string(),
                })
                .expect("open"),
            DaemonIpcResponse::Ok
        );
        handler.join().expect("handler thread");
    }

    #[test]
    fn stale_socket_file_is_cleaned_automatically() {
        let tmp = TempDir::new().expect("temp dir");
        let sock = tmp.path().join("stale.sock");

        // Create a leftover dead socket by dropping a raw listener without unlinking.
        let dead = UnixListener::bind(&sock).expect("bind raw socket");
        drop(dead);
        assert!(sock.exists());

        let (server, _rx) = IpcServer::bind(&sock).expect("stale socket should be replaced");
        let client = IpcClient::new(server.socket_path());
        assert!(client.is_daemon_running());
    }

    #[test]
    fn dmenu_ipc_waits_without_blocking_other_requests() {
        use aios_daemon::DmenuRequest;

        let tmp = TempDir::new().expect("temp dir");
        let sock = tmp.path().join("dmenu.sock");

        let (_server, rx) = IpcServer::bind(&sock).expect("bind server");
        let dmenu_client = IpcClient::new(&sock);
        let ping_client = IpcClient::new(&sock);

        let dmenu_thread = thread::spawn(move || {
            dmenu_client
                .send(&DaemonIpcCommand::Dmenu(DmenuRequest {
                    prompt: Some("Screenshot".to_string()),
                    items: vec!["Full".to_string(), "Area".to_string()],
                    lines: Some(2),
                    ..Default::default()
                }))
                .expect("dmenu response")
        });

        let env = rx.recv_blocking().expect("receive dmenu envelope");
        assert!(matches!(env.command, DaemonIpcCommand::Dmenu(_)));

        // While dmenu is open and waiting, Ping must still respond immediately on the accept loop.
        assert!(ping_client.is_daemon_running());

        env.respond(DaemonIpcResponse::DmenuSelected {
            value: "Area".to_string(),
            index: Some(1),
        });

        let reply = dmenu_thread.join().expect("dmenu client thread");
        assert_eq!(
            reply,
            DaemonIpcResponse::DmenuSelected {
                value: "Area".to_string(),
                index: Some(1),
            }
        );
    }

    #[test]
    fn dmenu_streaming_ipc_delivers_batches_and_allows_early_selection() {
        let tmp = TempDir::new().expect("temp dir");
        let sock = tmp.path().join("dmenu-stream.sock");

        let (_server, rx) = IpcServer::bind(&sock).expect("bind server");
        let client = IpcClient::new(&sock);

        let session = client
            .start_dmenu_stream(&DmenuRequest {
                prompt: Some("Find".to_string()),
                streaming: true,
                ..Default::default()
            })
            .expect("start stream");
        let (mut writer, reader) = session.split();

        let env = rx.recv_blocking().expect("receive streaming dmenu");
        let stream_rx = env
            .dmenu_stream_rx
            .clone()
            .expect("streaming receiver present");

        assert!(
            writer
                .send_stream(&DmenuStreamMessage::Batch {
                    items: vec!["camera-photo".to_string(), "edit-cut".to_string()],
                    item_icons: vec![None, None],
                })
                .expect("send batch")
        );

        let batch = stream_rx.recv_blocking().expect("receive batch");
        assert!(matches!(batch, DmenuStreamMessage::Batch { ref items, .. } if items.len() == 2));

        // User selects BEFORE Eof arrives -> client receives selection immediately!
        env.respond(DaemonIpcResponse::DmenuSelected {
            value: "camera-photo".to_string(),
            index: Some(0),
        });
        let resp = reader.wait_response().expect("wait response");
        assert_eq!(
            resp,
            DaemonIpcResponse::DmenuSelected {
                value: "camera-photo".to_string(),
                index: Some(0),
            }
        );
    }
}
