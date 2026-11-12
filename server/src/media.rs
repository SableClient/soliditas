use std::hash::{BuildHasher as _, Hasher as _, RandomState};

use axum::extract::Path;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};

use crate::matrix;
use crate::mxc::from_matrix_id;

pub(crate) async fn download(Path(raw): Path<String>) -> Response {
    let Some(decoded) = from_matrix_id(&raw) else {
        return matrix::error(
            StatusCode::BAD_REQUEST,
            "M_INVALID_PARAM",
            "The ID given is not a valid MXC id",
        );
    };
    let is = |prefix: &str| raw.starts_with(prefix) || decoded.starts_with(prefix);
    let location = if is("tenor_") {
        format!("https://media.tenor.com/{decoded}/tenor.gif")
    } else if is("giphy_") {
        giphy(&decoded)
    } else if is("klipy_") {
        format!("https://static.klipy.com/ii/{decoded}")
    } else if raw.starts_with("og_") {
        match sable_link_preview::parse_public_url(&decoded, None) {
            Some(image) => image.into(),
            None => return matrix::invalid_param("the og image is not a public http(s) url"),
        }
    } else {
        return matrix::invalid_param(
            "the identifier of the remote didn't match any supported remote identifier",
        );
    };
    // The location is written into the multipart body, so a line break would add a part header.
    if location.chars().any(char::is_control) {
        return matrix::invalid_param("the identifier holds a control character");
    }
    redirect(&location)
}

fn giphy(id: &str) -> String {
    let has_extension = id.rsplit_once('.').is_some_and(|(_, extension)| {
        !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    });
    if has_extension {
        format!("https://i.giphy.com/{id}")
    } else {
        format!("https://i.giphy.com/{id}.gif")
    }
}

fn redirect(location: &str) -> Response {
    let boundary = format!("soliditas{:x}", RandomState::new().build_hasher().finish());
    let body = format!(
        "--{boundary}\r\nContent-Type: application/json\r\n\r\n{{}}\r\n\
         --{boundary}\r\nContent-Type: application/octet-stream\r\nLocation: {location}\r\n\r\n\r\n\
         --{boundary}--\r\n"
    );
    (
        [(
            CONTENT_TYPE,
            format!("multipart/mixed; boundary={boundary}"),
        )],
        body,
    )
        .into_response()
}
