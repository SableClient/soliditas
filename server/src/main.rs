mod gifs;
mod matrix;
mod media;
mod metrics;
mod mxc;
mod preview;
mod provider;
mod ticket;
mod tiktok;
mod tunnel;
mod youtube;

use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::header::HOST;
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::metrics::Metrics;
use crate::mxc::to_matrix_id;
use crate::preview::Previews;
use crate::provider::Providers;
use crate::ticket::Tickets;
use crate::tunnel::Tunnels;

pub(crate) struct AppState {
    server_name: Option<String>,
    hostname: Option<String>,
    port: Option<String>,
    previews: Previews,
    providers: Providers,
    tickets: Tickets,
    tunnels: Tunnels,
    metrics: Metrics,
}

impl AppState {
    fn server_name(&self, uri: &Uri, headers: &HeaderMap) -> String {
        self.server_name
            .clone()
            .unwrap_or_else(|| request_host(uri, headers).0)
    }
}

fn request_host(uri: &Uri, headers: &HeaderMap) -> (String, Option<u16>) {
    if let Some(authority) = uri.authority() {
        return (authority.host().to_owned(), authority.port_u16());
    }
    headers
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| host.parse::<axum::http::uri::Authority>().ok())
        .map_or_else(
            || (String::new(), None),
            |authority| (authority.host().to_owned(), authority.port_u16()),
        )
}

pub(crate) fn client_ip(headers: &HeaderMap) -> Option<std::net::IpAddr> {
    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

pub(crate) fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    url::form_urlencoded::parse(query?.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

async fn well_known(State(state): State<Arc<AppState>>, uri: Uri, headers: HeaderMap) -> Response {
    let (host, port) = request_host(&uri, &headers);
    let host = state.hostname.clone().unwrap_or(host);
    let port = state
        .port
        .clone()
        .or_else(|| port.map(|port| port.to_string()))
        .unwrap_or_else(|| "443".to_owned());
    Json(json!({ "m.server": format!("{host}:{port}") })).into_response()
}

async fn address_convert(
    State(state): State<Arc<AppState>>,
    uri: Uri,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.as_deref();
    let (Some(remote_type), Some(remote_id)) = (
        query_param(query, "remoteType").filter(|value| !value.is_empty()),
        query_param(query, "remoteId").filter(|value| !value.is_empty()),
    ) else {
        return matrix::invalid_param("invalid parameter");
    };
    let mxc_id = to_matrix_id(&remote_id, &format!("{remote_type}_"));
    let server_name = state.server_name(&uri, &headers);
    Json(json!({
        "remoteType": remote_type,
        "remoteId": remote_id,
        "mxcUrl": format!("mxc://{server_name}/{mxc_id}"),
        "mxcId": mxc_id,
    }))
    .into_response()
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/_matrix/federation/v1/version", get(matrix::version))
        .route(
            "/_matrix/federation/v1/media/download/{id}",
            get(media::download),
        )
        .route("/_soliditas/preview_url", get(preview::preview_url))
        .route("/_soliditas/tunnel_ticket", post(ticket::tunnel_ticket))
        .route("/_soliditas/tunnel", get(tunnel::tunnel))
        .route("/_soliditas/embed/youtube", get(youtube::embed))
        .route("/_soliditas/embed/youtube/details", get(youtube::details))
        .route("/_soliditas/embed/youtube/dearrow", get(youtube::dearrow))
        .route(
            "/_soliditas/embed/youtube/thumbnail",
            get(youtube::thumbnail),
        )
        .route("/_soliditas/embed/tiktok", get(tiktok::lookup))
        .route("/_soliditas/gifs/search", get(gifs::search))
        .route("/_soliditas/gifs/media", get(gifs::media))
        .route("/_soliditas/adressconvert", get(address_convert))
        .route("/.well-known/matrix/server", get(well_known))
        .route("/livez", get(|| async { "ok" }))
        .fallback(matrix::not_implemented)
        .with_state(state)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

async fn shutdown() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing the SIGTERM handler failed");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let state = Arc::new(AppState {
        server_name: env("SOLIDITAS_SERVER_NAME"),
        hostname: env("SOLIDITAS_DELEGATE_HOST"),
        port: env("SOLIDITAS_DELEGATE_PORT"),
        previews: Previews::new(env("KLIPY_API_KEY")).map_err(std::io::Error::other)?,
        providers: Providers::new().map_err(std::io::Error::other)?,
        tickets: Tickets::new(env("SOLIDITAS_TICKET_KEY").as_deref())
            .map_err(std::io::Error::other)?,
        tunnels: Tunnels::new(),
        metrics: Metrics::default(),
    });
    let cleanup = Arc::clone(&state);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            every.tick().await;
            cleanup.previews.forget_idle_clients();
            cleanup.providers.forget_idle_clients();
            cleanup.tickets.forget_idle_clients();
            cleanup.tunnels.forget_idle_clients();
        }
    });
    let metrics_listen = env("SOLIDITAS_METRICS_LISTEN").unwrap_or_else(|| "[::]:9090".to_owned());
    let metrics_listener = tokio::net::TcpListener::bind(&metrics_listen).await?;
    let metrics = Router::new()
        .route("/metrics", get(metrics::metrics))
        .with_state(Arc::clone(&state));
    tokio::spawn(async move { axum::serve(metrics_listener, metrics).await });
    let listen = env("SOLIDITAS_LISTEN").unwrap_or_else(|| "[::]:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await
}

#[cfg(test)]
mod tests;
