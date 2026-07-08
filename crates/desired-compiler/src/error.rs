use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CompileError {
    #[error("permission conflict in channel {channel} for target {target}")]
    PermissionConflict { channel: String, target: String },
}
