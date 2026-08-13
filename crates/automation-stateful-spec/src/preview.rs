use serde::{Deserialize, Serialize};

use crate::{
    stateful_spec_deployment_status_v1, stateful_spec_digest_v1, StatefulSpecDeploymentStatusV1,
    StatefulSpecDigestErrorV1, StatefulSpecDigestV1, StatefulSpecV1,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSpecPreviewV1 {
    pub spec_digest: StatefulSpecDigestV1,
    pub stateless_workflow_count: u16,
    pub state_variable_count: u16,
    pub stateful_workflow_count: u16,
    pub deployment: StatefulSpecDeploymentStatusV1,
}

pub fn preview_stateful_spec_v1(
    spec: &StatefulSpecV1,
) -> Result<StatefulSpecPreviewV1, StatefulSpecDigestErrorV1> {
    let spec_digest = stateful_spec_digest_v1(spec)?;
    let deployment =
        stateful_spec_deployment_status_v1(spec).map_err(StatefulSpecDigestErrorV1::Invalid)?;
    Ok(StatefulSpecPreviewV1 {
        spec_digest,
        stateless_workflow_count: spec.stateless_workflows.len() as u16,
        state_variable_count: spec.state_variables.len() as u16,
        stateful_workflow_count: spec.stateful_workflows.len() as u16,
        deployment,
    })
}
