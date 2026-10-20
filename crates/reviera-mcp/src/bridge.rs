// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The local bridge (DM-MCP-002, UI-AGENT-004): agents that start stdio MCP servers reach the
//! running window's model through `reviera mcp-bridge`.
//!
//! - The window opens an [`Endpoint`]: a Unix-domain socket (mode 0600, in a directory only the
//!   user can enter) on Linux and macOS, a named pipe on Windows. No network port is opened.
//! - Each terminal tab gets a session: a one-time secret issued with its permission level and
//!   name ([`Endpoint::issue`]). A connection must first send `REVIERA-MCP <secret>\n`; a wrong
//!   or revoked secret is refused (`ERR …`) and counted; a good one gets `OK\n` and is then
//!   served MCP with the session's configuration.
//! - [`run_bridge`] is the client side: it connects, sends the secret, and relays stdin to the
//!   endpoint and the endpoint to stdout until either side closes.
//!
//! The endpoint and the secret reach the bridge in its environment (`REVIERA_MCP_ENDPOINT`,
//! `REVIERA_MCP_SECRET`), never on its command line, so other users' `ps` cannot see them.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rmcp::ServiceExt;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::queue::AppHandle;
use crate::server::{McpConfig, McpServer};

pub const ENDPOINT_ENV: &str = "REVIERA_MCP_ENDPOINT";
pub const SECRET_ENV: &str = "REVIERA_MCP_SECRET";
const HELLO: &str = "REVIERA-MCP ";

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The window's end of the bridge.
pub struct Endpoint {
    /// The socket path or pipe name.
    pub address: String,
    sessions: Arc<Mutex<HashMap<String, McpConfig>>>,
    refused: Arc<AtomicUsize>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("address", &self.address).finish()
    }
}

/// A random secret: two v4 UUIDs (244 random bits).
fn secret() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Serves one connection: the handshake, then MCP.
async fn serve<S>(stream: S, app: AppHandle, sessions: Arc<Mutex<HashMap<String, McpConfig>>>, refused: Arc<AtomicUsize>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    let ok = tokio::time::timeout(std::time::Duration::from_secs(10), reader.read_line(&mut line)).await;
    let given = line.trim_end().strip_prefix(HELLO).map(str::to_owned);
    let config = match (ok, given) {
        (Ok(Ok(_)), Some(s)) => sessions.lock().expect("sessions").get(&s).cloned(),
        _ => None,
    };
    let Some(config) = config else {
        refused.fetch_add(1, Ordering::Relaxed);
        eprintln!("reviera: an MCP bridge connection without a valid session secret was refused");
        let _ = write.write_all(b"ERR unauthorised\n").await;
        return;
    };
    if write.write_all(b"OK\n").await.is_err() {
        return;
    }
    let server = McpServer::new(app, config);
    if let Ok(running) = server.serve((reader, write)).await {
        let _ = running.waiting().await;
    }
}

impl Endpoint {
    /// Opens the endpoint in `dir` (made private to the user) and serves connections on its own
    /// threads, running the calls on `app`.
    pub fn start(app: AppHandle, dir: &Path) -> Result<Self, BoxError> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("reviera-mcp-bridge").build()?;
        let sessions: Arc<Mutex<HashMap<String, McpConfig>>> = Arc::default();
        let refused = Arc::new(AtomicUsize::new(0));
        let address = Self::listen(&runtime, app, dir, sessions.clone(), refused.clone())?;
        Ok(Self { address, sessions, refused, runtime: Some(runtime) })
    }

    #[cfg(unix)]
    fn listen(
        runtime: &tokio::runtime::Runtime,
        app: AppHandle,
        dir: &Path,
        sessions: Arc<Mutex<HashMap<String, McpConfig>>>,
        refused: Arc<AtomicUsize>,
    ) -> Result<String, BoxError> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let path: std::path::PathBuf = dir.join(format!("mcp-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = {
            let _guard = runtime.enter();
            tokio::net::UnixListener::bind(&path)?
        };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        runtime.spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, app.clone(), sessions.clone(), refused.clone()));
            }
        });
        Ok(path.to_string_lossy().into_owned())
    }

    #[cfg(windows)]
    fn listen(
        runtime: &tokio::runtime::Runtime,
        app: AppHandle,
        _dir: &Path,
        sessions: Arc<Mutex<HashMap<String, McpConfig>>>,
        refused: Arc<AtomicUsize>,
    ) -> Result<String, BoxError> {
        use tokio::net::windows::named_pipe::ServerOptions;
        // A pipe name no other process can guess; the secret still guards each session.
        let name = format!(r"\\.\pipe\reviera-mcp-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple());
        let first = {
            let _guard = runtime.enter();
            ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?
        };
        let pipe = name.clone();
        runtime.spawn(async move {
            let mut server = first;
            loop {
                if server.connect().await.is_err() {
                    break;
                }
                let connected = server;
                server = match ServerOptions::new().reject_remote_clients(true).create(&pipe) {
                    Ok(s) => s,
                    Err(_) => break,
                };
                tokio::spawn(serve(connected, app.clone(), sessions.clone(), refused.clone()));
            }
        });
        Ok(name)
    }

    /// A session for one agent: its secret.
    pub fn issue(&self, config: McpConfig) -> String {
        let s = secret();
        self.sessions.lock().expect("sessions").insert(s.clone(), config);
        s
    }

    /// Ends a session: its secret no longer connects (open connections end when the bridge
    /// exits with its process).
    pub fn revoke(&self, secret: &str) {
        self.sessions.lock().expect("sessions").remove(secret);
    }

    /// Changes a session's permission for its next connection and calls.
    pub fn set_config(&self, secret: &str, config: McpConfig) {
        if let Some(c) = self.sessions.lock().expect("sessions").get_mut(secret) {
            *c = config;
        }
    }

    /// Connections refused for a missing or wrong secret.
    pub fn refused(&self) -> usize {
        self.refused.load(Ordering::Relaxed)
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        // Safe to drop from any context, async ones included.
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.address);
    }
}

/// Relays stdin and stdout to the endpoint (`reviera mcp-bridge`).
pub async fn run_bridge(address: &str, secret: &str) -> Result<(), BoxError> {
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(address).await?;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(address)?;
    let (read, mut write) = tokio::io::split(stream);
    write.write_all(format!("{HELLO}{secret}\n").as_bytes()).await?;
    let mut reader = BufReader::new(read);
    let mut answer = String::new();
    reader.read_line(&mut answer).await?;
    if answer.trim_end() != "OK" {
        return Err(format!("the Reviera window refused the bridge: {}", answer.trim()).into());
    }
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let up = async {
        let r = tokio::io::copy(&mut stdin, &mut write).await;
        let _ = write.shutdown().await;
        r
    };
    let down = async {
        let r = tokio::io::copy(&mut reader, &mut stdout).await;
        let _ = stdout.flush().await;
        r
    };
    // Either side closing ends the bridge.
    tokio::select! {
        r = up => { r?; }
        r = down => { r?; }
    }
    Ok(())
}
