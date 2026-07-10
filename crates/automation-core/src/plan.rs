use automation_state::ModalFieldSpec;
use discord_model::{RoleId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPlan {
    pub steps: Vec<PlannedAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalPresentation {
    pub key: String,
    pub title: String,
    pub fields: Vec<ModalFieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedAction {
    GrantRole { role: RoleId, target: UserId },
    RespondEphemeral { content: String },
    OpenModal(ModalPresentation),
}
