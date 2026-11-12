use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

pub(crate) fn error(status: StatusCode, errcode: &str, error: &str) -> Response {
    (status, Json(json!({ "errcode": errcode, "error": error }))).into_response()
}

pub(crate) fn invalid_param(message: &str) -> Response {
    error(
        StatusCode::BAD_REQUEST,
        "M_INVALID_PARAM",
        &format!("You submitted an invalid parameter: \"{message}\""),
    )
}

pub(crate) fn not_found(message: &str) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "M_NOT_FOUND",
        &format!("The requested resource couldn't be found: \"{message}\""),
    )
}

pub(crate) async fn not_implemented() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "M_UNRECOGNIZED",
        "Endpoint not implemented: \"only implements media endpoints\"",
    )
}

pub(crate) async fn version() -> Json<Value> {
    Json(json!({ "server": { "name": "Soliditas", "version": env!("CARGO_PKG_VERSION") } }))
}
