use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use url::Url;

use crate::provider::{self, Kind};
use crate::{AppState, client_ip, query_param};

fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn is_start(start: &str) -> bool {
    !start.is_empty() && start.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) async fn embed(RawQuery(query): RawQuery) -> Response {
    let query = query.as_deref();
    let id = query_param(query, "v");
    let start = query_param(query, "start");
    let Some(id) = id.filter(|id| is_video_id(id)) else {
        return invalid();
    };
    if start.as_deref().is_some_and(|start| !is_start(start)) {
        return invalid();
    }

    let start = start.map_or_else(String::new, |start| format!("&amp;start={start}"));
    let src = format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1{start}");
    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>YouTube</title>
<style>html,body{{margin:0;height:100%}}iframe{{display:block;width:100%;height:100%;border:0}}</style></head>
<body><iframe src="{src}" title="YouTube" allow="autoplay; encrypted-media; picture-in-picture; fullscreen" allowfullscreen></iframe></body></html>"#
    );
    (
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (REFERRER_POLICY, "strict-origin-when-cross-origin"),
            (
                CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'; frame-src https://www.youtube-nocookie.com; base-uri 'none'; form-action 'none'",
            ),
            (CACHE_CONTROL, "public, max-age=86400"),
        ],
        html,
    )
        .into_response()
}

fn invalid() -> Response {
    (StatusCode::BAD_REQUEST, "Invalid video or start time").into_response()
}

fn video(query: Option<&str>) -> Option<String> {
    query_param(query, "v").filter(|id| is_video_id(id))
}

pub(crate) fn details_url(id: &str) -> Url {
    let mut url = Url::parse("https://www.youtube.com/oembed?format=json").expect("a constant url");
    url.query_pairs_mut()
        .append_pair("url", &format!("https://www.youtube.com/watch?v={id}"));
    url
}

pub(crate) fn dearrow_url(id: &str) -> Url {
    let mut url = Url::parse("https://sponsor.ajay.app/api/branding").expect("a constant url");
    url.query_pairs_mut()
        .append_pair("videoID", id)
        .append_pair("service", "YouTube")
        .append_pair("fetchAll", "false");
    url
}

pub(crate) fn frame_url(id: &str, frame: Option<&str>) -> Option<Url> {
    let file = match frame {
        None => "hqdefault",
        Some("start") => "hq1",
        Some("middle") => "hq2",
        Some("end") => "hq3",
        Some(_) => return None,
    };
    Url::parse(&format!("https://i.ytimg.com/vi/{id}/{file}.jpg")).ok()
}

pub(crate) fn dearrow_frame_url(id: &str, time: &str) -> Option<Url> {
    time.parse::<f64>()
        .ok()
        .filter(|time| time.is_finite() && *time >= 0.0)?;
    let mut url =
        Url::parse("https://dearrow-thumb.ajay.app/api/v1/getThumbnail").expect("a constant url");
    url.query_pairs_mut()
        .append_pair("videoID", id)
        .append_pair("time", time);
    Some(url)
}

pub(crate) async fn details(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    lookup(&state, &headers, query.as_deref(), details_url).await
}

pub(crate) async fn dearrow(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    lookup(&state, &headers, query.as_deref(), dearrow_url).await
}

async fn lookup(
    state: &AppState,
    headers: &HeaderMap,
    query: Option<&str>,
    url: fn(&str) -> Url,
) -> Response {
    let Some(id) = video(query) else {
        return provider::invalid(&state.metrics, "v must be a YouTube video id");
    };
    let found = state
        .providers
        .get(&url(&id), Kind::Json, client_ip(headers), &state.metrics)
        .await;
    provider::answer(found, &state.metrics)
}

/// With `time`, DeArrow renders that moment of the video, and the YouTube frame stands in when
/// it has not rendered it yet.
pub(crate) async fn thumbnail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.as_deref();
    let frame = query_param(query, "frame");
    let Some(id) = video(query) else {
        return provider::invalid(&state.metrics, "v must be a YouTube video id");
    };
    let Some(frame) = frame_url(&id, frame.as_deref()) else {
        return provider::invalid(&state.metrics, "frame must be start, middle or end");
    };
    let rendered = match query_param(query, "time") {
        Some(time) => match dearrow_frame_url(&id, &time) {
            Some(url) => Some(url),
            None => return provider::invalid(&state.metrics, "time must be a moment in seconds"),
        },
        None => None,
    };
    let client = client_ip(&headers);
    let providers = &state.providers;
    if let Some(rendered) = rendered
        && let Ok(found) = providers
            .get(&rendered, Kind::Image, client, &state.metrics)
            .await
    {
        return provider::answer(Ok(found), &state.metrics);
    }
    let found = providers
        .get(&frame, Kind::Image, client, &state.metrics)
        .await;
    provider::answer(found, &state.metrics)
}
