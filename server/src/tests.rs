use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use crate::mxc::{from_matrix_id, to_matrix_id};
use crate::preview::Previews;
use crate::ticket::Tickets;
use crate::{AppState, router};

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|value| value.to_str().unwrap())
    }
}

async fn get_with(state: AppState, uri: &str) -> Answer {
    let response = router(Arc::new(state))
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    Answer {
        status,
        headers,
        body: String::from_utf8(body.to_vec()).unwrap(),
    }
}

pub(crate) fn state() -> AppState {
    AppState {
        server_name: None,
        hostname: None,
        port: None,
        previews: Previews::new(None).unwrap(),
        tickets: Tickets::new(None).unwrap(),
        tunnels: crate::tunnel::Tunnels::new(),
    }
}

async fn get(uri: &str) -> Answer {
    get_with(state(), uri).await
}

#[test]
fn matrix_ids_round_trip() {
    assert_eq!(to_matrix_id("example.txt", "mxc_"), "mxc_ZXhhbXBsZS50eHQ");
    assert_eq!(to_matrix_id("", "mxc_"), "mxc_");
    for name in [
        "photo.png",
        "📁-résumé.txt",
        "こんにちは.txt",
        "file/with+symbols",
    ] {
        let id = to_matrix_id(name, "mxc_");
        assert!(!id.contains(['+', '/', '=']), "{id}");
        assert_eq!(from_matrix_id(&id).as_deref(), Some(name));
    }
    let encoded = to_matrix_id("file.txt", "prefix_");
    let payload = encoded.split_once('_').unwrap().1;
    assert_eq!(
        from_matrix_id(&format!("custom_{payload}")).as_deref(),
        Some("file.txt")
    );
    assert_eq!(from_matrix_id("invalid"), None);
    assert_eq!(from_matrix_id("mxc_"), None);
}

#[tokio::test]
async fn well_known_delegates_to_the_address_it_was_reached_on() {
    let answer = get("https://gifs.example/.well-known/matrix/server").await;
    assert_eq!(answer.json(), json!({ "m.server": "gifs.example:443" }));
    assert_eq!(answer.header("content-type"), Some("application/json"));

    let answer = get("https://gifs.example:8443/.well-known/matrix/server").await;
    assert_eq!(answer.json(), json!({ "m.server": "gifs.example:8443" }));

    let pinned = AppState {
        hostname: Some("proxy.example".into()),
        port: Some("8448".into()),
        ..state()
    };
    let answer = get_with(pinned, "https://gifs.example/.well-known/matrix/server").await;
    assert_eq!(answer.json(), json!({ "m.server": "proxy.example:8448" }));
}

