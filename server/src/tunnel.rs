use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use governor::{DefaultKeyedRateLimiter, Quota};
use sable_link_preview::{is_public_address, parse_public_url};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::{AppState, matrix, query_param};

/// The client offers this and its ticket as WebSocket subprotocols, since a browser
/// cannot set other headers and a URL ends up in logs.
const PROTOCOL: &str = "sable-tunnel";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

const IDLE_TIMEOUT: Duration = Duration::from_secs(20);

/// A preview sends a TLS handshake and a GET, so anything larger is not a preview.
const MAX_UPLOAD_BYTES: usize = 64 * 1024;

const MAX_DOWNLOAD_BYTES: usize = 100 * 1024 * 1024;

const TUNNELS_PER_MINUTE: NonZeroU32 = NonZeroU32::new(240).unwrap();

const TUNNEL_BURST: NonZeroU32 = NonZeroU32::new(60).unwrap();

pub(crate) struct Tunnels {
    per_ip: DefaultKeyedRateLimiter<IpAddr>,
}

impl Tunnels {
    pub(crate) fn new() -> Self {
        Self {
            per_ip: DefaultKeyedRateLimiter::dashmap(
                Quota::per_minute(TUNNELS_PER_MINUTE).allow_burst(TUNNEL_BURST),
            ),
        }
    }

    pub(crate) fn forget_idle_clients(&self) {
        self.per_ip.retain_recent();
        self.per_ip.shrink_to_fit();
    }
}

fn target(query: Option<&str>) -> Option<(String, u16)> {
    let host = query_param(query, "host")?;
    let port: u16 = query_param(query, "port")?.parse().ok()?;
    if !matches!(port, 80 | 443) {
        return None;
    }
    let scheme = if port == 443 { "https" } else { "http" };
    let url = parse_public_url(&format!("{scheme}://{host}/"), None)?;
    (url.host_str() == Some(host.as_str()) && url.port().is_none() && url.username().is_empty())
        .then_some((host, port))
}

/// Connects to an address this function resolved and checked, so a name that resolves
/// differently a moment later cannot reach a private address.
async fn connect(host: &str, port: u16) -> Option<TcpStream> {
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .filter(|address| is_public_address(address.ip()))
        .collect();
    for address in addresses {
        if let Ok(Ok(stream)) =
            tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await
        {
            return Some(stream);
        }
    }
    None
}

pub(crate) async fn tunnel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    upgrade: WebSocketUpgrade,
) -> Response {
    let query = query.as_deref();
    let ticket = upgrade
        .requested_protocols()
        .filter_map(|protocol| protocol.to_str().ok())
        .find(|protocol| *protocol != PROTOCOL);
    if !ticket.is_some_and(|ticket| state.tickets.accepts(ticket)) {
        state.metrics.tunnels_unauthorized.inc();
        return matrix::error(
            StatusCode::UNAUTHORIZED,
            "M_UNAUTHORIZED",
            "A valid ticket is required",
        );
    }
    if let Some(client) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        && state.tunnels.per_ip.check_key(&client).is_err()
    {
        state.metrics.tunnels_limited.inc();
        return matrix::error(
            StatusCode::TOO_MANY_REQUESTS,
            "M_LIMIT_EXCEEDED",
            "Too many tunnels from this address",
        );
    }
    let Some((host, port)) = target(query) else {
        state.metrics.tunnels_invalid.inc();
        return matrix::invalid_param("host must be a public host name and port 80 or 443");
    };
    let Some(origin) = connect(&host, port).await else {
        state.metrics.tunnels_unreachable.inc();
        return matrix::error(
            StatusCode::BAD_GATEWAY,
            "M_UNKNOWN",
            "The host could not be reached",
        );
    };
    upgrade
        .protocols([PROTOCOL])
        .max_message_size(MAX_UPLOAD_BYTES)
        .on_upgrade(move |socket| relay(socket, origin, state))
}

async fn relay(mut socket: WebSocket, mut origin: TcpStream, state: Arc<AppState>) {
    let metrics = &state.metrics;
    metrics.tunnels_opened.inc();
    let mut uploaded = 0;
    let mut downloaded = 0;
    let mut chunk = vec![0; 16 * 1024];
    loop {
        let step = tokio::time::timeout(IDLE_TIMEOUT, async {
            tokio::select! {
                message = socket.recv() => Step::FromClient(message),
                read = origin.read(&mut chunk) => Step::FromOrigin(read),
            }
        })
        .await;
        match step {
            Ok(Step::FromClient(Some(Ok(Message::Binary(data))))) => {
                uploaded += data.len();
                metrics.tunnel_bytes_up.add(data.len());
                if uploaded > MAX_UPLOAD_BYTES || origin.write_all(&data).await.is_err() {
                    break;
                }
            }
            Ok(Step::FromClient(Some(Ok(Message::Ping(_) | Message::Pong(_))))) => {}
            Ok(Step::FromOrigin(Ok(read))) if read > 0 => {
                downloaded += read;
                metrics.tunnel_bytes_down.add(read);
                let Some(data) = chunk.get(..read) else {
                    break;
                };
                if downloaded > MAX_DOWNLOAD_BYTES
                    || socket
                        .send(Message::Binary(data.to_vec().into()))
                        .await
                        .is_err()
                {
                    break;
                }
            }
            _ => break,
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    metrics.tunnels_closed.inc();
}

enum Step {
    FromClient(Option<Result<Message, axum::Error>>),
    FromOrigin(std::io::Result<usize>),
}

#[cfg(test)]
mod tests {
    use super::target;

    #[test]
    fn only_public_names_on_web_ports_are_targets() {
        assert_eq!(
            target(Some("host=example.com&port=443")),
            Some(("example.com".to_owned(), 443))
        );
        assert_eq!(
            target(Some("host=example.com&port=80")),
            Some(("example.com".to_owned(), 80))
        );
        for refused in [
            "host=example.com&port=22",
            "host=example.com&port=8080",
            "host=example.com",
            "port=443",
            "host=localhost&port=443",
            "host=127.0.0.1&port=443",
            "host=10.0.0.1&port=443",
            "host=%5B%3A%3A1%5D&port=443",
            "host=printer.local&port=443",
            "host=example.com%3A22&port=443",
            "host=example.com%2Fx&port=443",
            "host=user%40example.com&port=443",
        ] {
            assert_eq!(target(Some(refused)), None, "{refused}");
        }
    }
}
