use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2,
    RuntimeLiveAttestationDigestV2, RuntimeServingIdentityV2, RuntimeServingReceiptV2,
};
use automation_runtime_convergence::{
    DeploymentId, InstallationId, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::connection::ServingConnectionGuardV1;
use crate::contract::{
    DISCONNECT_PENDING_DRAIN_SOURCE_IF_EXPIRED_QUERY, OBSERVE_PENDING_DRAIN_SOURCE_QUERY,
};
use crate::database::{begin_serving_mutation_transaction, verify_runtime_serving_binding_v1};
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::v2::RuntimeServingMutationRowV2;
use crate::{PostgresRuntimeServingLeaseV1, RuntimeServingPersistenceErrorV1};

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainServingLookupV1 {
    intent_id: RuntimeDrainIntentIdV2,
    source_intent_revision: NonZeroU64,
    source_state_digest: [u8; 32],
}

impl RuntimePendingDrainServingLookupV1 {
    pub fn new(
        intent_id: RuntimeDrainIntentIdV2,
        source_intent_revision: NonZeroU64,
        source_state_digest: [u8; 32],
    ) -> Result<Self, RuntimeServingPersistenceErrorV1> {
        if source_intent_revision.get() > i64::MAX as u64 || source_state_digest == [0; 32] {
            return Err(RuntimeServingPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            intent_id,
            source_intent_revision,
            source_state_digest,
        })
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn source_state_digest(&self) -> &[u8; 32] {
        &self.source_state_digest
    }
}

impl Debug for RuntimePendingDrainServingLookupV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainServingLookupV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainServingSourceEvidenceV1 {
    intent_id: RuntimeDrainIntentIdV2,
    source_intent_revision: NonZeroU64,
    source_state_digest: [u8; 32],
}

impl RuntimePendingDrainServingSourceEvidenceV1 {
    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn source_state_digest(&self) -> &[u8; 32] {
        &self.source_state_digest
    }
}

impl From<&RuntimePendingDrainServingLookupV1> for RuntimePendingDrainServingSourceEvidenceV1 {
    fn from(lookup: &RuntimePendingDrainServingLookupV1) -> Self {
        Self {
            intent_id: lookup.intent_id.clone(),
            source_intent_revision: lookup.source_intent_revision,
            source_state_digest: lookup.source_state_digest,
        }
    }
}

impl Debug for RuntimePendingDrainServingSourceEvidenceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainServingSourceEvidenceV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimePendingDrainServingObservationV1 {
    Absent {
        source: RuntimePendingDrainServingSourceEvidenceV1,
        observed_at: DateTime<Utc>,
    },
    Fresh {
        source: RuntimePendingDrainServingSourceEvidenceV1,
        serving: Box<RuntimeServingReceiptV2>,
        observed_at: DateTime<Utc>,
    },
    Expired {
        source: RuntimePendingDrainServingSourceEvidenceV1,
        serving: Box<RuntimeServingReceiptV2>,
        observed_at: DateTime<Utc>,
    },
    Disconnected {
        source: RuntimePendingDrainServingSourceEvidenceV1,
        serving: Box<RuntimeServingReceiptV2>,
        observed_at: DateTime<Utc>,
    },
    Diverged {
        observed_at: DateTime<Utc>,
    },
}

impl PostgresRuntimeServingLeaseV1 {
    pub async fn observe_pending_drain_source_serving_v1(
        &self,
        lookup: &RuntimePendingDrainServingLookupV1,
    ) -> Result<RuntimePendingDrainServingObservationV1, RuntimeServingPersistenceErrorV1> {
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeServingPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ServingConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(deadline, async {
            let mut transaction =
                begin_serving_mutation_transaction(database_connection, self.timeouts).await?;
            verify_runtime_serving_binding_v1(&mut transaction, &self.expectation).await?;
            let rows = sqlx::query_as::<_, RuntimePendingDrainServingObservationRowV1>(
                OBSERVE_PENDING_DRAIN_SOURCE_QUERY,
            )
            .bind(lookup.intent_id.as_str())
            .bind(runtime_i64(lookup.source_intent_revision.get())?)
            .bind(lowercase_hex(&lookup.source_state_digest))
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_query_error)?;
            let [row] = rows.as_slice() else {
                return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
            };
            let observation = row.decode(lookup)?;
            transaction.commit().await.map_err(map_query_error)?;
            Ok(observation)
        })
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) => Err(RuntimeServingPersistenceErrorV1::Timeout),
        }
    }

    pub async fn disconnect_pending_drain_source_serving_if_expired_v1(
        &self,
        lookup: &RuntimePendingDrainServingLookupV1,
        identity: &RuntimeServingIdentityV2,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeServingPersistenceErrorV1::Indeterminate)?
            .map_err(map_query_error)?;
        let mut connection = ServingConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(deadline, async {
            let mut transaction =
                begin_serving_mutation_transaction(database_connection, self.timeouts).await?;
            verify_runtime_serving_binding_v1(&mut transaction, &self.expectation).await?;
            let target = &identity.process_identity.target;
            let rows = sqlx::query_as::<_, RuntimeServingMutationRowV2>(
                DISCONNECT_PENDING_DRAIN_SOURCE_IF_EXPIRED_QUERY,
            )
            .bind(lookup.intent_id.as_str())
            .bind(runtime_i64(lookup.source_intent_revision.get())?)
            .bind(lowercase_hex(&lookup.source_state_digest))
            .bind(identity.operation_id.as_str())
            .bind(identity.scope.tenant_id.as_str())
            .bind(identity.scope.installation_id.as_str())
            .bind(identity.scope.deployment_id.as_str())
            .bind(target.guild_id.to_string())
            .bind(target.ruleset_key.as_str())
            .bind(i64::from(target.version.get()))
            .bind(target.content_hash.to_hex())
            .bind(runtime_i64(target.binding_revision.get())?)
            .bind(target.binding_fingerprint.as_str())
            .bind(identity.attestation_digest.as_str())
            .bind(identity.process_identity.process_instance_id.as_str())
            .bind(runtime_i64(
                identity.process_identity.runtime_generation.get(),
            )?)
            .bind(runtime_i64(identity.lease_epoch.get())?)
            .bind(runtime_i64(identity.revision.get())?)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_mutation_error)?;
            let [row] = rows.as_slice() else {
                return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
            };
            let receipt = row.decode_disconnect(identity)?;
            transaction
                .commit()
                .await
                .map_err(map_mutation_commit_error)?;
            Ok(receipt)
        })
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) => Err(RuntimeServingPersistenceErrorV1::Indeterminate),
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimePendingDrainServingObservationRowV1 {
    outcome_name: String,
    drain_intent_id: Option<String>,
    source_intent_revision: Option<i64>,
    source_state_digest: Option<String>,
    operation_id: Option<String>,
    tenant_id: Option<String>,
    installation_id: Option<String>,
    deployment_id: Option<String>,
    attestation_digest: Option<String>,
    process_identity: Option<Json<Value>>,
    lease_epoch: Option<i64>,
    serving_revision: Option<i64>,
    acquired_at: Option<DateTime<Utc>>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    connected: Option<bool>,
    serving: Option<bool>,
    observed_at: DateTime<Utc>,
}

