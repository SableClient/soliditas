use std::net::{IpAddr, Ipv6Addr};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer as _, SigningKey};
use governor::{DefaultKeyedRateLimiter, Quota};
use moka::Expiry;
use moka::future::Cache;
use reqwest::ClientBuilder;
use sable_link_preview::{Fetch, ReqwestFetch, Response as _, parse_public_url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::mxc::BASE64URL;
use crate::{AppState, matrix};

const AUDIENCE: &str = "sable-preview-tunnel";

const TICKET_TTL: Duration = Duration::from_secs(600);

const MAX_TOKEN_CACHE: Duration = Duration::from_secs(600);

const MAX_RESPONSE_BYTES: usize = 16 * 1024;

const TICKETS_PER_MINUTE: NonZeroU32 = NonZeroU32::new(10).unwrap();

const TICKET_BURST: NonZeroU32 = NonZeroU32::new(5).unwrap();

const FEDERATION_PORT: u16 = 8448;

struct Signing {
    key: SigningKey,
    kid: String,
}

struct VerifiedFor;

impl Expiry<[u8; 32], (String, Duration)> for VerifiedFor {
    fn expire_after_create(
        &self,
        _: &[u8; 32],
        value: &(String, Duration),
        _: std::time::Instant,
    ) -> Option<Duration> {
        Some(value.1)
    }
}

pub(crate) struct Tickets {
    fetch: ReqwestFetch,
    signing: Option<Signing>,
    verified: Cache<[u8; 32], (String, Duration)>,
    per_ip: DefaultKeyedRateLimiter<IpAddr>,
}

struct TicketRequest {
    access_token: String,
    matrix_server_name: String,
    expires_in: Option<u64>,
}

impl TicketRequest {
    fn parse(body: &Value) -> Option<Self> {
        Some(Self {
            access_token: body.get("access_token")?.as_str()?.to_owned(),
            matrix_server_name: body.get("matrix_server_name")?.as_str()?.to_owned(),
            expires_in: body.get("expires_in").and_then(Value::as_u64),
        })
    }
}

impl Tickets {
    pub(crate) fn new(seed: Option<&str>) -> Result<Self, String> {
        let signing = seed
            .map(|seed| {
                let seed: [u8; 32] = BASE64URL
                    .decode(seed)
                    .ok()
                    .and_then(|seed| seed.try_into().ok())
                    .ok_or("SOLIDITAS_TICKET_KEY must be a base64url 32-byte seed")?;
                let key = SigningKey::from_bytes(&seed);
                let digest = Sha256::digest(key.verifying_key().as_bytes());
                Ok::<_, String>(Signing {
                    kid: BASE64URL.encode(digest)[..16].to_owned(),
                    key,
                })
            })
            .transpose()?;
        let builder = ClientBuilder::new().timeout(Duration::from_secs(10));
        Ok(Self {
            fetch: ReqwestFetch::new(builder).map_err(|error| error.to_string())?,
            signing,
            verified: Cache::builder()
                .max_capacity(100_000)
                .expire_after(VerifiedFor)
                .build(),
            per_ip: DefaultKeyedRateLimiter::dashmap(
                Quota::per_minute(TICKETS_PER_MINUTE).allow_burst(TICKET_BURST),
            ),
        })
    }

    pub(crate) fn accepts(&self, ticket: &str) -> bool {
        self.signing
            .as_ref()
            .is_some_and(|key| is_valid(key, ticket, SystemTime::now()))
    }

    pub(crate) fn forget_idle_clients(&self) {
        self.per_ip.retain_recent();
        self.per_ip.shrink_to_fit();
    }

    async fn is_verified(&self, request: &TicketRequest) -> bool {
        let key: [u8; 32] = Sha256::digest(request.access_token.as_bytes()).into();
        let lifetime = request
            .expires_in
            .map_or(MAX_TOKEN_CACHE, Duration::from_secs)
            .min(MAX_TOKEN_CACHE);
        let verified = self
            .verified
            .try_get_with(key, async {
                let server = token_server(
                    &self.fetch,
                    &request.matrix_server_name,
                    &request.access_token,
                )
                .await
                .ok_or(())?;
                Ok::<_, ()>((server, lifetime))
            })
            .await;
        verified.is_ok_and(|(server, _)| server.eq_ignore_ascii_case(&request.matrix_server_name))
    }
}

fn issue(key: &Signing, now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let header = json!({ "alg": "EdDSA", "typ": "JWT", "kid": key.kid });
    let claims = json!({ "aud": AUDIENCE, "exp": seconds + TICKET_TTL.as_secs(), "iat": seconds });
    let signing_input = format!(
        "{}.{}",
        BASE64URL.encode(header.to_string()),
        BASE64URL.encode(claims.to_string())
    );
    let signature = key.key.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", BASE64URL.encode(signature.to_bytes()))
}

fn is_valid(key: &Signing, ticket: &str, now: SystemTime) -> bool {
    let mut parts = ticket.split('.');
    let (Some(header), Some(claims), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Some(signature) = BASE64URL
        .decode(signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
    else {
        return false;
    };
    let signed = format!("{header}.{claims}");
    if key
        .key
        .verifying_key()
        .verify_strict(signed.as_bytes(), &signature)
        .is_err()
    {
        return false;
    }
    let Some(claims) = BASE64URL
        .decode(claims)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    claims.get("aud").and_then(Value::as_str) == Some(AUDIENCE)
        && claims
            .get("exp")
            .and_then(Value::as_u64)
            .is_some_and(|expiry| expiry > seconds)
}

fn parse_server_name(name: &str) -> Option<(&str, Option<u16>)> {
    let (host, port) = if name.starts_with('[') {
        let end = name.find(']')?;
        name[1..end].parse::<Ipv6Addr>().ok()?;
        let port = match &name[end + 1..] {
            "" => None,
            rest => Some(rest.strip_prefix(':')?),
        };
        (&name[..=end], port)
    } else {
        let (host, port) = match name.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (name, None),
        };
        let hostname = !host.is_empty()
            && host.len() <= 255
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-');
        if !hostname {
            return None;
        }
        (host, port)
    };
    let port = match port {
        Some(port) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            Some(port.parse::<u16>().ok().filter(|port| *port != 0)?)
        }
        Some(_) => return None,
        None => None,
    };
    Some((host, port))
}

fn user_server(user_id: &str) -> Option<&str> {
    let (localpart, server) = user_id.strip_prefix('@')?.split_once(':')?;
    let valid =
        !localpart.is_empty() && user_id.len() <= 255 && !localpart.contains(char::is_control);
    (valid && parse_server_name(server).is_some()).then_some(server)
}

async fn get_json<F: Fetch>(fetch: &F, url: &Url) -> Option<Value> {
    let body = fetch
        .get(url, "application/json")
        .await?
        .body(MAX_RESPONSE_BYTES)
        .await?;
    serde_json::from_slice(&body).ok()
}

/// Matrix server discovery without SRV records. `None` for hosts that are not public domains.
async fn federation_base<F: Fetch>(fetch: &F, server_name: &str) -> Option<Url> {
    let (host, port) = parse_server_name(server_name)?;
    let authority = if port.is_some() {
        server_name.to_owned()
    } else {
        let well_known = Url::parse(&format!("https://{host}/.well-known/matrix/server")).ok()?;
        parse_public_url(well_known.as_str(), None)?;
        let delegated = get_json(fetch, &well_known)
            .await
            .and_then(|body| body.get("m.server")?.as_str().map(str::to_owned));
        match delegated {
            Some(delegated) => {
                let (delegated_host, delegated_port) = parse_server_name(&delegated)?;
                if delegated_port.is_some() {
                    delegated
                } else {
                    format!("{delegated_host}:{FEDERATION_PORT}")
                }
            }
            None => format!("{host}:{FEDERATION_PORT}"),
        }
    };
    parse_public_url(&format!("https://{authority}/"), None)
}

/// The server part of the user ID the homeserver at `server_name` vouches for.
async fn token_server<F: Fetch>(fetch: &F, server_name: &str, token: &str) -> Option<String> {
    let mut url = federation_base(fetch, server_name)
        .await?
        .join("_matrix/federation/v1/openid/userinfo")
        .ok()?;
    url.query_pairs_mut().append_pair("access_token", token);
    let body = get_json(fetch, &url).await?;
    user_server(body.get("sub")?.as_str()?).map(str::to_owned)
}

pub(crate) async fn tunnel_ticket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let tickets = &state.tickets;
    let Some(key) = &tickets.signing else {
        return unrecognized();
    };
    if let Some(client) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        && tickets.per_ip.check_key(&client).is_err()
    {
        state.metrics.tickets_limited.inc();
        return matrix::error(
            StatusCode::TOO_MANY_REQUESTS,
            "M_LIMIT_EXCEEDED",
            "Too many tickets requested from this address",
        );
    }
    let Some(request) = body.ok().and_then(|Json(body)| TicketRequest::parse(&body)) else {
        state.metrics.tickets_invalid.inc();
        return matrix::invalid_param("expected access_token and matrix_server_name");
    };
    if parse_server_name(&request.matrix_server_name).is_none() {
        state.metrics.tickets_invalid.inc();
        return matrix::invalid_param("matrix_server_name is not a Matrix server name");
    }
    if !tickets.is_verified(&request).await {
        state.metrics.tickets_rejected.inc();
        return matrix::error(
            StatusCode::UNAUTHORIZED,
            "M_UNAUTHORIZED",
            "The homeserver did not accept this access token",
        );
    }
    state.metrics.tickets_issued.inc();
    Json(json!({
        "ticket": issue(key, SystemTime::now()),
        "expires_in_ms": TICKET_TTL.as_millis(),
    }))
    .into_response()
}

fn unrecognized() -> Response {
    matrix::error(
        StatusCode::NOT_FOUND,
        "M_UNRECOGNIZED",
        "Endpoint not implemented: \"tunnel tickets are not enabled\"",
    )
}

#[cfg(test)]
mod tests;
