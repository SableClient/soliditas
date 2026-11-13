use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{RawQuery, State};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use governor::{DefaultKeyedRateLimiter, Quota};
use moka::future::Cache;
use sable_link_preview::{ReqwestFetch, ScrapeOptions, parse_public_url, scrape};
use tokio::sync::Semaphore;
use url::Url;

use crate::metrics::Metrics;
use crate::mxc::to_matrix_id;
use crate::{AppState, client_ip, matrix, query_param};

const USER_AGENT: &str =
    "Soliditas (bot; +https://github.com/SableClient/soliditas) facebookexternalhit/1.1";

const CACHE_BYTES: u64 = 256 * 1024 * 1024;

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

const MAX_CONCURRENT_SCRAPES: usize = 64;

const SCRAPES_PER_MINUTE: NonZeroU32 = NonZeroU32::new(120).unwrap();

const SCRAPE_BURST: NonZeroU32 = NonZeroU32::new(30).unwrap();

enum Failure {
    NoPreview,
    Limited,
}

pub(crate) struct Previews {
    fetch: ReqwestFetch,
    options: ScrapeOptions,
    cache: Cache<String, Arc<str>>,
    scrapes: Semaphore,
    per_ip: DefaultKeyedRateLimiter<IpAddr>,
}

impl Previews {
    pub(crate) fn new(klipy_api_key: Option<String>) -> reqwest::Result<Self> {
        let builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10));
        Ok(Self {
            fetch: ReqwestFetch::new(builder)?,
            options: ScrapeOptions { klipy_api_key },
            cache: Cache::builder()
                .max_capacity(CACHE_BYTES)
                .weigher(|key: &String, value: &Arc<str>| {
                    u32::try_from(key.len() + value.len()).unwrap_or(u32::MAX)
                })
                .time_to_live(CACHE_TTL)
                .build(),
            scrapes: Semaphore::new(MAX_CONCURRENT_SCRAPES),
            per_ip: DefaultKeyedRateLimiter::dashmap(
                Quota::per_minute(SCRAPES_PER_MINUTE).allow_burst(SCRAPE_BURST),
            ),
        })
    }

    pub(crate) fn scrapes_in_flight(&self) -> u64 {
        (MAX_CONCURRENT_SCRAPES - self.scrapes.available_permits()) as u64
    }

    pub(crate) fn cached(&self) -> u64 {
        self.cache.entry_count()
    }

    pub(crate) fn forget_idle_clients(&self) {
        self.per_ip.retain_recent();
        self.per_ip.shrink_to_fit();
    }

    /// A cached preview is free. Only the client whose request starts a scrape is charged for it.
    async fn preview(
        &self,
        server_name: &str,
        url: &Url,
        client: Option<IpAddr>,
        metrics: &Metrics,
    ) -> Result<Arc<str>, Arc<Failure>> {
        self.cache
            .try_get_with(format!("{server_name} {url}"), async {
                if let Some(client) = client
                    && self.per_ip.check_key(&client).is_err()
                {
                    return Err(Failure::Limited);
                }
                let _permit = self
                    .scrapes
                    .acquire()
                    .await
                    .map_err(|_| Failure::NoPreview)?;
                metrics.scrapes.inc();
                let preview = scrape(&self.fetch, url, &self.options)
                    .await
                    .ok_or(Failure::NoPreview)?;
                let media = |media: &Url| {
                    format!(
                        "mxc://{server_name}/{}",
                        to_matrix_id(media.as_str(), "og_")
                    )
                };
                serde_json::to_string(&preview.to_matrix(media))
                    .map(Arc::from)
                    .map_err(|_| Failure::NoPreview)
            })
            .await
    }
}

pub(crate) async fn preview_url(
    State(state): State<Arc<AppState>>,
    uri: Uri,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let target = query_param(query.as_deref(), "url");
    let Some(url) = target.and_then(|target| parse_public_url(&target, None)) else {
        state.metrics.previews_invalid.inc();
        return cors(matrix::invalid_param("url must be a public http(s) url"));
    };
    let server_name = state.server_name(&uri, &headers);
    let client = client_ip(&headers);
    let metrics = &state.metrics;
    match state
        .previews
        .preview(&server_name, &url, client, metrics)
        .await
    {
        Ok(preview) => {
            metrics.previews_served.inc();
            (
                [
                    (CONTENT_TYPE, "application/json"),
                    (ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                    (CACHE_CONTROL, "public, max-age=86400"),
                ],
                String::from(&*preview),
            )
                .into_response()
        }
        Err(failure) => match *failure {
            Failure::NoPreview => {
                metrics.previews_missing.inc();
                cors(matrix::not_found("no html preview for this url"))
            }
            Failure::Limited => {
                metrics.previews_limited.inc();
                cors(matrix::error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "M_LIMIT_EXCEEDED",
                    "Too many previews from this address",
                ))
            }
        },
    }
}

fn cors(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}
