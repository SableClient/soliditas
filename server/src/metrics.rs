use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;

use crate::AppState;

#[derive(Default)]
pub(crate) struct Counter(AtomicU64);

impl Counter {
    pub(crate) fn add(&self, amount: usize) {
        self.0.fetch_add(amount as u64, Ordering::Relaxed);
    }

    pub(crate) fn inc(&self) {
        self.add(1);
    }

    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
pub(crate) struct Metrics {
    pub(crate) previews_served: Counter,
    pub(crate) previews_missing: Counter,
    pub(crate) previews_limited: Counter,
    pub(crate) previews_invalid: Counter,
    pub(crate) scrapes: Counter,
    pub(crate) tickets_issued: Counter,
    pub(crate) tickets_rejected: Counter,
    pub(crate) tickets_limited: Counter,
    pub(crate) tickets_invalid: Counter,
    pub(crate) tunnels_opened: Counter,
    pub(crate) tunnels_closed: Counter,
    pub(crate) tunnels_unauthorized: Counter,
    pub(crate) tunnels_limited: Counter,
    pub(crate) tunnels_invalid: Counter,
    pub(crate) tunnels_unreachable: Counter,
    pub(crate) tunnel_bytes_up: Counter,
    pub(crate) tunnel_bytes_down: Counter,
}

fn family(out: &mut String, name: &str, kind: &str, label: &str, series: &[(&str, u64)]) {
    let _ = writeln!(out, "# TYPE {name} {kind}");
    for (value, count) in series {
        if label.is_empty() {
            let _ = writeln!(out, "{name} {count}");
        } else {
            let _ = writeln!(out, "{name}{{{label}=\"{value}\"}} {count}");
        }
    }
}

pub(crate) fn render(state: &AppState) -> String {
    let m = &state.metrics;
    let mut out = String::new();
    family(
        &mut out,
        "soliditas_preview_requests_total",
        "counter",
        "result",
        &[
            ("served", m.previews_served.get()),
            ("missing", m.previews_missing.get()),
            ("limited", m.previews_limited.get()),
            ("invalid", m.previews_invalid.get()),
        ],
    );
    family(
        &mut out,
        "soliditas_scrapes_total",
        "counter",
        "",
        &[("", m.scrapes.get())],
    );
    family(
        &mut out,
        "soliditas_scrapes_in_flight",
        "gauge",
        "",
        &[("", state.previews.scrapes_in_flight())],
    );
    family(
        &mut out,
        "soliditas_preview_cache_entries",
        "gauge",
        "",
        &[("", state.previews.cached())],
    );
    family(
        &mut out,
        "soliditas_ticket_requests_total",
        "counter",
        "result",
        &[
            ("issued", m.tickets_issued.get()),
            ("rejected", m.tickets_rejected.get()),
            ("limited", m.tickets_limited.get()),
            ("invalid", m.tickets_invalid.get()),
        ],
    );
    family(
        &mut out,
        "soliditas_tunnel_requests_total",
        "counter",
        "result",
        &[
            ("opened", m.tunnels_opened.get()),
            ("unauthorized", m.tunnels_unauthorized.get()),
            ("limited", m.tunnels_limited.get()),
            ("invalid", m.tunnels_invalid.get()),
            ("unreachable", m.tunnels_unreachable.get()),
        ],
    );
    family(
        &mut out,
        "soliditas_tunnels_open",
        "gauge",
        "",
        &[(
            "",
            m.tunnels_opened
                .get()
                .saturating_sub(m.tunnels_closed.get()),
        )],
    );
    family(
        &mut out,
        "soliditas_tunnel_bytes_total",
        "counter",
        "direction",
        &[
            ("up", m.tunnel_bytes_up.get()),
            ("down", m.tunnel_bytes_down.get()),
        ],
    );
    out
}

pub(crate) async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4")],
        render(&state),
    )
}