impl RuntimePendingDrainServingObservationRowV1 {
    fn decode(
        &self,
        expected: &RuntimePendingDrainServingLookupV1,
    ) -> Result<RuntimePendingDrainServingObservationV1, RuntimeServingPersistenceErrorV1> {
        match self.outcome_name.as_str() {
            "diverged" => {
                self.require_all_empty()?;
                Ok(RuntimePendingDrainServingObservationV1::Diverged {
                    observed_at: self.observed_at,
                })
            }
            "absent" => {
                self.require_source(expected)?;
                self.require_serving_empty()?;
                Ok(RuntimePendingDrainServingObservationV1::Absent {
                    source: expected.into(),
                    observed_at: self.observed_at,
                })
            }
            "current" => {
                self.require_source(expected)?;
                let serving = Box::new(self.decode_serving()?);
                if serving.acquired_at > serving.last_heartbeat_at
                    || serving.last_heartbeat_at > serving.expires_at
                    || serving.last_heartbeat_at > self.observed_at
                    || serving.connected != serving.serving
                {
                    return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
                }
                match (serving.connected, serving.expires_at > self.observed_at) {
                    (true, true) => Ok(RuntimePendingDrainServingObservationV1::Fresh {
                        source: expected.into(),
                        serving,
                        observed_at: self.observed_at,
                    }),
                    (true, false) => Ok(RuntimePendingDrainServingObservationV1::Expired {
                        source: expected.into(),
                        serving,
                        observed_at: self.observed_at,
                    }),
                    (false, false) if serving.last_heartbeat_at == serving.expires_at => {
                        Ok(RuntimePendingDrainServingObservationV1::Disconnected {
                            source: expected.into(),
                            serving,
                            observed_at: self.observed_at,
                        })
                    }
                    _ => Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt),
                }
            }
            _ => Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt),
        }
    }

    fn require_source(
        &self,
        expected: &RuntimePendingDrainServingLookupV1,
    ) -> Result<(), RuntimeServingPersistenceErrorV1> {
        if self.drain_intent_id.as_deref() != Some(expected.intent_id.as_str())
            || self.source_intent_revision
                != Some(runtime_i64(expected.source_intent_revision.get())?)
            || self.source_state_digest.as_deref()
                != Some(lowercase_hex(&expected.source_state_digest).as_str())
        {
            return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(())
    }

    fn decode_serving(&self) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        let identity = RuntimeServingIdentityV2 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse(required(self.tenant_id.clone())?)
                    .map_err(|_| invalid())?,
                installation_id: InstallationId::parse(required(self.installation_id.clone())?)
                    .map_err(|_| invalid())?,
                deployment_id: DeploymentId::parse(required(self.deployment_id.clone())?)
                    .map_err(|_| invalid())?,
            },
            operation_id: RuntimeCertificationOperationIdV2::parse(required(
                self.operation_id.clone(),
            )?)
            .map_err(|_| invalid())?,
            attestation_digest: RuntimeLiveAttestationDigestV2::parse(required(
                self.attestation_digest.clone(),
            )?)
            .map_err(|_| invalid())?,
            process_identity: decode_process_identity(required(self.process_identity.clone())?)?,
            lease_epoch: positive_nonzero(required(self.lease_epoch)?)?,
            revision: positive_nonzero(required(self.serving_revision)?)?,
        };
        Ok(RuntimeServingReceiptV2 {
            identity,
            acquired_at: required(self.acquired_at)?,
            last_heartbeat_at: required(self.last_heartbeat_at)?,
            expires_at: required(self.expires_at)?,
            connected: required(self.connected)?,
            serving: required(self.serving)?,
        })
    }

    fn require_serving_empty(&self) -> Result<(), RuntimeServingPersistenceErrorV1> {
        let empty = self.operation_id.is_none()
            && self.tenant_id.is_none()
            && self.installation_id.is_none()
            && self.deployment_id.is_none()
            && self.attestation_digest.is_none()
            && self.process_identity.is_none()
            && self.lease_epoch.is_none()
            && self.serving_revision.is_none()
            && self.acquired_at.is_none()
            && self.last_heartbeat_at.is_none()
            && self.expires_at.is_none()
            && self.connected.is_none()
            && self.serving.is_none();
        if empty {
            Ok(())
        } else {
            Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
        }
    }

    fn require_all_empty(&self) -> Result<(), RuntimeServingPersistenceErrorV1> {
        if self.drain_intent_id.is_none()
            && self.source_intent_revision.is_none()
            && self.source_state_digest.is_none()
        {
            self.require_serving_empty()
        } else {
            Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
        }
    }
}

