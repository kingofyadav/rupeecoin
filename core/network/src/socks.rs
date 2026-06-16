/// SOCKS5 transport for libp2p — routes all outbound dials through a SOCKS5 proxy.
///
/// This implements the raw `libp2p::core::Transport` trait so it can be used via
/// `SwarmBuilder::with_other_transport()` after applying noise + yamux upgrades.
/// Inbound listening is not supported (returns `MultiaddrNotSupported`).
use std::net::SocketAddr;
use std::task::{Context, Poll};

use std::pin::Pin;

use futures::future::BoxFuture;
use futures::FutureExt;
use libp2p::core::transport::{ListenerId, TransportError, TransportEvent};
use libp2p::{Multiaddr, Transport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncReadCompatExt;

#[derive(Clone)]
pub struct SocksTransport {
    proxy: Option<SocketAddr>,
}

impl SocksTransport {
    /// Create a SOCKS5 transport that routes dials through `proxy`.
    pub fn new(proxy: SocketAddr) -> Self {
        SocksTransport { proxy: Some(proxy) }
    }

    /// Create a no-op transport — all dials return `MultiaddrNotSupported`.
    pub fn disabled() -> Self {
        SocksTransport { proxy: None }
    }

    /// Create from an `Option<SocketAddr>`: active when `Some`, disabled when `None`.
    pub fn from_option(proxy: Option<SocketAddr>) -> Self {
        SocksTransport { proxy }
    }
}

/// Extract (host, port) from a `/ip4/…/tcp/…` or `/dns4/…/tcp/…` Multiaddr.
fn multiaddr_to_host_port(addr: &Multiaddr) -> Option<(String, u16)> {
    use libp2p::multiaddr::Protocol;
    let mut iter = addr.iter();
    let host = match iter.next()? {
        Protocol::Ip4(ip) => ip.to_string(),
        Protocol::Ip6(ip) => ip.to_string(),
        Protocol::Dns(name) | Protocol::Dns4(name) | Protocol::Dns6(name) => name.to_string(),
        _ => return None,
    };
    let port = match iter.next()? {
        Protocol::Tcp(p) => p,
        _ => return None,
    };
    Some((host, port))
}

/// Perform the SOCKS5 CONNECT handshake and return the established `TcpStream`.
async fn socks5_connect(proxy: SocketAddr, host: String, port: u16) -> std::io::Result<TcpStream> {
    let mut s = TcpStream::connect(proxy).await?;

    // Greeting: version 5, 1 method, no-auth.
    s.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut buf = [0u8; 2];
    s.read_exact(&mut buf).await?;
    if buf[0] != 0x05 || buf[1] != 0x00 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            format!("SOCKS5 auth rejected ({:02x} {:02x})", buf[0], buf[1]),
        ));
    }

    // CONNECT request: domain name form.
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "hostname exceeds 255-byte SOCKS5 limit",
        ));
    }
    let mut req = Vec::with_capacity(7 + host_bytes.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    req.extend_from_slice(host_bytes);
    req.push((port >> 8) as u8);
    req.push(port as u8);
    s.write_all(&req).await?;

    // Response.
    let mut resp = [0u8; 4];
    s.read_exact(&mut resp).await?;
    if resp[1] != 0x00 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            format!("SOCKS5 CONNECT rejected with code {:#04x}", resp[1]),
        ));
    }
    // Consume bound-address fields.
    match resp[3] {
        0x01 => { s.read_exact(&mut [0u8; 6]).await?; }
        0x03 => {
            let mut len = [0u8; 1];
            s.read_exact(&mut len).await?;
            let mut tail = vec![0u8; len[0] as usize + 2];
            s.read_exact(&mut tail).await?;
        }
        0x04 => { s.read_exact(&mut [0u8; 18]).await?; }
        _ => {}
    }
    Ok(s)
}

/// Compat wrapper: presents a `tokio::TcpStream` as `futures_io::{AsyncRead, AsyncWrite}`.
pub type SocksStream = tokio_util::compat::Compat<TcpStream>;

#[derive(Debug, thiserror::Error)]
#[error("socks5 transport error: {0}")]
pub struct SocksError(#[from] std::io::Error);

impl Transport for SocksTransport {
    type Output = SocksStream;
    type Error = SocksError;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        _id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        _opts: libp2p::core::transport::DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let proxy = match self.proxy {
            Some(p) => p,
            None => return Err(TransportError::MultiaddrNotSupported(addr)),
        };
        let (host, port) = multiaddr_to_host_port(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;
        Ok(async move {
            let stream = socks5_connect(proxy, host, port).await?;
            Ok(stream.compat())
        }
        .boxed())
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }
}
