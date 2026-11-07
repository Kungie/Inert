//! # inert-transport
//!
//! Frames: `[u32 big-endian length][deterministic CBOR message]` (spec §5.1)
//! carried over TCP + TLS 1.3. Limits are enforced per direction *before*
//! the body is read: everything a client sends is small, and only `welcome`
//! and `asset` may use the large server → client limit.
//!
//! QUIC will later sit behind the same [`Conn`] API.

#![forbid(unsafe_code)]

pub mod tls;

use inert_proto::limits::{FRAME_MAX_SERVER_TO_CLIENT, FRAME_MAX_SMALL, HANDSHAKE_TIMEOUT_SECS, IDLE_TIMEOUT_SECS, READ_TIMEOUT_SECS};
use inert_proto::{DecodeError, ErrorCode, Message};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub use rustls;
pub use tokio_rustls;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame of {0} bytes exceeds the limit")]
    FrameTooLarge(usize),
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
    #[error("timed out")]
    Timeout,
    #[error("connection closed")]
    Closed,
    #[error("tls: {0}")]
    Tls(String),
}

impl TransportError {
    /// The fatal code to send to the peer for this failure, if any.
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            TransportError::FrameTooLarge(_) => Some(ErrorCode::FrameTooLarge),
            TransportError::Decode(d) => Some(d.code()),
            TransportError::Timeout => Some(ErrorCode::Timeout),
            _ => None,
        }
    }
}

/// Which side of the connection we are; decides incoming frame limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

impl Role {
    fn incoming_max(self) -> usize {
        match self {
            Role::Client => FRAME_MAX_SERVER_TO_CLIENT,
            Role::Server => FRAME_MAX_SMALL,
        }
    }
}

/// Reads one frame body. Fails with `FrameTooLarge` before reading the body.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> Result<Vec<u8>, TransportError> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Err(TransportError::Closed),
        Err(e) => return Err(e.into()),
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > max {
        return Err(TransportError::FrameTooLarge(n));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).await?;
    Ok(body)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, body: &[u8]) -> Result<(), TransportError> {
    let n = u32::try_from(body.len()).map_err(|_| TransportError::FrameTooLarge(body.len()))?;
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&n.to_be_bytes());
    buf.extend_from_slice(body);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// A message connection over any byte stream.
pub struct Conn<S> {
    stream: S,
    role: Role,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Conn<S> {
    pub fn new(stream: S, role: Role) -> Self {
        Self { stream, role }
    }

    pub async fn send(&mut self, m: &Message) -> Result<(), TransportError> {
        let body = m.to_bytes();
        let max = if self.role == Role::Server && m.is_large() { FRAME_MAX_SERVER_TO_CLIENT } else { FRAME_MAX_SMALL };
        if body.len() > max {
            return Err(TransportError::FrameTooLarge(body.len()));
        }
        tokio::time::timeout(Duration::from_secs(READ_TIMEOUT_SECS), write_frame(&mut self.stream, &body))
            .await
            .map_err(|_| TransportError::Timeout)?
    }

    /// Receives one message. `idle` waits up to the idle timeout for the
    /// frame to *start*; once started, the whole frame must arrive within the
    /// read timeout.
    pub async fn recv(&mut self, idle: bool) -> Result<Message, TransportError> {
        let wait = if idle { IDLE_TIMEOUT_SECS } else { READ_TIMEOUT_SECS };
        let max = self.role.incoming_max();
        let body = tokio::time::timeout(Duration::from_secs(wait), read_frame(&mut self.stream, max))
            .await
            .map_err(|_| TransportError::Timeout)??;
        let msg = Message::from_bytes(&body)?;
        if self.role == Role::Client && !msg.is_large() && body.len() > FRAME_MAX_SMALL {
            return Err(TransportError::FrameTooLarge(body.len()));
        }
        Ok(msg)
    }

    /// Best-effort fatal notice before closing.
    pub async fn fatal(&mut self, code: ErrorCode) {
        let _ = self.send(&Message::Fatal { code }).await;
        let _ = self.stream.shutdown().await;
    }

    pub fn into_inner(self) -> S {
        self.stream
    }
}

pub type ClientConn = Conn<tokio_rustls::client::TlsStream<TcpStream>>;
pub type ServerConn = Conn<tokio_rustls::server::TlsStream<TcpStream>>;

/// Dials `addr`, completes TLS for `server_name`, returns a client connection.
pub async fn connect(addr: &str, server_name: &str, config: Arc<rustls::ClientConfig>) -> Result<ClientConn, TransportError> {
    let fut = async {
        let tcp = TcpStream::connect(addr).await?;
        tcp.set_nodelay(true)?;
        let name = rustls_pki_types::ServerName::try_from(server_name.to_owned()).map_err(|e| TransportError::Tls(e.to_string()))?;
        let tls = tokio_rustls::TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        Ok::<_, TransportError>(Conn::new(tls, Role::Client))
    };
    tokio::time::timeout(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS), fut).await.map_err(|_| TransportError::Timeout)?
}

/// Accepts and TLS-handshakes one connection.
pub async fn accept(listener: &TcpListener, acceptor: &tokio_rustls::TlsAcceptor) -> Result<(ServerConn, std::net::SocketAddr), TransportError> {
    let (tcp, peer) = listener.accept().await?;
    tcp.set_nodelay(true)?;
    let tls = tokio::time::timeout(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS), acceptor.accept(tcp))
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(|e| TransportError::Tls(e.to_string()))?;
    Ok((Conn::new(tls, Role::Server), peer))
}

/// Completes the TLS handshake for an already-accepted TCP stream.
pub async fn handshake(tcp: TcpStream, acceptor: &tokio_rustls::TlsAcceptor) -> Result<ServerConn, TransportError> {
    tcp.set_nodelay(true)?;
    let tls = tokio::time::timeout(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS), acceptor.accept(tcp))
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(|e| TransportError::Tls(e.to_string()))?;
    Ok(Conn::new(tls, Role::Server))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frame_limit_is_checked_before_reading_body() {
        let (mut a, mut b) = tokio::io::duplex(64);
        // Claim a 1 GiB body but send nothing else.
        a.write_all(&(1u32 << 30).to_be_bytes()).await.unwrap();
        let r = read_frame(&mut b, FRAME_MAX_SMALL).await;
        assert!(matches!(r, Err(TransportError::FrameTooLarge(n)) if n == 1 << 30));
    }

    #[tokio::test]
    async fn roundtrip_over_duplex() {
        let (a, b) = tokio::io::duplex(1 << 16);
        let mut client = Conn::new(a, Role::Client);
        let mut server = Conn::new(b, Role::Server);
        client.send(&Message::Hello { v: 1 }).await.unwrap();
        assert_eq!(server.recv(false).await.unwrap(), Message::Hello { v: 1 });
        server.send(&Message::Fatal { code: ErrorCode::Timeout }).await.unwrap();
        assert_eq!(client.recv(false).await.unwrap(), Message::Fatal { code: ErrorCode::Timeout });
    }
}
