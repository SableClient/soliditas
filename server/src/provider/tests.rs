use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::ServiceExt as _;
use url::Url;

use super::{Failure, Kind, Providers};
use crate::metrics::Metrics;
use crate::tests::state;
use crate::{gifs, router, tiktok, youtube};

async fn upstream(app: Router) -> Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    Url::parse(&format!("http://{address}/")).unwrap()
}

#[tokio::test]
async fn an_answer_is_fetched_once_and_then_served_from_memory() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let base = upstream(Router::new().route(
        "/details",
        get(move || {
            counted.fetch_add(1, Ordering::Relaxed);
            async { ([(CONTENT_TYPE, "text/plain")], r#"{"title":"A video"}"#) }
        }),
    ))
    .await;
    let providers = Providers::new().unwrap();
    let metrics = Metrics::default();
    let url = base.join("details").unwrap();
    for _ in 0..3 {
        let found = providers
            .get(&url, Kind::Json, None, &metrics)
            .await
            .ok()
            .unwrap();
        assert_eq!(found.status, StatusCode::OK);
        assert_eq!(found.content_type, "application/json");
        assert_eq!(&found.body[..], br#"{"title":"A video"}"#);
    }
    assert_eq!(hits.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn only_json_and_images_come_back() {
    let base = upstream(
        Router::new()
            .route("/html", get(|| async { "<script>alert(1)</script>" }))
            .route(
                "/page-as-image",
                get(|| async { ([(CONTENT_TYPE, "text/html")], "<script>alert(1)</script>") }),
            )
            .route(
                "/svg",
                get(|| async { ([(CONTENT_TYPE, "image/svg+xml")], "<svg/>") }),
            )
            .route(
                "/gif",
                get(|| async { ([(CONTENT_TYPE, "image/gif")], "GIF89a") }),
            )
            .route("/empty", get(|| async { StatusCode::NO_CONTENT }))
            .route(
                "/elsewhere",
                get(|| async { axum::response::Redirect::temporary("/gif") }),
            )
            .route(
                "/no-votes",
                get(|| async { (StatusCode::NOT_FOUND, r#"{"titles":[]}"#) }),
            ),
    )
    .await;
    let providers = Providers::new().unwrap();
    let metrics = Metrics::default();
    let get = async |path: &str, kind| {
        providers
            .get(&base.join(path).unwrap(), kind, None, &metrics)
            .await
    };
    assert!(get("html", Kind::Json).await.is_err());
    for path in ["page-as-image", "svg", "empty", "elsewhere"] {
        assert!(get(path, Kind::Image).await.is_err(), "{path}");
    }
    let gif = get("gif", Kind::Image).await.ok().unwrap();
    assert_eq!(gif.content_type, "image/gif");
    let unvoted = get("no-votes", Kind::Json).await.ok().unwrap();
    assert_eq!(unvoted.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_oversized_answer_is_dropped() {
    let base = upstream(Router::new().route(
        "/huge",
        get(|| async { ([(CONTENT_TYPE, "image/gif")], vec![0u8; 9 * 1024 * 1024]) }),
    ))
    .await;
    let providers = Providers::new().unwrap();
    let found = providers
        .get(
            &base.join("huge").unwrap(),
            Kind::Image,
            None,
            &Metrics::default(),
        )
        .await;
    assert!(found.is_err());
}

#[tokio::test]
async fn one_address_is_limited_without_slowing_another() {
    let base = upstream(Router::new().fallback(|| async { "{}" })).await;
    let providers = Providers::new().unwrap();
    let metrics = Metrics::default();
    let mut refused = None;
    for page in 0..400 {
        let url = base.join(&format!("page-{page}")).unwrap();
        let found = providers
            .get(
                &url,
                Kind::Json,
                Some("203.0.113.7".parse().unwrap()),
                &metrics,
            )
            .await;
        if found.is_err_and(|failure| matches!(*failure, Failure::Limited)) {
            refused = Some(page);
            break;
        }
    }
    assert!(refused.is_some_and(|page| page >= 300), "{refused:?}");
    let other = providers
        .get(
            &base.join("other").unwrap(),
            Kind::Json,
            Some("198.51.100.9".parse().unwrap()),
            &metrics,
        )
        .await;
    assert!(other.is_ok());
}

#[test]
fn provider_urls_are_built_from_checked_parts() {
    assert_eq!(
        youtube::details_url("MTn_bhTVr2U").as_str(),
        "https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3DMTn_bhTVr2U"
    );
    assert_eq!(
        youtube::dearrow_url("MTn_bhTVr2U").as_str(),
        "https://sponsor.ajay.app/api/branding?videoID=MTn_bhTVr2U&service=YouTube&fetchAll=false"
    );
    for (frame, file) in [
        (None, "hqdefault"),
        (Some("start"), "hq1"),
        (Some("middle"), "hq2"),
        (Some("end"), "hq3"),
    ] {
        assert_eq!(
            youtube::frame_url("MTn_bhTVr2U", frame).unwrap().as_str(),
            format!("https://i.ytimg.com/vi/MTn_bhTVr2U/{file}.jpg")
        );
    }
    assert!(youtube::frame_url("MTn_bhTVr2U", Some("../x")).is_none());
    assert_eq!(
        youtube::dearrow_frame_url("MTn_bhTVr2U", "13.37")
            .unwrap()
            .as_str(),
        "https://dearrow-thumb.ajay.app/api/v1/getThumbnail?videoID=MTn_bhTVr2U&time=13.37"
    );
    for time in ["", "-1", "NaN", "inf", "1&redirectUrl=https://evil.example"] {
        assert!(
            youtube::dearrow_frame_url("MTn_bhTVr2U", time).is_none(),
            "{time}"
        );
    }
}

#[test]
fn only_tiktok_short_links_are_looked_up() {
    for (link, cleaned) in [
        (
            "https://vm.tiktok.com/ZMabc123/",
            "https://vm.tiktok.com/ZMabc123",
        ),
        (
            "http://vt.tiktok.com/ZSabc123",
            "https://vt.tiktok.com/ZSabc123",
        ),
        (
            "https://www.tiktok.com/t/ZTabc123/?_t=tracking",
            "https://www.tiktok.com/t/ZTabc123",
        ),
    ] {
        assert_eq!(tiktok::short_link(link).as_deref(), Some(cleaned), "{link}");
    }
    for link in [
        "https://vm.tiktok.com/",
        "https://vm.tiktok.com/a/b",
        "https://www.tiktok.com/@someone/video/7234567890123456789",
        "https://vm.tiktok.com.evil.example/ZMabc123",
        "https://example.com/t/ZTabc123",
        "ftp://vm.tiktok.com/ZMabc123",
        "not a url",
    ] {
        assert!(tiktok::short_link(link).is_none(), "{link}");
    }
}

#[test]
fn gif_searches_go_to_the_named_provider() {
    assert_eq!(
        gifs::search_url("tenor", "abc_DEF-1", "happy cat")
            .unwrap()
            .as_str(),
        "https://tenor.googleapis.com/v2/search?key=abc_DEF-1&client_key=tenor_web&limit=50&media_filter=gif%2Cmediumgif%2Ctinygif%2Cnanogif&q=happy+cat"
    );
    assert_eq!(
        gifs::search_url("giphy", "abc", "a&b=c").unwrap().as_str(),
        "https://api.giphy.com/v1/gifs/search?api_key=abc&limit=50&q=a%26b%3Dc"
    );
    assert_eq!(
        gifs::search_url("klipy", "abc", "cat").unwrap().as_str(),
        "https://api.klipy.com/api/v1/abc/gifs/search?per_page=50&q=cat"
    );
    assert!(gifs::search_url("other", "abc", "cat").is_none());
    assert!(gifs::search_url("klipy", "../../x", "cat").is_none());
    assert!(gifs::search_url("tenor", "", "cat").is_none());
    assert!(gifs::search_url("tenor", "abc", "").is_none());
    assert!(gifs::search_url("tenor", "abc", &"a".repeat(201)).is_none());
}

#[test]
fn only_gif_hosts_are_fetched() {
    for url in [
        "https://media.tenor.com/lfDATg4Bhc0AAAAM/happy-cat.gif",
        "https://media1.tenor.com/m/lfDATg4Bhc0AAAAC/happy-cat.gif",
        "https://c.tenor.com/lfDATg4Bhc0AAAAC/tenor.gif",
        "https://i.giphy.com/media/JIX9t2j0ZTN9S/giphy.gif",
        "https://media2.giphy.com/media/v1.abc/JIX9t2j0ZTN9S/giphy.gif",
        "https://static.klipy.com/ii/f87f46a2/84/09/VBhPiYbU.gif",
    ] {
        assert!(gifs::media_url(url).is_some(), "{url}");
    }
    for url in [
        "http://media.tenor.com/a/b.gif",
        "https://media.tenor.com:8443/a/b.gif",
        "https://user@media.tenor.com/a/b.gif",
        "https://media.tenor.com.evil.example/a/b.gif",
        "https://mediax.tenor.com/a/b.gif",
        "https://static.klipy.com/other/b.gif",
        "https://tenor.com/view/a",
        "https://169.254.169.254/latest/meta-data",
        "https://example.com/a.gif",
    ] {
        assert!(gifs::media_url(url).is_none(), "{url}");
    }
}

#[tokio::test]
async fn bad_requests_are_refused_before_any_fetch() {
    let state = Arc::new(state());
    let app = router(Arc::clone(&state));
    for uri in [
        "/_soliditas/embed/youtube/details",
        "/_soliditas/embed/youtube/details?v=short",
        "/_soliditas/embed/youtube/dearrow?v=has%20space11",
        "/_soliditas/embed/youtube/thumbnail?v=MTn_bhTVr2U&frame=sideways",
        "/_soliditas/embed/youtube/thumbnail?v=MTn_bhTVr2U&time=soon",
        "/_soliditas/embed/tiktok?url=https%3A%2F%2Fexample.com%2Ft%2FZTabc123",
        "/_soliditas/gifs/search?provider=tenor&q=cat",
        "/_soliditas/gifs/search?provider=bing&key=abc&q=cat",
        "/_soliditas/gifs/media?url=https%3A%2F%2Fexample.com%2Fa.gif",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(
            body.starts_with(br#"{"errcode":"M_INVALID_PARAM""#),
            "{uri}"
        );
    }
    let text = crate::metrics::render(&state);
    for line in [
        "soliditas_provider_requests_total{result=\"invalid\"} 9",
        "soliditas_provider_fetches_total 0",
    ] {
        assert!(text.lines().any(|have| have == line), "{line}\n{text}");
    }
}
