use serde::{Deserialize, Serialize};

use policy_engine::{Finding, Verdict};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewSeverity {
    Info,
    Notice,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewChangeKind {
    RoleCreate,
    RoleUpdate,
    RoleDelete,
    ChannelCreate,
    ChannelUpdate,
    ChannelDelete,
    OverwriteCreate,
    OverwriteUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewChange {
    pub kind: PreviewChangeKind,
    pub target: String,
    pub severity: PreviewSeverity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessChange {
    pub subject: String,
    pub channel: String,
    pub before_can_view: bool,
    pub after_can_view: bool,
    pub before_can_send: bool,
    pub after_can_send: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewModel {
    pub title: String,
    pub verdict: Verdict,
    pub approval_required: bool,
    pub blocked: bool,
    pub changes: Vec<PreviewChange>,
    pub access_changes: Vec<AccessChange>,
    pub policy_findings: Vec<Finding>,
    pub warnings: Vec<String>,
    pub deferred: Vec<String>,
}
