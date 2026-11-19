//! The review agent's egress allowlist (D-04): a minimal loopback HTTP
//! `CONNECT` proxy that admits only allowlisted `host:port` pairs, the
//! model-provider endpoint and the daemon's run API.
//!
//! The agent's environment points `HTTPS_PROXY`/`HTTP_PROXY` here. Residual:
//! a process that ignores the proxy variables is not blocked by this proxy.
//! The mitigations are that the agent has no network tools at all, and a
//! host firewall when the operator wants one.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The largest request head the proxy reads.
pub const MAX_HEAD_BYTES: usize = 8 * 1024;
/// How long an upstream connect may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The Anthropic API, when `ANTHROPIC_BASE_URL` names no other endpoint.
pub const DEFAULT_PROVIDER: &str = "api.anthropic.com:443";

/// A running proxy; it stops when dropped.
pub struct EgressProxy {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl EgressProxy {
    /// Listen on an ephemeral loopback port, admitting only `allow`
    /// (`host:port`, compared case-insensitively).
    pub async fn start(allow: &[String]) -> std::io::Result<EgressProxy> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let allow: BTreeSet<String> = allow.iter().map(|a| a.to_ascii_lowercase()).collect();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let allow = allow.clone();
                tokio::spawn(async move {
                    let _ = serve(stream, &allow).await;
                });
            }
        });
        Ok(EgressProxy { addr, task })
    }

    /// The proxy URL for `HTTPS_PROXY` and `HTTP_PROXY`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The provider endpoint as `host:port`, from an `ANTHROPIC_BASE_URL` value.
pub fn provider_endpoint(base_url: Option<&str>) -> String {
    let Some(url) = base_url.filter(|u| !u.trim().is_empty()) else {
        return DEFAULT_PROVIDER.to_string();
    };
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if authority
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        return authority.to_ascii_lowercase();
    }
    let port = if scheme.eq_ignore_ascii_case("http") {
        80
    } else {
        443
    };
    format!("{}:{port}", authority.to_ascii_lowercase())
}

/// Serve one client: tunnel an allowlisted `CONNECT`, refuse anything else.
async fn serve(mut client: TcpStream, allow: &BTreeSet<String>) -> std::io::Result<()> {
    let (head, rest) = read_head(&mut client).await?;
    let target = connect_target(&head).filter(|t| allow.contains(&t.to_ascii_lowercase()));
    let Some(target) = target else {
        tracing::info!(audit = "review.egress.refused", "egress refused");
        return client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await;
    };
    let upstream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&target)).await;
    let Ok(Ok(mut upstream)) = upstream else {
        return client
            .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            .await;
    };
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    upstream.write_all(&rest).await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// The request head up to the blank line, and any bytes read past it.
async fn read_head(client: &mut TcpStream) -> std::io::Result<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((String::from_utf8_lossy(&buf).to_string(), rest));
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err(std::io::Error::other("request head too large"));
        }
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// The `host:port` of a `CONNECT host:port HTTP/1.x` request line.
fn connect_target(head: &str) -> Option<String> {
    let mut parts = head.lines().next()?.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    let valid = method == "CONNECT" && version.starts_with("HTTP/1.");
    let port_ok = target
        .rsplit_once(':')
        .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok());
    (valid && port_ok).then(|| target.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_endpoint_comes_from_the_base_url() {
        assert_eq!(provider_endpoint(None), DEFAULT_PROVIDER);
        assert_eq!(provider_endpoint(Some("")), DEFAULT_PROVIDER);
        assert_eq!(
            provider_endpoint(Some("https://Gateway.example.com/v1")),
            "gateway.example.com:443"
        );
        assert_eq!(
            provider_endpoint(Some("http://127.0.0.1:8080/api")),
            "127.0.0.1:8080"
        );
    }

    #[test]
    fn only_a_well_formed_connect_names_a_target() {
        assert_eq!(
            connect_target("CONNECT api.example.com:443 HTTP/1.1\r\nHost: x"),
            Some("api.example.com:443".into())
        );
        for bad in [
            "GET http://api.example.com/ HTTP/1.1",
            "CONNECT api.example.com HTTP/1.1",
            "CONNECT :443 HTTP/1.1",
            "CONNECT a:443 SPDY/3",
        ] {
            assert_eq!(connect_target(bad), None, "{bad}");
        }
    }
}
