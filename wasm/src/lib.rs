use js_sys::{Function, Promise, Uint8Array};
use sable_link_preview::{Fetch, Response, parse_public_url, scrape};
use url::Url;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, RequestInit, RequestRedirect};

const USER_AGENT: &str =
    "Soliditas (bot; +https://github.com/SableClient/soliditas) facebookexternalhit/1.1";

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = fetch)]
    fn global_fetch(input: &str, init: &RequestInit) -> Promise;
}

struct WorkerFetch;

struct WorkerResponse {
    requested: Url,
    response: web_sys::Response,
}

fn cancel(response: &web_sys::Response) {
    if !response.body_used()
        && let Some(body) = response.body()
    {
        let _ = body.cancel();
    }
}

impl Fetch for WorkerFetch {
    type Response = WorkerResponse;

    async fn get(&self, url: &Url, accept: &'static str) -> Option<WorkerResponse> {
        let headers = Headers::new().ok()?;
        headers.set("User-Agent", USER_AGENT).ok()?;
        headers.set("Accept", accept).ok()?;
        let init = RequestInit::new();
        init.set_method("GET");
        init.set_headers(&headers);
        init.set_redirect(RequestRedirect::Follow);
        let response: web_sys::Response = JsFuture::from(global_fetch(url.as_str(), &init))
            .await
            .ok()?
            .dyn_into()
            .ok()?;
        if !response.ok() {
            cancel(&response);
            return None;
        }
        Some(WorkerResponse {
            requested: url.clone(),
            response,
        })
    }
}

impl Response for WorkerResponse {
    fn url(&self) -> Url {
        Url::parse(&self.response.url()).unwrap_or_else(|_| self.requested.clone())
    }

    fn header(&self, name: &str) -> Option<String> {
        self.response.headers().get(name).ok().flatten()
    }

    async fn body(self, limit: usize) -> Option<Vec<u8>> {
        let buffer = JsFuture::from(self.response.array_buffer().ok()?).await.ok()?;
        let bytes = Uint8Array::new(&buffer);
        let length = bytes.length().min(u32::try_from(limit).unwrap_or(u32::MAX));
        Some(bytes.subarray(0, length).to_vec())
    }
}

impl Drop for WorkerResponse {
    fn drop(&mut self) {
        cancel(&self.response);
    }
}

#[wasm_bindgen(js_name = parsePublicUrl)]
#[must_use]
pub fn parse_public(value: &str) -> Option<String> {
    parse_public_url(value, None).map(String::from)
}

#[wasm_bindgen(js_name = scrapePreview)]
pub async fn scrape_preview(target: String, to_mxc: Function) -> Option<String> {
    let url = parse_public_url(&target, None)?;
    let preview = scrape(&WorkerFetch, &url).await?;
    let media = |url: &Url| {
        to_mxc
            .call1(&JsValue::NULL, &JsValue::from_str(url.as_str()))
            .ok()
            .and_then(|uri| uri.as_string())
            .unwrap_or_default()
    };
    serde_json::to_string(&preview.to_matrix(media)).ok()
}
