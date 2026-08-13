use serde::{Deserialize, Serialize};

use crate::{validate_stateful_spec_v1, StatefulSpecV1, StatefulSpecValidationErrorV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatefulSpecDeploymentBlockerV1 {
    StatefulRuntimeUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSpecDeploymentStatusV1 {
    pub deployable: bool,
    pub compilation_available: bool,
    pub blockers: Vec<StatefulSpecDeploymentBlockerV1>,
}

/// A valid StatefulSpec can be compiled into a pure immutable artifact bundle, but it has no live
/// publication, promotion, Apply, persistence, or runtime activation path in R0.
pub fn stateful_spec_deployment_status_v1(
    spec: &StatefulSpecV1,
) -> Result<StatefulSpecDeploymentStatusV1, StatefulSpecValidationErrorV1> {
    validate_stateful_spec_v1(spec)?;
    Ok(StatefulSpecDeploymentStatusV1 {
        deployable: false,
        compilation_available: true,
        blockers: vec![StatefulSpecDeploymentBlockerV1::StatefulRuntimeUnavailable],
    })
}
