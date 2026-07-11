pub mod modal;
pub mod panel;
pub mod rule;

pub use automation_instance::InstanceKind;
pub use modal::{ModalFieldSpec, ModalFieldStyle, ModalSpec};
pub use panel::{ButtonRoute, ButtonSpec, PanelSpec};
pub use rule::{
    ActionSpec, ActionTarget, ChannelRef, CreatedRef, InstanceRef, InstanceResourceRefs,
    InteractionRule, InteractionRuleSet, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
