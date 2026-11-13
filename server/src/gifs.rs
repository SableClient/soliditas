use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;
use url::Url;

use crate::provider::{self, Kind};
use crate::{AppState, client_ip, query_param};

const RESULTS: &str = "50";

const MAX_QUERY_BYTES: usize = 200;

fn is_key(key: &str) -> bool {
    (1..=128).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub(crate) fn search_url(provider: &str, key: &str, query: &str) -> Option<Url> {
    if !is_key(key) || query.is_empty() || query.len() > MAX_QUERY_BYTES {
        return None;
    }
    let mut url = match provider {
        "klipy" => Url::parse(&format!("https://api.klipy.com/api/v1/{key}/gifs/search")).ok()?,
        "tenor" => Url::parse("https://tenor.googleapis.com/v2/search").ok()?,
        "giphy" => Url::parse("https://api.giphy.com/v1/gifs/search").ok()?,
        _ => return None,
    };
    {
        let mut pairs = url.query_pairs_mut();
        match provider {
            "klipy" => pairs.append_pair("per_page", RESULTS),
            "tenor" => pairs
                .append_pair("key", key)
                // Any other client_key is a 403 worded as "Tenor API is discontinued".
                .append_pair("client_key", "tenor_web")
                .append_pair("limit", RESULTS)
                .append_pair("media_filter", "gif,mediumgif,tinygif,nanogif"),
            _ => pairs
                .append_pair("api_key", key)
                .append_pair("limit", RESULTS),
        };
        pairs.append_pair("q", query);
    }
    Some(url)
}

fn is_numbered(host: &str, prefix: &str, domain: &str) -> bool {
    host.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(domain))
        .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn media_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host = url.host_str()?;
    let known = (host == "static.klipy.com" && url.path().starts_with("/ii/"))
        || host == "c.tenor.com"
        || is_numbered(host, "media", ".tenor.com")
        || host == "i.giphy.com"
        || is_numbered(host, "media", ".giphy.com");
    known.then_some(url)
}

pub(crate) async fn search(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.as_deref();
    let param = |name| query_param(query, name).unwrap_or_default();
    let Some(url) = search_url(&param("provider"), &param("key"), param("q").trim()) else {
        return provider::invalid(&state.metrics, "expected provider, key and q");
    };
    let found = state
        .providers
        .get(&url, Kind::Json, client_ip(&headers), &state.metrics)
        .await;
    provider::answer(found, &state.metrics)
}

pub(crate) async fn media(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(url) = query_param(query.as_deref(), "url").and_then(|raw| media_url(&raw)) else {
        return provider::invalid(&state.metrics, "url must be a GIF from a known provider");
    };
    let found = state
        .providers
        .get(&url, Kind::Image, client_ip(&headers), &state.metrics)
        .await;
    provider::answer(found, &state.metrics)
}
