#![allow(dead_code)] // connect_via_socks5 will be used in Phase 14 transport hook.

/// Phase 13.3 — Tor / SOCKS5 anonymity layer.
///
/// When `--tor-socks <host:port>` is passed, the node routes outgoing peer
/// connections through the specified SOCKS5 proxy (Tor's default is `127.0.0.1:9050`).
///
/// # Limitations (Phase 13)
/// libp2p's internal TCP dialer is NOT automatically proxied in this release;
/// the SOCKS5 layer wraps direct dial-out to `--peer` addresses and any future
/// outbound dialer hooks.  Full transport-level proxying requires integrating
/// `libp2p-socks` (planned for Phase 14).
///
/// # Usage
/// ```
/// rupeecoin-node --tor-socks 127.0.0.1:9050 --peer /dns4/xyz.onion/tcp/30333
/// ```
use std::net::SocketAddr;
use std::str::FromStr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ── Configuration ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct TorConfig {
    pub proxy_addr: SocketAddr,
}

impl TorConfig {
    pub fn parse(s: &str) -> Result<Self, String> {
        SocketAddr::from_str(s)
            .map(|a| TorConfig { proxy_addr: a })
            .map_err(|e| format!("invalid --tor-socks address '{s}': {e}"))
    }
}

// ── SOCKS5 dialer ─────────────────────────────────────────────────────────────

/// Open a SOCKS5 tunnel to `target_host:target_port` through the proxy.
///
/// Returns a `TcpStream` that is transparently connected to the target via the
/// proxy and ready for application-level I/O.
pub async fn connect_via_socks5(
    proxy: SocketAddr,
    target_host: &str,
    target_port: u16,
) -> std::io::Result<TcpStream> {
    let mut s = TcpStream::connect(proxy).await?;

    // ── SOCKS5 greeting: NO AUTH ───────────────────────────────────────────
    s.write_all(&[
        0x05, // version
        0x01, // num methods
        0x00, // method: no authentication
    ])
    .await?;

    let mut buf = [0u8; 2];
    s.read_exact(&mut buf).await?;
    if buf[0] != 0x05 || buf[1] != 0x00 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            format!(
                "SOCKS5 handshake failed (got {:02x} {:02x}); proxy may require authentication",
                buf[0], buf[1]
            ),
        ));
    }

    // ── SOCKS5 CONNECT request ─────────────────────────────────────────────
    let host_bytes = target_host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target hostname exceeds 255-byte SOCKS5 limit",
        ));
    }

    // Build: VER CMD RSV ATYP LEN[host...] PORT_HI PORT_LO
    let mut req = Vec::with_capacity(7 + host_bytes.len());
    req.extend_from_slice(&[
        0x05, // version
        0x01, // CMD = CONNECT
        0x00, // reserved
        0x03, // ATYP = domain name
        host_bytes.len() as u8,
    ]);
    req.extend_from_slice(host_bytes);
    req.push((target_port >> 8) as u8);
    req.push(target_port as u8);
    s.write_all(&req).await?;

    // ── SOCKS5 response ────────────────────────────────────────────────────
    let mut resp = [0u8; 4];
    s.read_exact(&mut resp).await?;
    if resp[1] != 0x00 {
        let reason = match resp[1] {
            0x01 => "general failure",
            0x02 => "connection not allowed by ruleset",
            0x03 => "network unreachable",
            0x04 => "host unreachable",
            0x05 => "connection refused",
            0x06 => "TTL expired",
            0x07 => "command not supported",
            0x08 => "address type not supported",
            c => return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("SOCKS5 CONNECT rejected with code {c:#04x}"),
            )),
        };
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            format!("SOCKS5 CONNECT rejected: {reason}"),
        ));
    }

    // Consume the bound address returned in the response.
    match resp[3] {
        0x01 => {
            s.read_exact(&mut [0u8; 6]).await?; // IPv4 (4 bytes) + port (2 bytes)
        }
        0x03 => {
            let mut len = [0u8; 1];
            s.read_exact(&mut len).await?;
            let mut tail = vec![0u8; len[0] as usize + 2]; // domain + port
            s.read_exact(&mut tail).await?;
        }
        0x04 => {
            s.read_exact(&mut [0u8; 18]).await?; // IPv6 (16 bytes) + port (2 bytes)
        }
        atyp => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown SOCKS5 ATYP {atyp:#04x} in CONNECT response"),
            ));
        }
    }

    Ok(s)
}

// ── Connectivity check ────────────────────────────────────────────────────────

/// Verify that the SOCKS5 proxy is reachable and speaks SOCKS5.
///
/// Returns `Ok(())` if the proxy accepts the TCP connection and acknowledges the
/// SOCKS5 greeting.  Returns `Err(description)` if the proxy is unreachable or
/// not a SOCKS5 server.
pub async fn check_proxy_reachability(proxy: SocketAddr) -> Result<(), String> {
    let mut s = TcpStream::connect(proxy)
        .await
        .map_err(|e| format!("Cannot reach Tor SOCKS5 proxy at {proxy}: {e}"))?;

    s.write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| format!("Write to Tor SOCKS5 proxy at {proxy} failed: {e}"))?;

    let mut buf = [0u8; 2];
    s.read_exact(&mut buf)
        .await
        .map_err(|e| format!("Tor SOCKS5 proxy at {proxy} did not respond: {e}"))?;

    if buf[0] != 0x05 || buf[1] != 0x00 {
        return Err(format!(
            "SOCKS5 handshake failed with proxy {proxy}: got [{:#04x}, {:#04x}]; \
             check that Tor is running and --tor-socks points at the SOCKS port",
            buf[0], buf[1]
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tor_config_parse_valid() {
        let cfg = TorConfig::parse("127.0.0.1:9050").unwrap();
        assert_eq!(cfg.proxy_addr.port(), 9050);
    }

    #[test]
    fn tor_config_parse_invalid() {
        assert!(TorConfig::parse("not-an-addr").is_err());
        assert!(TorConfig::parse("127.0.0.1").is_err()); // missing port
    }
}
