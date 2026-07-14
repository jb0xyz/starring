pub mod modal;
pub mod panel;
pub mod rule;

pub use automation_instance::InstanceKind;
pub use modal::{
    modal_input_utf16_len, ModalFieldSpec, ModalFieldStyle, ModalInputPolicy, ModalSpec,
    DISCORD_MODAL_INPUT_MAX_LENGTH,
};
pub use panel::{ButtonRoute, ButtonSpec, PanelSpec};
pub use rule::{
    ActionSpec, ActionTarget, ChannelRef, CreatedRef, InstanceRef, InstanceResourceRefs,
    InteractionRule, InteractionRuleSet, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
