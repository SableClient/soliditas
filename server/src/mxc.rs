use base64::Engine as _;
use base64::alphabet::URL_SAFE;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

const BASE64URL: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

pub(crate) fn to_matrix_id(value: &str, prefix: &str) -> String {
    format!("{prefix}{}", BASE64URL.encode(value))
}

/// Decodes what follows the first `_`, whatever the prefix.
pub(crate) fn from_matrix_id(id: &str) -> Option<String> {
    let (_, payload) = id.split_once('_')?;
    if payload.is_empty() {
        return None;
    }
    let bytes = BASE64URL.decode(payload).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
