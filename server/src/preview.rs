use std::sync::Arc;
use std::time::Duration;

use axum::extract::{RawQuery, State};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Uri};
use axum::response::{IntoResponse, Response};
use moka::future::Cache;
use sable_link_preview::{ReqwestFetch, ScrapeOptions, parse_public_url, scrape};
use tokio::sync::Semaphore;
use url::Url;

use crate::mxc::to_matrix_id;
use crate::{AppState, matrix, query_param};

const USER_AGENT: &str =
    "Soliditas (bot; +https://github.com/SableClient/soliditas) facebookexternalhit/1.1";

const CACHE_BYTES: u64 = 256 * 1024 * 1024;

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

const MAX_CONCURRENT_SCRAPES: usize = 64;

pub(crate) struct Previews {
    fetch: ReqwestFetch,
    options: ScrapeOptions,
    cache: Cache<String, Arc<str>>,
    scrapes: Semaphore,
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
        })
    }

    async fn preview(&self, server_name: &str, url: &Url) -> Option<Arc<str>> {
        self.cache
            .optionally_get_with(format!("{server_name} {url}"), async {
                let _permit = self.scrapes.acquire().await.ok()?;
                let preview = scrape(&self.fetch, url, &self.options).await?;
                let media = |media: &Url| {
                    format!(
                        "mxc://{server_name}/{}",
                        to_matrix_id(media.as_str(), "og_")
                    )
                };
                serde_json::to_string(&preview.to_matrix(media))
                    .ok()
                    .map(Arc::from)
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
        return cors(matrix::invalid_param("url must be a public http(s) url"));
    };
    let server_name = state.server_name(&uri, &headers);
    match state.previews.preview(&server_name, &url).await {
        Some(preview) => (
            [
                (CONTENT_TYPE, "application/json"),
                (ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                (CACHE_CONTROL, "public, max-age=86400"),
            ],
            String::from(&*preview),
        )
            .into_response(),
        None => cors(matrix::not_found("no html preview for this url")),
    }
}

fn cors(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}
