use std::collections::HashMap;

use axum::body::{Body, to_bytes};
use axum::http::Request;
use tower::ServiceExt as _;

use super::*;
use crate::{router, tests::state};

struct Homeserver(HashMap<&'static str, Value>);

struct Reply(Value);

impl sable_link_preview::Response for Reply {
    fn url(&self) -> Url {
        Url::parse("https://reply.example/").unwrap()
    }

    fn header(&self, _: &str) -> Option<String> {
        None
    }

    async fn body(self, _: usize) -> Option<Vec<u8>> {
        Some(self.0.to_string().into_bytes())
    }
}

impl Fetch for Homeserver {
    type Response = Reply;

    async fn get(&self, url: &Url, _: &'static str) -> Option<Reply> {
        let without_query = &url.as_str()[..url.as_str().find('?').unwrap_or(url.as_str().len())];
        self.0.get(without_query).cloned().map(Reply)
    }
}

const SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

fn tickets() -> Tickets {
    Tickets::new(Some(SEED)).unwrap()
}

#[test]
fn a_ticket_is_accepted_until_it_expires_and_only_as_issued() {
    let tickets = tickets();
    let signing = tickets.signing.as_ref().unwrap();
    let now = SystemTime::now();
    let ticket = issue(signing, now);

    let [header, claims, signature] = ticket.split('.').collect::<Vec<_>>()[..] else {
        panic!("not a compact JWS")
    };
    let claims_json: Value = serde_json::from_slice(&BASE64URL.decode(claims).unwrap()).unwrap();
    let iat = claims_json["iat"].as_u64().unwrap();
    assert_eq!(
        claims_json,
        json!({ "aud": "sable-preview-tunnel", "exp": iat + 600, "iat": iat })
    );

    assert!(is_valid(signing, &ticket, now));
    assert!(is_valid(signing, &ticket, now + Duration::from_secs(599)));
    assert!(!is_valid(signing, &ticket, now + Duration::from_secs(600)));

    let other_audience = BASE64URL
        .encode(json!({ "aud": "something-else", "exp": iat + 600, "iat": iat }).to_string());
    assert!(!is_valid(
        signing,
        &format!("{header}.{other_audience}.{signature}"),
        now
    ));
    let forged = Tickets::new(Some("AQECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8")).unwrap();
    assert!(!is_valid(
        signing,
        &issue(forged.signing.as_ref().unwrap(), now),
        now
    ));
    for malformed in ["", "a.b", "a.b.c.d", &format!("{header}.{claims}.AAAA")] {
        assert!(!is_valid(signing, malformed, now), "{malformed}");
    }
    assert!(!Tickets::new(None).unwrap().accepts(&ticket));
}

#[test]
fn server_names_are_hostnames_or_ips_with_an_optional_port() {
    for valid in [
        "example.org",
        "example.org:8448",
        "1.2.3.4",
        "1.2.3.4:80",
        "[2001:db8::1]",
        "[2001:db8::1]:8448",
    ] {
        assert!(parse_server_name(valid).is_some(), "{valid}");
    }
    for invalid in [
        "",
        "example.org/path",
        "example.org:",
        "example.org:0",
        "example.org:65536",
        "example.org:80:80",
        "user@example.org",
        "exa mple.org",
        "[example.org]",
        "[2001:db8::1]8448",
        "https://example.org",
    ] {
        assert!(parse_server_name(invalid).is_none(), "{invalid}");
    }
}

#[tokio::test]
async fn delegation_and_default_port_decide_where_userinfo_is_asked() {
    let delegated = Homeserver(HashMap::from([(
        "https://example.org/.well-known/matrix/server",
        json!({ "m.server": "matrix.example.org:443" }),
    )]));
    assert_eq!(
        federation_base(&delegated, "example.org")
            .await
            .unwrap()
            .as_str(),
        "https://matrix.example.org/"
    );
    let bare = Homeserver(HashMap::from([(
        "https://example.org/.well-known/matrix/server",
        json!({ "m.server": "matrix.example.org" }),
    )]));
    assert_eq!(
        federation_base(&bare, "example.org")
            .await
            .unwrap()
            .as_str(),
        "https://matrix.example.org:8448/"
    );
    let none = Homeserver(HashMap::new());
    assert_eq!(
        federation_base(&none, "example.org")
            .await
            .unwrap()
            .as_str(),
        "https://example.org:8448/"
    );
    assert_eq!(
        federation_base(&none, "example.org:9000")
            .await
            .unwrap()
            .as_str(),
        "https://example.org:9000/"
    );
    assert!(federation_base(&none, "localhost").await.is_none());
    assert!(federation_base(&none, "10.0.0.1:8448").await.is_none());
}

#[tokio::test]
async fn userinfo_names_the_server_that_vouches_for_the_token() {
    let homeserver = Homeserver(HashMap::from([(
        "https://example.org:8448/_matrix/federation/v1/openid/userinfo",
        json!({ "sub": "@alice:example.org" }),
    )]));
    assert_eq!(
        token_server(&homeserver, "example.org", "token")
            .await
            .as_deref(),
        Some("example.org")
    );

    let liar = Homeserver(HashMap::from([(
        "https://evil.example:8448/_matrix/federation/v1/openid/userinfo",
        json!({ "sub": "@alice:example.org" }),
    )]));
    let server = token_server(&liar, "evil.example", "token").await.unwrap();
    assert_ne!(server, "evil.example");

    let malformed = Homeserver(HashMap::from([(
        "https://example.org:8448/_matrix/federation/v1/openid/userinfo",
        json!({ "sub": "alice" }),
    )]));
    assert_eq!(token_server(&malformed, "example.org", "token").await, None);
}

async fn post_ticket(state: AppState, body: &str) -> (StatusCode, Value) {
    let response = router(Arc::new(state))
        .oneshot(
            Request::post("/_soliditas/tunnel_ticket")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn tickets_are_off_without_a_key() {
    let (status, body) = post_ticket(state(), "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["errcode"], "M_UNRECOGNIZED");
}

#[tokio::test]
async fn bad_requests_are_refused_before_any_lookup() {
    let with_key = || AppState {
        tickets: tickets(),
        ..state()
    };
    for body in [
        r#"{"access_token":"t","matrix_server_name":"example.org/path"}"#,
        r#"{"access_token":"t","matrix_server_name":""}"#,
        r#"{"access_token":"t"}"#,
        "not json",
    ] {
        let (status, answer) = post_ticket(with_key(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(answer["errcode"], "M_INVALID_PARAM");
    }
    let (status, answer) = post_ticket(
        with_key(),
        r#"{"access_token":"t","matrix_server_name":"localhost"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer["errcode"], "M_UNAUTHORIZED");
}

#[tokio::test]
async fn ticket_requests_are_limited_per_address() {
    let app = router(Arc::new(AppState {
        tickets: tickets(),
        ..state()
    }));
    let mut limited = false;
    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(
                Request::post("/_soliditas/tunnel_ticket")
                    .header("x-real-ip", "203.0.113.7")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        limited |= response.status() == StatusCode::TOO_MANY_REQUESTS;
    }
    assert!(limited);
}