fn required<T>(value: Option<T>) -> Result<T, RuntimeServingPersistenceErrorV1> {
    value.ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
}

fn invalid() -> RuntimeServingPersistenceErrorV1 {
    RuntimeServingPersistenceErrorV1::PersistenceCorrupt
}

fn runtime_i64(value: u64) -> Result<i64, RuntimeServingPersistenceErrorV1> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeServingPersistenceErrorV1::InvalidInput)
}

fn positive_nonzero(value: i64) -> Result<NonZeroU64, RuntimeServingPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
}

fn decode_process_identity(
    process_identity: Json<Value>,
) -> Result<RuntimeProcessIdentityV1, RuntimeServingPersistenceErrorV1> {
    let mut value = process_identity.0;
    let guild_id = value
        .as_object_mut()
        .and_then(|identity| identity.get_mut("target"))
        .and_then(Value::as_object_mut)
        .and_then(|target| target.get_mut("guild_id"))
        .ok_or_else(invalid)?;
    let canonical = match guild_id {
        Value::Number(number) => number.as_u64().map(|value| value.to_string()),
        Value::String(value) => value
            .parse::<u64>()
            .ok()
            .filter(|parsed| parsed.to_string() == *value)
            .map(|parsed| parsed.to_string()),
        _ => None,
    }
    .ok_or_else(invalid)?;
    guild_id.clone_from(&Value::String(canonical));
    serde_json::from_value(value).map_err(|_| invalid())
}

