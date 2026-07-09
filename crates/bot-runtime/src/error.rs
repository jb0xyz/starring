use executor_core::{AdapterError, AdapterErrorKind};
use twilight_http::error::ErrorType;

pub fn classify_status(status: u16) -> AdapterErrorKind {
    match status {
        429 => AdapterErrorKind::RateLimited,
        408 => AdapterErrorKind::Timeout,
        400 => AdapterErrorKind::BadRequest,
        401 | 403 => AdapterErrorKind::Forbidden,
        404 => AdapterErrorKind::NotFound,
        500..=599 => AdapterErrorKind::ServerError,
        _ => AdapterErrorKind::Unknown,
    }
}

pub fn classify_error(err: &twilight_http::Error) -> AdapterError {
    let kind = match err.kind() {
        ErrorType::Response { status, .. } => classify_status(status.get()),
        ErrorType::RequestTimedOut => AdapterErrorKind::Timeout,
        ErrorType::RequestError => AdapterErrorKind::Network,
        ErrorType::Unauthorized => AdapterErrorKind::Forbidden,
        _ => AdapterErrorKind::Unknown,
    };
    AdapterError::new(kind, format!("twilight error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use executor_core::{AdapterError, AdapterErrorKind};

    #[test]
    fn status_classification() {
        assert_eq!(classify_status(429), AdapterErrorKind::RateLimited);
        assert_eq!(classify_status(408), AdapterErrorKind::Timeout);
        assert_eq!(classify_status(400), AdapterErrorKind::BadRequest);
        assert_eq!(classify_status(401), AdapterErrorKind::Forbidden);
        assert_eq!(classify_status(403), AdapterErrorKind::Forbidden);
        assert_eq!(classify_status(404), AdapterErrorKind::NotFound);
        assert_eq!(classify_status(503), AdapterErrorKind::ServerError);
        assert_eq!(classify_status(200), AdapterErrorKind::Unknown);
    }

    #[test]
    fn retryable_reflects_status() {
        assert!(AdapterError::new(classify_status(429), "").is_retryable());
        assert!(!AdapterError::new(classify_status(403), "").is_retryable());
    }
}
