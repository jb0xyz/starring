use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ResolutionError {
    #[error("unresolved key: {key}")]
    UnresolvedKey { key: String },
    #[error("missing identity for key: {key}")]
    MissingIdentity { key: String },
}