fn lowercase_hex(value: &[u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in value {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup() -> RuntimePendingDrainServingLookupV1 {
        RuntimePendingDrainServingLookupV1::new(
            RuntimeDrainIntentIdV2::parse("00112233445566778899aabbccddeeff").unwrap(),
            NonZeroU64::new(7).unwrap(),
            [0xab; 32],
        )
        .unwrap()
    }

    fn empty_row(outcome_name: &str) -> RuntimePendingDrainServingObservationRowV1 {
        RuntimePendingDrainServingObservationRowV1 {
            outcome_name: outcome_name.to_string(),
            drain_intent_id: None,
            source_intent_revision: None,
            source_state_digest: None,
            operation_id: None,
            tenant_id: None,
            installation_id: None,
            deployment_id: None,
            attestation_digest: None,
            process_identity: None,
            lease_epoch: None,
            serving_revision: None,
            acquired_at: None,
            last_heartbeat_at: None,
            expires_at: None,
            connected: None,
            serving: None,
            observed_at: DateTime::from_timestamp(100, 0).unwrap(),
        }
    }

    fn current_row(
        last_heartbeat_at: i64,
        expires_at: i64,
        connected: bool,
    ) -> RuntimePendingDrainServingObservationRowV1 {
        let lookup = lookup();
        RuntimePendingDrainServingObservationRowV1 {
            outcome_name: "current".to_string(),
            drain_intent_id: Some(lookup.intent_id().to_string()),
            source_intent_revision: Some(7),
            source_state_digest: Some("ab".repeat(32)),
            operation_id: Some("11".repeat(16)),
            tenant_id: Some("serving-test-tenant".to_string()),
            installation_id: Some("serving-test-installation".to_string()),
            deployment_id: Some("serving-test-deployment".to_string()),
            attestation_digest: Some("22".repeat(32)),
            process_identity: Some(Json(serde_json::json!({
                "target": {
                    "guild_id": 9300101,
                    "ruleset_key": "serving_test_ruleset",
                    "version": 1,
                    "content_hash": "33".repeat(32),
                    "binding_revision": 1,
                    "binding_fingerprint": "44".repeat(32)
                },
                "runtime_generation": 1,
                "process_instance_id": "serving-test-process"
            }))),
            lease_epoch: Some(1),
            serving_revision: Some(2),
            acquired_at: Some(DateTime::from_timestamp(80, 0).unwrap()),
            last_heartbeat_at: Some(DateTime::from_timestamp(last_heartbeat_at, 0).unwrap()),
            expires_at: Some(DateTime::from_timestamp(expires_at, 0).unwrap()),
            connected: Some(connected),
            serving: Some(connected),
            observed_at: DateTime::from_timestamp(100, 0).unwrap(),
        }
    }

    fn set_guild_id(row: &mut RuntimePendingDrainServingObservationRowV1, value: Value) {
        row.process_identity.as_mut().unwrap().0["target"]["guild_id"] = value;
    }

    #[test]
    fn lookup_rejects_zero_digest_and_unpersistable_revision() {
        let intent = RuntimeDrainIntentIdV2::parse("00112233445566778899aabbccddeeff").unwrap();
        assert_eq!(
            RuntimePendingDrainServingLookupV1::new(
                intent.clone(),
                NonZeroU64::new(1).unwrap(),
                [0; 32],
            ),
            Err(RuntimeServingPersistenceErrorV1::InvalidInput)
        );
        assert_eq!(
            RuntimePendingDrainServingLookupV1::new(
                intent,
                NonZeroU64::new(i64::MAX as u64 + 1).unwrap(),
                [1; 32],
            ),
            Err(RuntimeServingPersistenceErrorV1::InvalidInput)
        );
    }

    #[test]
    fn absent_and_diverged_rows_require_exact_payload_shapes() {
        let lookup = lookup();
        let mut absent = empty_row("absent");
        absent.drain_intent_id = Some(lookup.intent_id().to_string());
        absent.source_intent_revision = Some(7);
        absent.source_state_digest = Some("ab".repeat(32));
        assert_eq!(
            absent.decode(&lookup).unwrap(),
            RuntimePendingDrainServingObservationV1::Absent {
                source: RuntimePendingDrainServingSourceEvidenceV1::from(&lookup),
                observed_at: absent.observed_at
            }
        );

        let diverged = empty_row("diverged");
        assert_eq!(
            diverged.decode(&lookup).unwrap(),
            RuntimePendingDrainServingObservationV1::Diverged {
                observed_at: diverged.observed_at
            }
        );

        absent.operation_id = Some("f".repeat(32));
        assert_eq!(
            absent.decode(&lookup),
            Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn current_rows_are_classified_against_database_time() {
        let lookup = lookup();
        assert!(matches!(
            current_row(90, 110, true).decode(&lookup).unwrap(),
            RuntimePendingDrainServingObservationV1::Fresh { .. }
        ));
        assert!(matches!(
            current_row(90, 90, true).decode(&lookup).unwrap(),
            RuntimePendingDrainServingObservationV1::Expired { .. }
        ));
        assert!(matches!(
            current_row(90, 90, false).decode(&lookup).unwrap(),
            RuntimePendingDrainServingObservationV1::Disconnected { .. }
        ));
        assert_eq!(
            current_row(90, 110, false).decode(&lookup),
            Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn process_identity_accepts_sql_number_and_canonical_string_guild_ids() {
        let lookup = lookup();
        for guild_id in [
            serde_json::json!(9300101),
            serde_json::json!(u64::MAX),
            serde_json::json!("9300101"),
            serde_json::json!(u64::MAX.to_string()),
        ] {
            let mut row = current_row(90, 110, true);
            set_guild_id(&mut row, guild_id);
            assert!(matches!(
                row.decode(&lookup).unwrap(),
                RuntimePendingDrainServingObservationV1::Fresh { .. }
            ));
        }
    }

    #[test]
    fn process_identity_rejects_noncanonical_or_out_of_range_guild_ids() {
        let lookup = lookup();
        for guild_id in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("-1"),
            serde_json::json!("01"),
            serde_json::json!("18446744073709551616"),
            serde_json::json!(true),
            serde_json::json!({}),
            Value::Null,
        ] {
            let mut row = current_row(90, 110, true);
            set_guild_id(&mut row, guild_id);
            assert_eq!(
                row.decode(&lookup),
                Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
            );
        }
    }
}
