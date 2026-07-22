use std::num::NonZeroU32;

use automation_runtime_convergence::{
    ControllerId, FencingToken, RuntimeDeployment, RuntimeDeploymentSnapshotV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeExecutionOutcomeV1 {
    Applied,
    Replayed,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeExecutionOperationRowV1 {
    outcome_name: Option<String>,
    previous_snapshot: Option<Json<Value>>,
    snapshot: Option<Json<Value>>,
    controller_id: Option<String>,
    fencing_token: Option<i64>,
    convergence_attempt_no: Option<i64>,
    acquired_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
}

pub(crate) struct DecodedRuntimeExecutionRowV1 {
    pub(crate) outcome: RuntimeExecutionOutcomeV1,
    pub(crate) previous: RuntimeDeployment,
    pub(crate) current: RuntimeDeployment,
    pub(crate) controller_id: ControllerId,
    pub(crate) fencing_token: FencingToken,
    pub(crate) convergence_attempt: NonZeroU32,
    pub(crate) acquired_at: DateTime<Utc>,
    pub(crate) expires_at: DateTime<Utc>,
}

impl RuntimeExecutionOperationRowV1 {
    pub(crate) fn decode(
        self,
    ) -> Result<DecodedRuntimeExecutionRowV1, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let outcome = match self.outcome_name.as_deref() {
            Some("applied") => RuntimeExecutionOutcomeV1::Applied,
            Some("replayed") => RuntimeExecutionOutcomeV1::Replayed,
            _ => return Err(invalid()),
        };
        let previous = decode_deployment(self.previous_snapshot.ok_or_else(invalid)?.0)?;
        let current = decode_deployment(self.snapshot.ok_or_else(invalid)?.0)?;
        let controller_id =
            ControllerId::parse(self.controller_id.ok_or_else(invalid)?).map_err(|_| invalid())?;
        let fencing_token = positive_u64(self.fencing_token.ok_or_else(invalid)?)
            .and_then(|value| FencingToken::new(value).ok())
            .ok_or_else(invalid)?;
        let convergence_attempt =
            positive_u32(self.convergence_attempt_no.ok_or_else(invalid)?).ok_or_else(invalid)?;
        let acquired_at = self.acquired_at.ok_or_else(invalid)?;
        let expires_at = self.expires_at.ok_or_else(invalid)?;
        if acquired_at >= expires_at {
            return Err(invalid());
        }
        Ok(DecodedRuntimeExecutionRowV1 {
            outcome,
            previous,
            current,
            controller_id,
            fencing_token,
            convergence_attempt,
            acquired_at,
            expires_at,
        })
    }
}

fn decode_deployment(
    value: Value,
) -> Result<RuntimeDeployment, RuntimeExecutionPersistenceErrorV1> {
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    RuntimeDeployment::restore(snapshot)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|value| *value != 0)
}

fn positive_u32(value: i64) -> Option<NonZeroU32> {
    u32::try_from(value).ok().and_then(NonZeroU32::new)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn snapshot() -> Value {
        json!({
            "identity": {
                "deployment_id": "deployment",
                "tenant_id": "tenant",
                "installation_id": "installation",
                "promotion_id": "1".repeat(64),
                "activation_request_id": "activation"
            },
            "target": {
                "guild_id": "42",
                "ruleset_key": "studyroom",
                "version": 1,
                "content_hash": "2".repeat(64),
                "binding_revision": 1,
                "binding_fingerprint": "3".repeat(64)
            },
            "runtime_generation": 1,
            "previous_runtime": null,
            "requested_at": "2026-07-22T00:00:00Z",
            "revision": 1,
            "phase": { "phase": "requested" },
            "controller_lease": null,
            "last_fencing_token": null,
            "preflight": null,
            "drain": null,
            "activation": null,
            "panel_certificate": null,
            "gateway_ready": null,
            "live": null,
            "last_live_recovery": null,
            "last_runtime_failure": null
        })
    }

    fn row() -> RuntimeExecutionOperationRowV1 {
        RuntimeExecutionOperationRowV1 {
            outcome_name: Some("replayed".to_string()),
            previous_snapshot: Some(Json(snapshot())),
            snapshot: Some(Json(snapshot())),
            controller_id: Some("controller".to_string()),
            fencing_token: Some(1),
            convergence_attempt_no: Some(1),
            acquired_at: DateTime::parse_from_rfc3339("2026-07-22T00:00:01Z")
                .ok()
                .map(|value| value.with_timezone(&Utc)),
            expires_at: DateTime::parse_from_rfc3339("2026-07-22T00:01:01Z")
                .ok()
                .map(|value| value.with_timezone(&Utc)),
        }
    }

    #[test]
    fn row_decoder_accepts_only_closed_outcomes_and_positive_evidence() {
        assert!(row().decode().is_ok());
        for outcome in [None, Some("APPLIED"), Some("unknown")] {
            let mut candidate = row();
            candidate.outcome_name = outcome.map(str::to_string);
            assert_eq!(
                candidate.decode().err(),
                Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            );
        }
        for fencing_token in [None, Some(-1), Some(0)] {
            let mut candidate = row();
            candidate.fencing_token = fencing_token;
            assert_eq!(
                candidate.decode().err(),
                Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            );
        }
        for attempt in [None, Some(-1), Some(0), Some(i64::from(u32::MAX) + 1)] {
            let mut candidate = row();
            candidate.convergence_attempt_no = attempt;
            assert_eq!(
                candidate.decode().err(),
                Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
            );
        }
    }

    #[test]
    fn row_decoder_restores_both_snapshots_and_rejects_invalid_windows() {
        let mut malformed_previous = row();
        malformed_previous.previous_snapshot.as_mut().unwrap().0["unexpected"] = json!(true);
        assert_eq!(
            malformed_previous.decode().err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut malformed_current = row();
        malformed_current.snapshot.as_mut().unwrap().0["revision"] = json!(0);
        assert_eq!(
            malformed_current.decode().err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut inverted = row();
        inverted.expires_at = inverted.acquired_at;
        assert_eq!(
            inverted.decode().err(),
            Some(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
