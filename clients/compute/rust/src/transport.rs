// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! Connecting and serving: a Unix domain socket on one machine where the platform has them
//! (owner-only permissions, no network port), loopback TCP otherwise; a bearer token on every
//! call; and reason codes on errors.

use std::net::SocketAddr;
#[cfg(unix)]
use std::path::PathBuf;

use tonic::metadata::MetadataMap;
use tonic::service::Interceptor;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint, Server};
use tonic::{Code, Request, Status};

use crate::pb::compute_client::ComputeClient;
use crate::pb::compute_server::{Compute, ComputeServer};

/// Where a server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    #[cfg(unix)]
    Unix(PathBuf),
    Tcp(SocketAddr),
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Address::Unix(p) => write!(f, "unix:{}", p.display()),
            Address::Tcp(a) => write!(f, "tcp:{a}"),
        }
    }
}

impl std::str::FromStr for Address {
    type Err = String;
    fn from_str(s: &str) -> Result<Address, String> {
        if let Some(p) = s.strip_prefix("unix:") {
            #[cfg(unix)]
            return Ok(Address::Unix(p.into()));
            #[cfg(not(unix))]
            return Err(format!("unix sockets are not available here: {p}"));
        }
        if let Some(a) = s.strip_prefix("tcp:") {
            return a.parse().map(Address::Tcp).map_err(|e| e.to_string());
        }
        Err(format!("not an address: {s}"))
    }
}

/// Adds the bearer token to every call.
#[derive(Debug, Clone)]
pub struct Auth(pub String);

impl Interceptor for Auth {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        let v = format!("Bearer {}", self.0)
            .parse()
            .map_err(|_| Status::internal("token is not valid metadata"))?;
        req.metadata_mut().insert("authorization", v);
        Ok(req)
    }
}

/// A connected client.
pub type Client = ComputeClient<InterceptedService<Channel, Auth>>;

/// Connects to a server; messages up to 64 MiB.
pub async fn connect(addr: &Address, token: &str) -> Result<Client, tonic::transport::Error> {
    let channel = match addr {
        #[cfg(unix)]
        Address::Unix(path) => {
            let path = path.clone();
            Endpoint::from_static("http://localhost")
                .connect_with_connector(tower::service_fn(move |_| {
                    let path = path.clone();
                    async move {
                        let s = tokio::net::UnixStream::connect(path).await?;
                        Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(s))
                    }
                }))
                .await?
        }
        Address::Tcp(a) => {
            Endpoint::from_shared(format!("http://{a}"))?
                .connect()
                .await?
        }
    };
    Ok(ComputeClient::with_interceptor(channel, Auth(token.into()))
        .max_decoding_message_size(64 << 20)
        .max_encoding_message_size(64 << 20))
}

/// A bound listener and the address it is reachable at.
#[derive(Debug)]
pub enum Listener {
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),
    Tcp(tokio::net::TcpListener),
}

/// Binds `addr`; a TCP port 0 is replaced by the port chosen. A Unix socket left over from a
/// server that has gone is removed first, and the new one is made owner-only.
pub async fn bind(addr: &Address) -> std::io::Result<(Listener, Address)> {
    match addr {
        #[cfg(unix)]
        Address::Unix(path) => {
            use std::os::unix::fs::PermissionsExt;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if path.exists() && std::os::unix::net::UnixStream::connect(path).is_err() {
                std::fs::remove_file(path)?;
            }
            let l = tokio::net::UnixListener::bind(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok((Listener::Unix(l), addr.clone()))
        }
        Address::Tcp(a) => {
            let l = tokio::net::TcpListener::bind(a).await?;
            let actual = l.local_addr()?;
            Ok((Listener::Tcp(l), Address::Tcp(actual)))
        }
    }
}

/// Checks the bearer token on every call.
pub fn check_token(token: String) -> impl Interceptor + Clone + Send + 'static {
    let expected = format!("Bearer {token}");
    move |req: Request<()>| -> Result<Request<()>, Status> {
        match req.metadata().get("authorization") {
            Some(v) if v.as_bytes() == expected.as_bytes() => Ok(req),
            _ => Err(status(
                Code::Unauthenticated,
                "unauthorised",
                "missing or wrong token",
            )),
        }
    }
}

/// Serves `service` on `listener` until `shutdown` completes.
pub async fn serve<S: Compute>(
    listener: Listener,
    token: String,
    service: S,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> Result<(), tonic::transport::Error> {
    let svc = InterceptedService::new(
        ComputeServer::new(service)
            .max_decoding_message_size(64 << 20)
            .max_encoding_message_size(64 << 20),
        check_token(token),
    );
    let router = Server::builder().add_service(svc);
    match listener {
        #[cfg(unix)]
        Listener::Unix(l) => {
            router
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::UnixListenerStream::new(l),
                    shutdown,
                )
                .await
        }
        Listener::Tcp(l) => {
            router
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(l),
                    shutdown,
                )
                .await
        }
    }
}

/// A status carrying a reason code in the `reviera-reason` trailer.
pub fn status(code: Code, reason: &str, message: impl Into<String>) -> Status {
    let mut md = MetadataMap::new();
    if let Ok(v) = reason.parse() {
        md.insert("reviera-reason", v);
    }
    Status::with_metadata(code, message, md)
}

/// The reason code of a status, if it carries one.
pub fn reason(s: &Status) -> Option<String> {
    s.metadata()
        .get("reviera-reason")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// A random token: 32 bytes from the operating system, as hex.
pub fn new_token() -> String {
    use std::io::Read;
    let mut b = [0u8; 32];
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom")
            && f.read_exact(&mut b).is_ok()
        {
            return hex::encode(b);
        }
    }
    // Fallback: hash of time, process and address entropy.
    let seed = format!(
        "{:?}{}{:p}",
        std::time::SystemTime::now(),
        std::process::id(),
        &b
    );
    crate::blob::sha256_hex(seed.as_bytes())
}
