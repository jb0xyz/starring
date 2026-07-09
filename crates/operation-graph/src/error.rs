use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum OperationGraphError {
    #[error("diff has {0} unresolved conflict(s)")]
    DiffHasConflicts(usize),
    #[error("missing payload for {key}")]
    MissingPayload { key: String },
    #[error("unsupported diff change")]
    UnsupportedChange,
    #[error("dependency cycle detected")]
    DependencyCycle,
}
