#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDatabaseFailureV1 {
    #[error("product database request timed out")]
    Timeout,
    #[error("product database request can be retried")]
    Retryable,
    #[error("product database is unavailable")]
    Unavailable,
}

impl ProductDatabaseFailureV1 {
    pub(crate) fn classify(error: &sqlx::Error) -> Self {
        match error {
            sqlx::Error::Database(database) => match database.code().as_deref() {
                Some("57014" | "55P03") => Self::Timeout,
                Some("40001" | "40P01" | "53300" | "57P01" | "57P02" | "57P03") => Self::Retryable,
                Some(code) if code.starts_with("08") => Self::Retryable,
                _ => Self::Unavailable,
            },
            sqlx::Error::PoolTimedOut => Self::Timeout,
            sqlx::Error::Io(_) | sqlx::Error::Tls(_) => Self::Retryable,
            sqlx::Error::PoolClosed | sqlx::Error::WorkerCrashed => Self::Unavailable,
            _ => Self::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn retryable_transport_failures_are_bounded_and_redacted() {
        let error = sqlx::Error::Io(io::Error::new(io::ErrorKind::ConnectionReset, "secret"));
        let classified = ProductDatabaseFailureV1::classify(&error);
        assert_eq!(classified, ProductDatabaseFailureV1::Retryable);
        assert!(!classified.to_string().contains("secret"));
    }

    #[test]
    fn invariant_and_decode_failures_are_not_marked_retryable() {
        let classified = ProductDatabaseFailureV1::classify(&sqlx::Error::RowNotFound);
        assert_eq!(classified, ProductDatabaseFailureV1::Unavailable);
        let pool_timeout = ProductDatabaseFailureV1::classify(&sqlx::Error::PoolTimedOut);
        assert_eq!(pool_timeout, ProductDatabaseFailureV1::Timeout);
        let closed_pool = ProductDatabaseFailureV1::classify(&sqlx::Error::PoolClosed);
        assert_eq!(closed_pool, ProductDatabaseFailureV1::Unavailable);
    }
}
