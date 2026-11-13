use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use governor::{DefaultKeyedRateLimiter, Quota};
use moka::future::Cache;
use tokio::sync::Semaphore;
use url::Url;

use crate::matrix;
use crate::metrics::Metrics;

const USER_AGENT: &str = "Soliditas (+https://git.sable.moe/SableClient/soliditas)";

const CACHE_BYTES: u64 = 128 * 1024 * 1024;

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

const MAX_CONCURRENT_FETCHES: usize = 16;

/// One GIF search shows up to 50 thumbnails at once.
const FETCHES_PER_MINUTE: NonZeroU32 = NonZeroU32::new(600).unwrap();

const FETCH_BURST: NonZeroU32 = NonZeroU32::new(300).unwrap();

const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

const IMAGE_TYPES: [&str; 5] = [
    "image/gif",
    "image/webp",
    "image/jpeg",
    "image/png",
    "image/avif",
];

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Json,
    Image,
}

pub(crate) struct Fetched {
    status: StatusCode,
    content_type: HeaderValue,
    body: Bytes,
}

pub(crate) enum Failure {
    Missing,
    Limited,
}

pub(crate) type Outcome = Result<Arc<Fetched>, Arc<Failure>>;

pub(crate) struct Providers {
    client: reqwest::Client,
    cache: Cache<String, Arc<Fetched>>,
    fetches: Semaphore,
    per_ip: DefaultKeyedRateLimiter<IpAddr>,
}

impl Providers {
    pub(crate) fn new() -> reqwest::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            cache: Cache::builder()
                .max_capacity(CACHE_BYTES)
                .weigher(|key: &String, value: &Arc<Fetched>| {
                    u32::try_from(key.len() + value.body.len()).unwrap_or(u32::MAX)
                })
                .time_to_live(CACHE_TTL)
                .build(),
            fetches: Semaphore::new(MAX_CONCURRENT_FETCHES),
            per_ip: DefaultKeyedRateLimiter::dashmap(
                Quota::per_minute(FETCHES_PER_MINUTE).allow_burst(FETCH_BURST),
            ),
        })
    }

    pub(crate) fn cached(&self) -> u64 {
        self.cache.entry_count()
    }

    pub(crate) fn forget_idle_clients(&self) {
        self.per_ip.retain_recent();
        self.per_ip.shrink_to_fit();
    }

    /// A cached answer is free. Only the client whose request reaches the provider is charged.
    pub(crate) async fn get(
        &self,
        url: &Url,
        kind: Kind,
        client: Option<IpAddr>,
        metrics: &Metrics,
    ) -> Outcome {
        self.cache
            .try_get_with(url.to_string(), async {
                if let Some(client) = client
                    && self.per_ip.check_key(&client).is_err()
                {
                    return Err(Failure::Limited);
                }
                let _permit = self.fetches.acquire().await.map_err(|_| Failure::Missing)?;
                metrics.provider_fetches.inc();
                self.fetch(url, kind)
                    .await
                    .map(Arc::new)
                    .ok_or(Failure::Missing)
            })
            .await
    }

    async fn fetch(&self, url: &Url, kind: Kind) -> Option<Fetched> {
        let mut response = self.client.get(url.clone()).send().await.ok()?;
        let status = response.status();
        let content_type = response.headers().get(CONTENT_TYPE).cloned();
        let limit = match kind {
            Kind::Json => MAX_JSON_BYTES,
            Kind::Image => MAX_IMAGE_BYTES,
        };
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if body.len() + chunk.len() > limit {
                return None;
            }
            body.extend_from_slice(&chunk);
        }
        let content_type = match kind {
            // DeArrow answers 404 with a body the client still reads.
            Kind::Json => (matches!(status, StatusCode::OK | StatusCode::NOT_FOUND)
                && serde_json::from_slice::<serde_json::Value>(&body).is_ok())
            .then(|| HeaderValue::from_static("application/json"))?,
            Kind::Image => content_type.filter(|value| {
                status == StatusCode::OK
                    && value.to_str().is_ok_and(|value| {
                        IMAGE_TYPES.contains(&value.split(';').next().unwrap_or("").trim())
                    })
            })?,
        };
        Some(Fetched {
            status,
            content_type,
            body: body.into(),
        })
    }
}

pub(crate) fn answer(outcome: Outcome, metrics: &Metrics) -> Response {
    let response = match outcome {
        Ok(fetched) => {
            metrics.provider_served.inc();
            (
                fetched.status,
                [
                    (CONTENT_TYPE, fetched.content_type.clone()),
                    (
                        CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=3600"),
                    ),
                    (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
                ],
                fetched.body.clone(),
            )
                .into_response()
        }
        Err(failure) => match *failure {
            Failure::Missing => {
                metrics.provider_missing.inc();
                matrix::not_found("the provider has nothing for this")
            }
            Failure::Limited => {
                metrics.provider_limited.inc();
                matrix::error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "M_LIMIT_EXCEEDED",
                    "Too many lookups from this address",
                )
            }
        },
    };
    cors(response)
}

pub(crate) fn invalid(metrics: &Metrics, message: &str) -> Response {
    metrics.provider_invalid.inc();
    cors(matrix::invalid_param(message))
}

fn cors(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}

#[cfg(test)]
mod tests;
