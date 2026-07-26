use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2, RuntimeDrainIntentV2,
    RuntimeObservedProductDrainV2, RuntimePersistedProductDrainRootV2,
    RuntimeProductDrainNaturalScopeV2, RuntimeProductDrainScopeCorruptionV2,
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
    RuntimeProductMutationDigestV2, RuntimeProductOperationIdV2,
};
use automation_runtime_convergence::{RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeProductDrainObservationRowV2 {
    outcome_name: String,
    locked_snapshot: Json<Value>,
    observed_at: DateTime<Utc>,
    product_tenant_id: Option<String>,
    product_installation_id: Option<String>,
    product_deployment_id: Option<String>,
    product_expected_revision: Option<i64>,
    product_operation_id: Option<String>,
    product_expected_target: Option<Json<Value>>,
    product_mutation_request_bytes: Option<Vec<u8>>,
    product_mutation_digest: Option<String>,
    drain_tenant_id: Option<String>,
    drain_installation_id: Option<String>,
    drain_deployment_id: Option<String>,
    drain_expected_revision: Option<i64>,
    drain_slot_guild_id: Option<String>,
    drain_slot_ruleset_key: Option<String>,
    drain_intent_id: Option<String>,
    drain_intent_request_bytes: Option<Vec<u8>>,
    drain_intent_digest: Option<String>,
    intent_revision: Option<i64>,
    intent_state: Option<String>,
}

impl RuntimeProductDrainObservationRowV2 {
    pub(crate) fn decode(
        self,
        lookup: RuntimeProductDrainScopeLookupV2,
    ) -> Result<RuntimeProductDrainScopeObservationV2, RuntimeExecutionPersistenceErrorV1> {
        let locked_snapshot = decode_snapshot(self.locked_snapshot.0.clone())?;
        match self.outcome_name.as_str() {
            "absent" => {
                self.require_empty_payload()?;
                RuntimeProductDrainScopeObservationV2::absent(
                    lookup,
                    locked_snapshot,
                    self.observed_at,
                )
                .map_err(|_| invalid())
            }
            "present" => self.decode_present(lookup, locked_snapshot),
            "ambiguous_product" => self.decode_corruption(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::Ambiguous(
                    RuntimeProductDrainNaturalScopeV2::ProductOperation,
                ),
                CorruptPayloadShapeV2::Empty,
            ),
            "ambiguous_drain" => self.decode_corruption(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::Ambiguous(
                    RuntimeProductDrainNaturalScopeV2::DrainIntent,
                ),
                CorruptPayloadShapeV2::Empty,
            ),
            "partial_product" => self.decode_corruption(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::PartialPair {
                    present: RuntimeProductDrainNaturalScopeV2::ProductOperation,
                },
                CorruptPayloadShapeV2::ProductOnly,
            ),
            "partial_drain" => self.decode_corruption(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::PartialPair {
                    present: RuntimeProductDrainNaturalScopeV2::DrainIntent,
                },
                CorruptPayloadShapeV2::DrainOnly,
            ),
            "pair_mismatch" => self.decode_corruption(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::PairMismatch,
                CorruptPayloadShapeV2::Both,
            ),
            _ => Err(invalid()),
        }
    }

    fn decode_present(
        self,
        lookup: RuntimeProductDrainScopeLookupV2,
        locked_snapshot: RuntimeDeploymentSnapshotV1,
    ) -> Result<RuntimeProductDrainScopeObservationV2, RuntimeExecutionPersistenceErrorV1> {
        let root = match self.decode_root(&lookup) {
            Ok(root) => root,
            Err(_) => {
                return RuntimeProductDrainScopeObservationV2::persistence_corrupt(
                    lookup,
                    locked_snapshot,
                    RuntimeProductDrainScopeCorruptionV2::PersistedRootInvalid,
                    self.observed_at,
                )
                .map_err(|_| invalid())
            }
        };
        let intent_revision = self
            .intent_revision
            .and_then(|value| u64::try_from(value).ok())
            .and_then(NonZeroU64::new);
        let intent = match (self.intent_state.as_deref(), intent_revision) {
            (Some("pending"), Some(intent_revision)) if intent_revision == NonZeroU64::MIN => {
                RuntimeDrainIntentV2::pending_from_persisted(&root, intent_revision, None)
            }
            _ => {
                return RuntimeProductDrainScopeObservationV2::persistence_corrupt(
                    lookup,
                    locked_snapshot,
                    RuntimeProductDrainScopeCorruptionV2::PersistedIntentInvalid,
                    self.observed_at,
                )
                .map_err(|_| invalid())
            }
        };
        let Ok(intent) = intent else {
            return RuntimeProductDrainScopeObservationV2::persistence_corrupt(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::PersistedIntentInvalid,
                self.observed_at,
            )
            .map_err(|_| invalid());
        };
        let persisted = match RuntimeObservedProductDrainV2::from_exact_parts(root, intent) {
            Ok(persisted) => persisted,
            Err(_) => {
                return RuntimeProductDrainScopeObservationV2::persistence_corrupt(
                    lookup,
                    locked_snapshot,
                    RuntimeProductDrainScopeCorruptionV2::PairMismatch,
                    self.observed_at,
                )
                .map_err(|_| invalid())
            }
        };
        if persisted
            .root()
            .canonical()
            .product_preimage()
            .expected_target
            != locked_snapshot.target
        {
            return RuntimeProductDrainScopeObservationV2::persistence_corrupt(
                lookup,
                locked_snapshot,
                RuntimeProductDrainScopeCorruptionV2::PersistedRootInvalid,
                self.observed_at,
            )
            .map_err(|_| invalid());
        }
        RuntimeProductDrainScopeObservationV2::present(
            lookup,
            locked_snapshot,
            persisted,
            self.observed_at,
        )
        .map_err(|_| invalid())
    }

    fn decode_root(
        &self,
        lookup: &RuntimeProductDrainScopeLookupV2,
    ) -> Result<RuntimePersistedProductDrainRootV2, RuntimeExecutionPersistenceErrorV1> {
        let product_scope = lookup.product_operation_scope();
        let drain_scope = lookup.drain_intent_scope();
        if self.product_tenant_id.as_deref() != Some(product_scope.scope().tenant_id.as_str())
            || self.product_installation_id.as_deref()
                != Some(product_scope.scope().installation_id.as_str())
            || self.product_deployment_id.as_deref()
                != Some(product_scope.scope().deployment_id.as_str())
            || self.product_expected_revision != i64_revision(product_scope.expected_revision())
            || self.drain_tenant_id.as_deref() != Some(drain_scope.scope().tenant_id.as_str())
            || self.drain_installation_id.as_deref()
                != Some(drain_scope.scope().installation_id.as_str())
            || self.drain_deployment_id.as_deref()
                != Some(drain_scope.scope().deployment_id.as_str())
            || self.drain_expected_revision != i64_revision(drain_scope.expected_revision())
            || self.drain_slot_guild_id.as_deref()
                != Some(drain_scope.slot().guild_id.to_string().as_str())
            || self.drain_slot_ruleset_key.as_deref()
                != Some(drain_scope.slot().ruleset_key.as_str())
        {
            return Err(invalid());
        }
        let product_operation_id = RuntimeProductOperationIdV2::parse(
            self.product_operation_id.clone().ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let drain_intent_id =
            RuntimeDrainIntentIdV2::parse(self.drain_intent_id.clone().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        let product_mutation_digest = RuntimeProductMutationDigestV2::parse(
            self.product_mutation_digest.clone().ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let drain_intent_digest = RuntimeDrainIntentDigestV2::parse(
            self.drain_intent_digest.clone().ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let expected_target = serde_json::from_value::<RuntimeDeploymentTargetV1>(
            self.product_expected_target.clone().ok_or_else(invalid)?.0,
        )
        .map_err(|_| invalid())?;
        RuntimePersistedProductDrainRootV2::from_persisted(
            product_scope.scope().clone(),
            product_scope.expected_revision(),
            &product_operation_id,
            drain_scope.scope().clone(),
            drain_scope.slot().clone(),
            drain_scope.expected_revision(),
            &drain_intent_id,
            &expected_target,
            self.product_mutation_request_bytes
                .as_deref()
                .ok_or_else(invalid)?,
            &product_mutation_digest,
            self.drain_intent_request_bytes
                .as_deref()
                .ok_or_else(invalid)?,
            &drain_intent_digest,
        )
        .map_err(|_| invalid())
    }

    fn decode_corruption(
        self,
        lookup: RuntimeProductDrainScopeLookupV2,
        locked_snapshot: RuntimeDeploymentSnapshotV1,
        corruption: RuntimeProductDrainScopeCorruptionV2,
        payload_shape: CorruptPayloadShapeV2,
    ) -> Result<RuntimeProductDrainScopeObservationV2, RuntimeExecutionPersistenceErrorV1> {
        self.require_payload_shape(payload_shape)?;
        RuntimeProductDrainScopeObservationV2::persistence_corrupt(
            lookup,
            locked_snapshot,
            corruption,
            self.observed_at,
        )
        .map_err(|_| invalid())
    }

    fn require_empty_payload(&self) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        self.require_payload_shape(CorruptPayloadShapeV2::Empty)
    }

    fn require_payload_shape(
        &self,
        expected: CorruptPayloadShapeV2,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        let product_any = self.product_tenant_id.is_some()
            || self.product_installation_id.is_some()
            || self.product_deployment_id.is_some()
            || self.product_expected_revision.is_some()
            || self.product_operation_id.is_some()
            || self.product_expected_target.is_some()
            || self.product_mutation_request_bytes.is_some()
            || self.product_mutation_digest.is_some();
        let product_all = self.product_tenant_id.is_some()
            && self.product_installation_id.is_some()
            && self.product_deployment_id.is_some()
            && self.product_expected_revision.is_some()
            && self.product_operation_id.is_some()
            && self.product_expected_target.is_some()
            && self.product_mutation_request_bytes.is_some()
            && self.product_mutation_digest.is_some();
        let drain_any = self.drain_tenant_id.is_some()
            || self.drain_installation_id.is_some()
            || self.drain_deployment_id.is_some()
            || self.drain_expected_revision.is_some()
            || self.drain_slot_guild_id.is_some()
            || self.drain_slot_ruleset_key.is_some()
            || self.drain_intent_id.is_some()
            || self.drain_intent_request_bytes.is_some()
            || self.drain_intent_digest.is_some()
            || self.intent_revision.is_some()
            || self.intent_state.is_some();
        let drain_all = self.drain_tenant_id.is_some()
            && self.drain_installation_id.is_some()
            && self.drain_deployment_id.is_some()
            && self.drain_expected_revision.is_some()
            && self.drain_slot_guild_id.is_some()
            && self.drain_slot_ruleset_key.is_some()
            && self.drain_intent_id.is_some()
            && self.drain_intent_request_bytes.is_some()
            && self.drain_intent_digest.is_some()
            && self.intent_revision.is_some()
            && self.intent_state.is_some();
        let matches = match expected {
            CorruptPayloadShapeV2::Empty => !product_any && !drain_any,
            CorruptPayloadShapeV2::ProductOnly => product_all && !drain_any,
            CorruptPayloadShapeV2::DrainOnly => !product_any && drain_all,
            CorruptPayloadShapeV2::Both => product_all && drain_all,
        };
        if matches {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

#[derive(Clone, Copy)]
enum CorruptPayloadShapeV2 {
    Empty,
    ProductOnly,
    DrainOnly,
    Both,
}

fn decode_snapshot(
    value: Value,
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeExecutionPersistenceErrorV1> {
    let snapshot =
        serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value).map_err(|_| invalid())?;
    automation_runtime_convergence::RuntimeDeployment::restore(snapshot.clone())
        .map_err(|_| invalid())?;
    Ok(snapshot)
}

fn i64_revision(revision: automation_runtime_convergence::DeploymentRevision) -> Option<i64> {
    i64::try_from(revision.get()).ok()
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use automation_runtime_controller::{
        RuntimeCanonicalProductDrainV2, RuntimeDeploymentScopeV1,
        RuntimeProductDrainScopeObservationKindV2, RuntimeProductMutationKindV2,
        RuntimeProductMutationPreimageV2, RuntimeProductSemanticRequestDigestV2,
        RuntimeServingSlotV2,
    };
    use automation_runtime_convergence::RuntimeDeployment;
    use serde_json::json;

    use super::*;

    fn snapshot_value() -> Value {
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

    fn snapshot() -> RuntimeDeploymentSnapshotV1 {
        let snapshot: RuntimeDeploymentSnapshotV1 =
            serde_json::from_value(snapshot_value()).unwrap();
        RuntimeDeployment::restore(snapshot.clone()).unwrap();
        snapshot
    }

    fn lookup() -> RuntimeProductDrainScopeLookupV2 {
        RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&snapshot()).unwrap()
    }

    fn empty(outcome_name: &str) -> RuntimeProductDrainObservationRowV2 {
        RuntimeProductDrainObservationRowV2 {
            outcome_name: outcome_name.to_owned(),
            locked_snapshot: Json(snapshot_value()),
            observed_at: DateTime::parse_from_rfc3339("2026-07-22T00:00:01Z")
                .unwrap()
                .with_timezone(&Utc),
            product_tenant_id: None,
            product_installation_id: None,
            product_deployment_id: None,
            product_expected_revision: None,
            product_operation_id: None,
            product_expected_target: None,
            product_mutation_request_bytes: None,
            product_mutation_digest: None,
            drain_tenant_id: None,
            drain_installation_id: None,
            drain_deployment_id: None,
            drain_expected_revision: None,
            drain_slot_guild_id: None,
            drain_slot_ruleset_key: None,
            drain_intent_id: None,
            drain_intent_request_bytes: None,
            drain_intent_digest: None,
            intent_revision: None,
            intent_state: None,
        }
    }

    fn present() -> RuntimeProductDrainObservationRowV2 {
        let snapshot = snapshot();
        present_with_expected_target(snapshot.target)
    }

    fn present_with_expected_target(
        expected_target: RuntimeDeploymentTargetV1,
    ) -> RuntimeProductDrainObservationRowV2 {
        let snapshot = snapshot();
        let preimage = RuntimeProductMutationPreimageV2 {
            operation_id: RuntimeProductOperationIdV2::parse("00112233445566778899aabbccddeeff")
                .unwrap(),
            scope: RuntimeDeploymentScopeV1::from_identity(&snapshot.identity),
            expected_revision: snapshot.revision,
            slot: RuntimeServingSlotV2::from_target(&expected_target),
            expected_target: expected_target.clone(),
            mutation_kind: RuntimeProductMutationKindV2::Teardown,
            product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
                "4".repeat(64),
            )
            .unwrap(),
        };
        let canonical = RuntimeCanonicalProductDrainV2::new(
            preimage,
            RuntimeDrainIntentIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        )
        .unwrap();
        let mut row = empty("present");
        row.product_tenant_id = Some("tenant".to_owned());
        row.product_installation_id = Some("installation".to_owned());
        row.product_deployment_id = Some("deployment".to_owned());
        row.product_expected_revision = Some(1);
        row.product_operation_id = Some(
            canonical
                .product_preimage()
                .operation_id
                .as_str()
                .to_owned(),
        );
        row.product_expected_target = Some(Json(serde_json::to_value(expected_target).unwrap()));
        row.product_mutation_request_bytes =
            Some(canonical.product_mutation_request_bytes().to_vec());
        row.product_mutation_digest = Some(canonical.product_mutation_digest().as_str().to_owned());
        row.drain_tenant_id = Some("tenant".to_owned());
        row.drain_installation_id = Some("installation".to_owned());
        row.drain_deployment_id = Some("deployment".to_owned());
        row.drain_expected_revision = Some(1);
        row.drain_slot_guild_id = Some("42".to_owned());
        row.drain_slot_ruleset_key = Some("studyroom".to_owned());
        row.drain_intent_id = Some(canonical.drain_preimage().key.intent_id.as_str().to_owned());
        row.drain_intent_request_bytes = Some(canonical.drain_intent_request_bytes().to_vec());
        row.drain_intent_digest = Some(canonical.drain_intent_digest().as_str().to_owned());
        row.intent_revision = Some(1);
        row.intent_state = Some("pending".to_owned());
        row
    }

    #[test]
    fn closed_outcomes_require_an_empty_payload() {
        assert_eq!(
            empty("absent").decode(lookup()).unwrap().kind(),
            RuntimeProductDrainScopeObservationKindV2::Absent
        );
        for outcome in ["ambiguous_product", "ambiguous_drain"] {
            assert_eq!(
                empty(outcome).decode(lookup()).unwrap().kind(),
                RuntimeProductDrainScopeObservationKindV2::PersistenceCorrupt
            );
        }
        let mut payload = empty("absent");
        payload.product_operation_id = Some("00112233445566778899aabbccddeeff".to_owned());
        assert_eq!(payload.decode(lookup()), Err(invalid()));
        assert_eq!(empty("unknown").decode(lookup()), Err(invalid()));
    }

    #[test]
    fn partial_and_mismatched_pairs_require_their_exact_payload_shape() {
        let mut product = present();
        product.outcome_name = "partial_product".to_owned();
        clear_drain(&mut product);
        assert_eq!(
            product.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PartialPair {
                present: RuntimeProductDrainNaturalScopeV2::ProductOperation,
            })
        );

        let mut drain = present();
        drain.outcome_name = "partial_drain".to_owned();
        clear_product(&mut drain);
        assert_eq!(
            drain.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PartialPair {
                present: RuntimeProductDrainNaturalScopeV2::DrainIntent,
            })
        );

        let mut pair = present();
        pair.outcome_name = "pair_mismatch".to_owned();
        assert_eq!(
            pair.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PairMismatch)
        );

        assert_eq!(empty("partial_product").decode(lookup()), Err(invalid()));
        assert_eq!(empty("partial_drain").decode(lookup()), Err(invalid()));
        assert_eq!(empty("pair_mismatch").decode(lookup()), Err(invalid()));
    }

    #[test]
    fn present_reconstructs_both_exact_canonical_roots() {
        let observation = present().decode(lookup()).unwrap();
        assert_eq!(
            observation.kind(),
            RuntimeProductDrainScopeObservationKindV2::Present
        );
        let persisted = observation.persisted().unwrap();
        assert_eq!(
            persisted.root().product_operation_id().as_str(),
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(
            persisted.intent().key().intent_id.as_str(),
            "ffeeddccbbaa99887766554433221100"
        );
        assert_eq!(persisted.intent().intent_revision(), NonZeroU64::MIN);
    }

    #[test]
    fn malformed_present_rows_become_typed_corruption() {
        let mut root = present();
        root.product_mutation_digest = Some("0".repeat(64));
        assert_eq!(
            root.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PersistedRootInvalid)
        );

        let mut state = present();
        state.intent_state = Some("consumed".to_owned());
        assert_eq!(
            state.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PersistedIntentInvalid)
        );

        let mut revision = present();
        revision.intent_revision = Some(2);
        assert_eq!(
            revision.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PersistedIntentInvalid)
        );

        let mut scope = present();
        scope.drain_slot_ruleset_key = Some("other".to_owned());
        assert_eq!(
            scope.decode(lookup()).unwrap().corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PersistedRootInvalid)
        );

        let mut expected_target = serde_json::to_value(&snapshot().target).unwrap();
        expected_target["version"] = json!(2);
        let expected_target =
            serde_json::from_value::<RuntimeDeploymentTargetV1>(expected_target).unwrap();
        assert_eq!(
            present_with_expected_target(expected_target)
                .decode(lookup())
                .unwrap()
                .corruption(),
            Some(RuntimeProductDrainScopeCorruptionV2::PersistedRootInvalid)
        );
    }

    #[test]
    fn observation_requires_a_restorable_locked_snapshot() {
        let mut row = empty("absent");
        row.locked_snapshot.0["revision"] = json!(0);
        assert_eq!(row.decode(lookup()), Err(invalid()));
    }

    fn clear_product(row: &mut RuntimeProductDrainObservationRowV2) {
        row.product_tenant_id = None;
        row.product_installation_id = None;
        row.product_deployment_id = None;
        row.product_expected_revision = None;
        row.product_operation_id = None;
        row.product_expected_target = None;
        row.product_mutation_request_bytes = None;
        row.product_mutation_digest = None;
    }

    fn clear_drain(row: &mut RuntimeProductDrainObservationRowV2) {
        row.drain_tenant_id = None;
        row.drain_installation_id = None;
        row.drain_deployment_id = None;
        row.drain_expected_revision = None;
        row.drain_slot_guild_id = None;
        row.drain_slot_ruleset_key = None;
        row.drain_intent_id = None;
        row.drain_intent_request_bytes = None;
        row.drain_intent_digest = None;
        row.intent_revision = None;
        row.intent_state = None;
    }
}
