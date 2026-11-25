//! Single-instance Unix domain socket IPC server and fast CLI client (`$XDG_RUNTIME_DIR/aios.sock`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use aios_core::{AiosError, Result};
use aios_daemon::{DaemonIpcCommand, DaemonIpcResponse};

const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// An incoming command from a CLI client paired with a one-shot response channel.
#[derive(Debug)]
pub struct IpcEnvelope {
    pub command: DaemonIpcCommand,
    reply_tx: std::sync::mpsc::SyncSender<DaemonIpcResponse>,
}

impl IpcEnvelope {
    /// Sends the response back to the waiting CLI client.
    pub fn respond(self, response: DaemonIpcResponse) {
        let _ = self.reply_tx.send(response);
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
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
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
        listener
            .set_nonblocking(true)
            .map_err(|source| AiosError::IoPath {
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
                handle_connection(stream, &tx);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(12));
            }
            Err(err) => {
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

    let mut line = String::new();
    {
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut line).is_err() {
            return;
        }
    }

    let response = match serde_json::from_str::<DaemonIpcCommand>(line.trim()) {
        Ok(DaemonIpcCommand::Ping) => DaemonIpcResponse::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Ok(command) => {
            let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
            let envelope = IpcEnvelope { command, reply_tx };
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

    if let Ok(mut encoded) = serde_json::to_string(&response) {
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
}
