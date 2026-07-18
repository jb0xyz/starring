use std::fmt::{Debug, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductState {
    PendingApproval,
    Approved,
    Applying,
    RuntimePending,
    Live,
    Rejected,
    Expired,
    Superseded,
    Withdrawn,
}

impl From<authoring_application::ProductStatusV1> for ProductState {
    fn from(value: authoring_application::ProductStatusV1) -> Self {
        match value {
            authoring_application::ProductStatusV1::PendingApproval => Self::PendingApproval,
            authoring_application::ProductStatusV1::Approved => Self::Approved,
            authoring_application::ProductStatusV1::Applying => Self::Applying,
            authoring_application::ProductStatusV1::RuntimePending => Self::RuntimePending,
            authoring_application::ProductStatusV1::Live => Self::Live,
            authoring_application::ProductStatusV1::Rejected => Self::Rejected,
            authoring_application::ProductStatusV1::Expired => Self::Expired,
            authoring_application::ProductStatusV1::Superseded => Self::Superseded,
            authoring_application::ProductStatusV1::Withdrawn => Self::Withdrawn,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct CurrentPrincipalView {
    pub principal_id: String,
    pub display_name: String,
    pub csrf_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentPrincipal {
    pub principal_id: String,
    pub display_name: String,
}

impl Debug for CurrentPrincipalView {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CurrentPrincipalView")
            .field("principal_id", &self.principal_id)
            .field("display_name", &self.display_name)
            .field("csrf_token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SafeApprovalSummary {
    pub panels: u64,
    pub modals: u64,
    pub rules: u64,
    pub actions: u64,
    pub target_version: u32,
    pub target_content_hash: String,
    pub binding_fingerprint: String,
    pub required_approvals: u32,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromotionView {
    pub installation_id: String,
    pub promotion_id: String,
    pub revision: u64,
    pub state: ProductState,
    pub payload_digest: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DecisionView {
    pub installation_id: String,
    pub promotion_id: String,
    pub revision: u64,
    pub state: ProductState,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApprovalPreviewView {
    pub installation_id: String,
    pub promotion_id: String,
    pub revision: u64,
    pub state: ProductState,
    pub payload_digest: String,
    pub summary: SafeApprovalSummary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApplyView {
    pub installation_id: String,
    pub promotion_id: String,
    pub state: ProductState,
    pub replayed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentState {
    NotApplicable,
    NotRequested,
    Pending,
    Failed,
    Live,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeploymentView {
    pub installation_id: String,
    pub promotion_id: String,
    pub observed_at: DateTime<Utc>,
    pub state: DeploymentState,
    pub retryable: bool,
    pub failure_code: Option<String>,
    pub attestation_revision: Option<u64>,
    pub last_serving_heartbeat: Option<DateTime<Utc>>,
    pub serving_lease_expires_at: Option<DateTime<Utc>>,
}
