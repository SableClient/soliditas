use axum::extract::RawQuery;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY};
use axum::response::{IntoResponse, Response};

use crate::query_param;

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
