use axum::http::StatusCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacadeErrorCode {
    AuthenticationRequired,
    Forbidden,
    NotFound,
    StaleGeneration,
    StalePayload,
    IdempotencyConflict,
    InvalidState,
    Superseded,
    InvalidServerCandidate,
    UpstreamInvalidResponse,
    DependencyUnavailable,
    DependencyTimeout,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FacadeError {
    code: FacadeErrorCode,
}

impl FacadeError {
    pub fn new(code: FacadeErrorCode) -> Self {
        Self { code }
    }

    pub fn error_code(self) -> FacadeErrorCode {
        self.code
    }

    pub fn retryable(self) -> bool {
        matches!(
            self.code,
            FacadeErrorCode::DependencyUnavailable | FacadeErrorCode::DependencyTimeout
        )
    }

    pub(crate) fn status(self) -> StatusCode {
        match self.code {
            FacadeErrorCode::AuthenticationRequired => StatusCode::UNAUTHORIZED,
            FacadeErrorCode::Forbidden => StatusCode::FORBIDDEN,
            FacadeErrorCode::NotFound => StatusCode::NOT_FOUND,
            FacadeErrorCode::StaleGeneration
            | FacadeErrorCode::StalePayload
            | FacadeErrorCode::IdempotencyConflict
            | FacadeErrorCode::InvalidState
            | FacadeErrorCode::Superseded => StatusCode::CONFLICT,
            FacadeErrorCode::InvalidServerCandidate => StatusCode::UNPROCESSABLE_ENTITY,
            FacadeErrorCode::UpstreamInvalidResponse => StatusCode::BAD_GATEWAY,
            FacadeErrorCode::DependencyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            FacadeErrorCode::DependencyTimeout => StatusCode::GATEWAY_TIMEOUT,
            FacadeErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn code(self) -> &'static str {
        match self.code {
            FacadeErrorCode::AuthenticationRequired => "authentication_required",
            FacadeErrorCode::Forbidden => "forbidden",
            FacadeErrorCode::NotFound => "not_found",
            FacadeErrorCode::StaleGeneration => "stale_generation",
            FacadeErrorCode::StalePayload => "stale_payload",
            FacadeErrorCode::IdempotencyConflict => "idempotency_conflict",
            FacadeErrorCode::InvalidState => "invalid_state",
            FacadeErrorCode::Superseded => "superseded",
            FacadeErrorCode::InvalidServerCandidate => "invalid_server_candidate",
            FacadeErrorCode::UpstreamInvalidResponse => "upstream_invalid_response",
            FacadeErrorCode::DependencyUnavailable => "dependency_unavailable",
            FacadeErrorCode::DependencyTimeout => "dependency_timeout",
            FacadeErrorCode::Internal => "internal_error",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self.code {
            FacadeErrorCode::AuthenticationRequired => "A valid product session is required.",
            FacadeErrorCode::Forbidden => "The operation is not allowed.",
            FacadeErrorCode::NotFound => "The requested resource was not found.",
            FacadeErrorCode::StaleGeneration => {
                "The authoring session changed. Reload and try again."
            }
            FacadeErrorCode::StalePayload => {
                "The approval payload changed. Reload and review it again."
            }
            FacadeErrorCode::IdempotencyConflict => {
                "The idempotency key was already used for a different request."
            }
            FacadeErrorCode::InvalidState => "The operation is not valid in the current state.",
            FacadeErrorCode::Superseded => "The requested target has been superseded.",
            FacadeErrorCode::InvalidServerCandidate => "The server-owned candidate is not valid.",
            FacadeErrorCode::UpstreamInvalidResponse => {
                "An upstream service returned an invalid response."
            }
            FacadeErrorCode::DependencyUnavailable => "A required dependency is unavailable.",
            FacadeErrorCode::DependencyTimeout => "A required dependency timed out.",
            FacadeErrorCode::Internal => "The request could not be completed.",
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::{FacadeError, FacadeErrorCode};

    #[test]
    fn invalid_state_is_a_non_retryable_conflict_wire_error() {
        let error = FacadeError::new(FacadeErrorCode::InvalidState);
        assert_eq!(error.status(), StatusCode::CONFLICT);
        assert_eq!(error.code(), "invalid_state");
        assert!(!error.retryable());
    }
}
