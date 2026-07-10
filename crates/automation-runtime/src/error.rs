use automation_core::{AdapterError, AdapterErrorKind};
use twilight_http::error::ErrorType;

pub fn classify_error(err: &twilight_http::Error) -> AdapterError {
    let kind = match err.kind() {
        ErrorType::Response { status, .. } => match status.get() {
            429 => AdapterErrorKind::RateLimited,
            401 | 403 => AdapterErrorKind::Forbidden,
            404 => AdapterErrorKind::NotFound,
            _ => AdapterErrorKind::Unknown,
        },
        ErrorType::RequestTimedOut | ErrorType::RequestError => AdapterErrorKind::Network,
        ErrorType::Unauthorized => AdapterErrorKind::Forbidden,
        _ => AdapterErrorKind::Unknown,
    };
    AdapterError::new(kind, format!("twilight error: {err}"))
}

pub fn classify_body_error(err: &twilight_http::response::DeserializeBodyError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::Unknown,
        format!("twilight model error: {err}"),
    )
}