#[tokio::test]
async fn well_known_reads_the_host_header_behind_a_proxy() {
    let response = router(Arc::new(state()))
        .oneshot(
            Request::get("/.well-known/matrix/server")
                .header("host", "gifs.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body, json!({ "m.server": "gifs.example:443" }));
}

#[tokio::test]
async fn address_convert_builds_the_mxc_on_the_reached_host() {
    let answer =
        get("https://gifs.example/_soliditas/adressconvert?remoteType=tenor&remoteId=abc").await;
    assert_eq!(
        answer.json(),
        json!({
            "remoteType": "tenor",
            "remoteId": "abc",
            "mxcUrl": "mxc://gifs.example/tenor_YWJj",
            "mxcId": "tenor_YWJj",
        })
    );
    assert_eq!(
        get("https://gifs.example/_soliditas/adressconvert?remoteType=tenor")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
}

fn expected_redirect(answer: &Answer, location: &str) {
    assert_eq!(answer.status, StatusCode::OK);
    let content_type = answer.header("content-type").unwrap();
    let boundary = content_type
        .strip_prefix("multipart/mixed; boundary=")
        .unwrap();
    assert!(boundary.starts_with("soliditas"), "{boundary}");
    let expected = format!(
        "--{boundary}\r\nContent-Type: application/json\r\n\r\n{{}}\r\n\
         --{boundary}\r\nContent-Type: application/octet-stream\r\nLocation: {location}\r\n\r\n\r\n\
         --{boundary}--\r\n"
    );
    assert_eq!(answer.body, expected);
    assert_eq!(
        answer.header("content-length"),
        Some(expected.len().to_string().as_str())
    );
}

async fn media(id: &str) -> Answer {
    get(&format!(
        "https://gifs.example/_matrix/federation/v1/media/download/{id}"
    ))
    .await
}

#[tokio::test]
async fn media_redirects_to_giphy_with_the_id_extension_or_gif() {
    expected_redirect(
        &media("giphy_eVhQcXVBVENiOGtHaw").await,
        "https://i.giphy.com/yXPquATCb8kGk.gif",
    );
    expected_redirect(
        &media("giphy_eVhQcXVBVENiOGtHay53ZWJw").await,
        "https://i.giphy.com/yXPquATCb8kGk.webp",
    );
}

#[tokio::test]
async fn media_redirects_to_tenor_and_klipy() {
    expected_redirect(
        &media("tenor_YWJjMTIz").await,
        "https://media.tenor.com/abc123/tenor.gif",
    );
    let path = "ffd4ac143e6335ac68951b787d3c1902/e8/3a/5LM0jRpL.gif";
    expected_redirect(
        &media(&to_matrix_id(path, "klipy_")).await,
        &format!("https://static.klipy.com/ii/{path}"),
    );
}

#[tokio::test]
async fn media_redirects_to_a_public_og_image_only() {
    let image = "https://cdn.example/cover.png";
    expected_redirect(&media(&to_matrix_id(image, "og_")).await, image);
    let private = media(&to_matrix_id("http://127.0.0.1/x.png", "og_")).await;
    assert_eq!(private.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn media_refuses_bad_and_unknown_ids_and_header_injection() {
    let invalid = media("invalid").await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        invalid.json(),
        json!({ "errcode": "M_INVALID_PARAM", "error": "The ID given is not a valid MXC id" })
    );
    let unknown = media("unsupported_123").await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown.json()["errcode"], "M_INVALID_PARAM");
    let injected = media(&to_matrix_id("abc\r\nX-Evil: 1", "tenor_")).await;
    assert_eq!(injected.status, StatusCode::BAD_REQUEST);
}

async fn embed(query: &str) -> Answer {
    get(&format!(
        "https://gifs.example/_soliditas/embed/youtube?{query}"
    ))
    .await
}

#[tokio::test]
async fn youtube_embeds_the_video_with_its_start() {
    for (query, start) in [
        ("", ""),
        ("&start=0", "&amp;start=0"),
        ("&start=90", "&amp;start=90"),
    ] {
        let answer = embed(&format!("v=MTn_bhTVr2U{query}")).await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(
            answer.header("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert!(answer.body.contains(&format!(
            r#"src="https://www.youtube-nocookie.com/embed/MTn_bhTVr2U?autoplay=1{start}""#
        )));
        assert!(answer.body.contains(
            r#"allow="autoplay; encrypted-media; picture-in-picture; fullscreen" allowfullscreen"#
        ));
    }
    let answer = embed("v=MTn_bhTVr2U").await;
    assert_eq!(
        answer.header("referrer-policy"),
        Some("strict-origin-when-cross-origin")
    );
    assert_eq!(
        answer.header("content-security-policy"),
        Some(
            "default-src 'none'; style-src 'unsafe-inline'; frame-src https://www.youtube-nocookie.com; base-uri 'none'; form-action 'none'"
        )
    );
    assert_eq!(answer.header("x-frame-options"), None);
    assert_eq!(
        answer.header("cache-control"),
        Some("public, max-age=86400")
    );
    assert_eq!(embed("v=aB09_-aB09_").await.status, StatusCode::OK);
}

#[tokio::test]
async fn youtube_rejects_bad_ids_and_starts() {
    for query in [
        "",
        "v=",
        "v=short",
        "v=MTn_bhTVr2Ux",
        "v=MTn_bhTVr2%2F",
        "v=%22%3E%3Cscript%3E",
        "v=MTn_bhTVr2U%0A",
        "v=MTn_bhTVr2U&start=",
        "v=MTn_bhTVr2U&start=-1",
        "v=MTn_bhTVr2U&start=1.5",
        "v=MTn_bhTVr2U&start=1e2",
        "v=MTn_bhTVr2U&start=90s",
        "v=MTn_bhTVr2U&start=%201",
        "v=MTn_bhTVr2U&start=1%0A",
    ] {
        assert_eq!(
            embed(query).await.status,
            StatusCode::BAD_REQUEST,
            "{query}"
        );
    }
}

#[tokio::test]
async fn preview_refuses_urls_that_are_not_public_http() {
    for target in [
        "ftp://files.example/a",
        "http://localhost/",
        "http://10.0.0.1/",
        "http://[::1]/",
        "not a url",
    ] {
        let query: String = url::form_urlencoded::byte_serialize(target.as_bytes()).collect();
        let answer = get(&format!(
            "https://gifs.example/_soliditas/preview_url?url={query}"
        ))
        .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{target}");
        assert_eq!(answer.header("access-control-allow-origin"), Some("*"));
    }
    let answer = get("https://gifs.example/_soliditas/preview_url").await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn version_and_unknown_routes_answer_like_a_matrix_server() {
    let version = get("https://gifs.example/_matrix/federation/v1/version").await;
    assert_eq!(version.json()["server"]["name"], "Soliditas");
    let unknown = get("https://gifs.example/_matrix/client/v3/sync").await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_eq!(unknown.json()["errcode"], "M_UNRECOGNIZED");
}

async fn preview_from(app: &axum::Router, ip: &str, target: &str) -> StatusCode {
    let query: String = url::form_urlencoded::byte_serialize(target.as_bytes()).collect();
    app.clone()
        .oneshot(
            Request::get(format!("/_soliditas/preview_url?url={query}"))
                .header("host", "gifs.example")
                .header("x-real-ip", ip)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn one_address_is_limited_without_slowing_another() {
    let app = router(Arc::new(state()));
    let mut refused = None;
    for page in 0..40 {
        let target = format!("https://nonexistent-{page}.invalid/");
        if preview_from(&app, "203.0.113.7", &target).await == StatusCode::TOO_MANY_REQUESTS {
            refused = Some(page);
            break;
        }
    }
    assert!(refused.is_some_and(|page| page >= 30), "{refused:?}");
    assert_ne!(
        preview_from(&app, "198.51.100.9", "https://nonexistent-other.invalid/").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}
