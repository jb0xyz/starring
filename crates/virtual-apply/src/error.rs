use resource_resolution::ResolutionError;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum VirtualApplyError {
    #[error("unresolved key: {key}")]
    UnresolvedKey { key: String },
    #[error("missing identity for key: {key}")]
    MissingIdentity { key: String },
    #[error("operation graph cycle")]
    GraphCycle,
}

impl From<ResolutionError> for VirtualApplyError {
    fn from(err: ResolutionError) -> Self {
        match err {
            ResolutionError::UnresolvedKey { key } => VirtualApplyError::UnresolvedKey { key },
            ResolutionError::MissingIdentity { key } => VirtualApplyError::MissingIdentity { key },
        }
    }
}
