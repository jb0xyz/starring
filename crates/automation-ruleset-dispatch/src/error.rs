use automation_core::AdapterError;
use automation_instance::{InstanceStatus, InstanceStoreError};
use automation_ruleset::RuleSetStoreError;
use automation_ruleset_readiness::{ReadinessContextError, ReadinessError};

use crate::snapshot::SnapshotError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchError {
    DeferFailed(AdapterError),
    InstanceLookup(InstanceStoreError),
    InstanceNotFound,
    InstanceInactive(InstanceStatus),
    PinnedKeyInvalid,
    VersionLookup(RuleSetStoreError),
    PinnedVersionMissing,
    SnapshotFailed(SnapshotError),
    ContextInvalid(ReadinessContextError),
    NotReady(ReadinessError),
    NoMatchingRule { action: String },
    Execution(AdapterError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureResponseOutcome {
    NotAttempted,
    Sent,
    Failed(AdapterError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchFailure {
    pub cause: DispatchError,
    pub failure_response: FailureResponseOutcome,
}
