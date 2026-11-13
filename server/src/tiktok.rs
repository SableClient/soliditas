use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;
use url::Url;

use crate::provider::{self, Kind};
use crate::{AppState, client_ip, query_param};

fn is_code(code: &str) -> bool {
    (4..=32).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// The link without its query, which only carries tracking.
pub(crate) fn short_link(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let segments: Vec<&str> = url
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect();
    let code = match (host, segments.as_slice()) {
        ("vm.tiktok.com" | "vt.tiktok.com", [code]) => code,
        ("tiktok.com" | "www.tiktok.com" | "m.tiktok.com", ["t", code]) => code,
        _ => return None,
    };
    is_code(code).then(|| format!("https://{host}/{}", segments.join("/")))
}

pub(crate) async fn lookup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(link) = query_param(query.as_deref(), "url").and_then(|raw| short_link(&raw)) else {
        return provider::invalid(&state.metrics, "url must be a TikTok short link");
    };
    let mut url = Url::parse("https://www.tiktok.com/oembed").expect("a constant url");
    url.query_pairs_mut().append_pair("url", &link);
    let found = state
        .providers
        .get(&url, Kind::Json, client_ip(&headers), &state.metrics)
        .await;
    provider::answer(found, &state.metrics)
}
