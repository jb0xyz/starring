pub mod generator;
pub mod id;
pub mod model;
pub mod store;
pub mod version;

pub use generator::{
    InstanceIdGenerationError, InstanceIdGenerator, SecureRandomInstanceIdGenerator,
    SequenceInstanceIdGenerator,
};
pub use id::{InstanceId, InstanceIdError};
pub use model::{
    AutomationInstance, InstanceKind, InstanceMessageRef, InstanceResources, InstanceStatus,
};
pub use store::{
    InMemoryInstanceStore, InstanceRegistrarV1, InstanceRouteReaderV1, InstanceStore,
    InstanceStoreError, InstanceTeardownClaimOutcomeV1, InstanceTeardownMarkOutcomeV1,
    InstanceTeardownStoreV1, LegacyInstanceStoreCapabilitiesV1,
    MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1,
};
pub use version::{InstanceRuleSetVersion, InstanceRuleSetVersionError};
