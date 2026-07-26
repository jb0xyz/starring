pub mod generator;
pub mod id;
pub mod model;
pub mod store;
pub mod version;

pub use generator::{InstanceIdGenerationError, InstanceIdGenerator, SequenceInstanceIdGenerator};
pub use id::{InstanceId, InstanceIdError};
pub use model::{
    AutomationInstance, InstanceKind, InstanceMessageRef, InstanceResources, InstanceStatus,
};
pub use store::{
    InMemoryInstanceStore, InstanceRegistrarV1, InstanceRouteReaderV1, InstanceStore,
    InstanceStoreError, LegacyInstanceStoreCapabilitiesV1,
};
pub use version::{InstanceRuleSetVersion, InstanceRuleSetVersionError};
