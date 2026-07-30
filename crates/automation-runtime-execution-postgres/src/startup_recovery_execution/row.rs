use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1,
};
use automation_runtime_worker::{
    RuntimePendingDrainStateDigestV2, RuntimeStartupRecoveryExecutionTerminalDigestV2,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::closed_evidence::{
    validate_closed_recovery_evidence_v2, RuntimeClosedRecoveryExpectedEvidenceV2,
};
use super::digest::{
    lowercase_sha256_bytes, startup_recovery_action_digest_v2, RuntimeStartupRecoveryActionProofV2,
};
use super::pending_projection::{
    decode_pending_drain_terminal_projection_v2, RuntimePendingDrainTerminalProjectionV2,
};
use super::pending_semantic::{
    validate_pending_drain_acknowledged_projection_v2,
    validate_pending_drain_claimed_projection_v2, RuntimePendingDrainExpectationV2,
};
use super::projection::{
    decode_terminal_projection_v2, RuntimeStartupRecoveryTerminalProjectionV2,
};
use super::reserved_projection::{
    decode_reserved_terminal_projection_v2, RuntimeReservedStartupRecoveryTerminalProjectionV2,
};
use super::reserved_semantic::{
    validate_reserved_progressed_projection_v2, validate_unreserved_progressed_projection_v2,
    RuntimeReservedStartupRecoveryExpectationV2,
};
use super::semantic::validate_progressed_projection_v2;
use super::suspended_projection::{
    decode_suspended_terminal_projection_v2, RuntimeSuspendedStartupRecoveryTerminalProjectionV2,
};
use super::suspended_semantic::{
    validate_suspended_progressed_projection_v2, RuntimeSuspendedStartupRecoveryExpectationV2,
};
use crate::gateway_owner::MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION;
use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(super) struct RuntimeStartupRecoveryExecutionRowV2 {
    journal_outcome_name: String,
    terminal_outcome_name: String,
    recovery_id: String,
    originating_emergency_generation: i64,
    coordinator_generation: i64,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    recovery_class: String,
    observed_gateway_shard_id: String,
    observed_process_instance_id: String,
    observed_lease_epoch: i64,
    observed_runtime_build_revision: String,
    observed_owner_revision: i64,
    database_now: DateTime<Utc>,
    observed_owner_expires_at: DateTime<Utc>,
    minimum_database_now: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
    terminal_projection_bytes: Vec<u8>,
    terminal_digest: String,
}

pub(super) struct RuntimeStartupRecoveryExecutionExpectedV2 {
    pub recovery_id: String,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
    pub recovery_class: &'static str,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: i64,
    pub owner_expires_at: DateTime<Utc>,
    pub minimum_database_now: DateTime<Utc>,
    pub closed_evidence: Option<RuntimeClosedRecoveryExpectedEvidenceV2>,
}

pub(super) struct RuntimeStartupRecoveryExecutionDatabaseReceiptV2 {
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub outcome: RuntimeStartupRecoveryExecutionDatabaseOutcomeV2,
}

pub(super) enum RuntimeStartupRecoveryExecutionDatabaseOutcomeV2 {
    NoCandidate,
    Progressed(RuntimeStartupRecoveryExecutionTerminalDigestV2),
}

pub(super) struct RuntimePendingDrainNoCandidateDatabaseReceiptV2 {
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
}

pub(super) struct RuntimePendingDrainProgressedDatabaseReceiptV2 {
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub successor_intent_revision: std::num::NonZeroU64,
    pub successor_state_digest: RuntimePendingDrainStateDigestV2,
}

pub(super) struct RuntimePendingDrainSuccessionDatabaseRecordV3 {
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub database_now: DateTime<Utc>,
    pub minimum_database_now: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
    pub terminal_projection_bytes: Box<[u8]>,
}

impl RuntimeStartupRecoveryExecutionRowV2 {
    pub(super) fn decode(
        self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<RuntimeStartupRecoveryExecutionDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.require_exact_identity(expected)?;
        self.require_exact_owner(expected)?;
        self.require_valid_times(expected)?;
        let projection_outcome = self.decode_projection(expected)?;
        let persisted_digest = lowercase_sha256_bytes(&self.terminal_digest)?;
        let derived_digest =
            startup_recovery_action_digest_v2(&RuntimeStartupRecoveryActionProofV2 {
                recovery_id: &self.recovery_id,
                originating_emergency_generation: self.originating_emergency_generation,
                coordinator_generation: self.coordinator_generation,
                action_authority_revision: self.action_authority_revision,
                selection_authority_revision: self.selection_authority_revision,
                recovery_class: &self.recovery_class,
                gateway_shard_id: &self.observed_gateway_shard_id,
                owner_process_instance_id: &self.observed_process_instance_id,
                owner_lease_epoch: self.observed_lease_epoch,
                owner_runtime_build_revision: &self.observed_runtime_build_revision,
                owner_revision: self.observed_owner_revision,
                owner_expires_at: self.observed_owner_expires_at,
                minimum_database_now: self.minimum_database_now,
                recorded_at: self.recorded_at,
                terminal_projection_bytes: &self.terminal_projection_bytes,
            })?;
        if persisted_digest != derived_digest {
            return Err(invalid());
        }
        let terminal_digest = RuntimeStartupRecoveryExecutionTerminalDigestV2::new(derived_digest)
            .map_err(|_| invalid())?;
        let outcome = match projection_outcome {
            RuntimeStartupRecoveryDecodedProjectionOutcomeV2::NoCandidate => {
                RuntimeStartupRecoveryExecutionDatabaseOutcomeV2::NoCandidate
            }
            RuntimeStartupRecoveryDecodedProjectionOutcomeV2::Progressed => {
                RuntimeStartupRecoveryExecutionDatabaseOutcomeV2::Progressed(terminal_digest)
            }
        };
        Ok(RuntimeStartupRecoveryExecutionDatabaseReceiptV2 {
            owner_receipt: self.owner_receipt(expected)?,
            outcome,
        })
    }

    pub(super) fn decode_pending_no_candidate(
        self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        expected_evidence: &RuntimeClosedRecoveryExpectedEvidenceV2,
    ) -> Result<RuntimePendingDrainNoCandidateDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        let (owner_receipt, terminal_digest) = self.decode_pending_common(expected, false)?;
        match decode_pending_drain_terminal_projection_v2(
            &self.terminal_outcome_name,
            &self.terminal_projection_bytes,
        )? {
            RuntimePendingDrainTerminalProjectionV2::NoCandidate(evidence) => {
                validate_closed_recovery_evidence_v2(&evidence, expected_evidence)?;
            }
            RuntimePendingDrainTerminalProjectionV2::Claimed(_)
            | RuntimePendingDrainTerminalProjectionV2::RouteAbsentAcknowledged(_) => {
                return Err(invalid());
            }
        }
        Ok(RuntimePendingDrainNoCandidateDatabaseReceiptV2 {
            owner_receipt,
            terminal_digest,
        })
    }

    pub(super) fn decode_pending_claimed(
        self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        pending: &RuntimePendingDrainExpectationV2<'_>,
    ) -> Result<RuntimePendingDrainProgressedDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.decode_pending_progressed(expected, pending, true)
    }

    pub(super) fn decode_pending_acknowledged(
        self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        pending: &RuntimePendingDrainExpectationV2<'_>,
    ) -> Result<RuntimePendingDrainProgressedDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.decode_pending_progressed(expected, pending, false)
    }

    pub(super) fn decode_pending_succession(
        self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<RuntimePendingDrainSuccessionDatabaseRecordV3, RuntimeExecutionPersistenceErrorV1>
    {
        if self.terminal_outcome_name != "route_absent_acknowledged" {
            return Err(invalid());
        }
        let (owner_receipt, terminal_digest) = self.decode_pending_common(expected, false)?;
        Ok(RuntimePendingDrainSuccessionDatabaseRecordV3 {
            owner_receipt,
            terminal_digest,
            database_now: self.database_now,
            minimum_database_now: self.minimum_database_now,
            recorded_at: self.recorded_at,
            terminal_projection_bytes: self.terminal_projection_bytes.clone().into_boxed_slice(),
        })
    }

    fn decode_pending_progressed(
        self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        pending: &RuntimePendingDrainExpectationV2<'_>,
        claimed: bool,
    ) -> Result<RuntimePendingDrainProgressedDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        let (owner_receipt, terminal_digest) = self.decode_pending_common(expected, true)?;
        let projection = match decode_pending_drain_terminal_projection_v2(
            &self.terminal_outcome_name,
            &self.terminal_projection_bytes,
        )? {
            RuntimePendingDrainTerminalProjectionV2::Claimed(projection) if claimed => projection,
            RuntimePendingDrainTerminalProjectionV2::RouteAbsentAcknowledged(projection)
                if !claimed =>
            {
                projection
            }
            RuntimePendingDrainTerminalProjectionV2::NoCandidate(_)
            | RuntimePendingDrainTerminalProjectionV2::Claimed(_)
            | RuntimePendingDrainTerminalProjectionV2::RouteAbsentAcknowledged(_) => {
                return Err(invalid());
            }
        };
        if claimed {
            validate_pending_drain_claimed_projection_v2(
                &projection,
                pending,
                self.minimum_database_now,
                self.database_now,
                self.recorded_at,
            )?;
        } else {
            validate_pending_drain_acknowledged_projection_v2(
                &projection,
                pending,
                self.minimum_database_now,
                self.database_now,
                self.recorded_at,
            )?;
        }
        let successor_intent_revision =
            pending_successor_intent_revision(&projection.successor_state_bytes)?;
        let successor_state_digest = RuntimePendingDrainStateDigestV2::new(
            Sha256::digest(&projection.successor_state_bytes).into(),
        )
        .map_err(|_| invalid())?;
        Ok(RuntimePendingDrainProgressedDatabaseReceiptV2 {
            owner_receipt,
            terminal_digest,
            successor_intent_revision,
            successor_state_digest,
        })
    }

    fn decode_pending_common(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        allow_later_replay_minimum: bool,
    ) -> Result<
        (
            RuntimeGatewayOwnerLeaseReceiptV1,
            RuntimeStartupRecoveryExecutionTerminalDigestV2,
        ),
        RuntimeExecutionPersistenceErrorV1,
    > {
        self.require_exact_identity(expected)?;
        self.require_exact_owner(expected)?;
        self.require_valid_pending_times(expected, allow_later_replay_minimum)?;
        let persisted_digest = lowercase_sha256_bytes(&self.terminal_digest)?;
        let derived_digest =
            startup_recovery_action_digest_v2(&RuntimeStartupRecoveryActionProofV2 {
                recovery_id: &self.recovery_id,
                originating_emergency_generation: self.originating_emergency_generation,
                coordinator_generation: self.coordinator_generation,
                action_authority_revision: self.action_authority_revision,
                selection_authority_revision: self.selection_authority_revision,
                recovery_class: &self.recovery_class,
                gateway_shard_id: &self.observed_gateway_shard_id,
                owner_process_instance_id: &self.observed_process_instance_id,
                owner_lease_epoch: self.observed_lease_epoch,
                owner_runtime_build_revision: &self.observed_runtime_build_revision,
                owner_revision: self.observed_owner_revision,
                owner_expires_at: self.observed_owner_expires_at,
                minimum_database_now: self.minimum_database_now,
                recorded_at: self.recorded_at,
                terminal_projection_bytes: &self.terminal_projection_bytes,
            })?;
        if persisted_digest != derived_digest {
            return Err(invalid());
        }
        let terminal_digest = RuntimeStartupRecoveryExecutionTerminalDigestV2::new(derived_digest)
            .map_err(|_| invalid())?;
        Ok((self.owner_receipt(expected)?, terminal_digest))
    }

    fn require_valid_pending_times(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
        allow_later_replay_minimum: bool,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        let minimum_matches = self.minimum_database_now == expected.minimum_database_now
            || (allow_later_replay_minimum
                && self.journal_outcome_name == "replayed"
                && self.minimum_database_now < expected.minimum_database_now);
        if !minimum_matches
            || self.database_now < expected.minimum_database_now
            || self.recorded_at < self.minimum_database_now
            || self.database_now < self.recorded_at
            || self.database_now >= self.observed_owner_expires_at
            || self.recorded_at >= self.observed_owner_expires_at
            || (self.journal_outcome_name == "applied" && self.database_now != self.recorded_at)
        {
            return Err(invalid());
        }
        let owner_duration = self
            .observed_owner_expires_at
            .signed_duration_since(self.database_now)
            .to_std()
            .map_err(|_| invalid())?;
        if owner_duration.is_zero() || owner_duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION {
            Err(invalid())
        } else {
            Ok(())
        }
    }

    fn owner_receipt(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<RuntimeGatewayOwnerLeaseReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        Ok(RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: expected.gateway_owner_lease_id.clone(),
            owner_revision: u64::try_from(self.observed_owner_revision)
                .ok()
                .and_then(std::num::NonZeroU64::new)
                .ok_or_else(invalid)?,
            database_now: self.database_now,
            expires_at: self.observed_owner_expires_at,
        })
    }

    fn decode_projection(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<RuntimeStartupRecoveryDecodedProjectionOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        match self.recovery_class.as_str() {
            "stale_live" => match decode_terminal_projection_v2(
                &self.terminal_outcome_name,
                &self.terminal_projection_bytes,
            )? {
                RuntimeStartupRecoveryTerminalProjectionV2::NoCandidate => {
                    Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::NoCandidate)
                }
                RuntimeStartupRecoveryTerminalProjectionV2::Progressed(projection) => {
                    validate_progressed_projection_v2(&projection, self.recorded_at)?;
                    Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::Progressed)
                }
            },
            "reserved_awaiting_certification" => {
                match decode_reserved_terminal_projection_v2(
                    &self.terminal_outcome_name,
                    &self.terminal_projection_bytes,
                )? {
                    RuntimeReservedStartupRecoveryTerminalProjectionV2::NoCandidate => {
                        Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::NoCandidate)
                    }
                    RuntimeReservedStartupRecoveryTerminalProjectionV2::Progressed(projection) => {
                        validate_reserved_progressed_projection_v2(
                            &projection,
                            &RuntimeReservedStartupRecoveryExpectationV2 {
                                recovery_id: &expected.recovery_id,
                                originating_emergency_generation: expected
                                    .originating_emergency_generation,
                                coordinator_generation: expected.coordinator_generation,
                                action_authority_revision: expected.action_authority_revision,
                                selection_authority_revision: expected.selection_authority_revision,
                            },
                            self.recorded_at,
                        )?;
                        Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::Progressed)
                    }
                    RuntimeReservedStartupRecoveryTerminalProjectionV2::UnreservedProgressed(
                        projection,
                    ) => {
                        validate_unreserved_progressed_projection_v2(
                            &projection,
                            self.recorded_at,
                        )?;
                        Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::Progressed)
                    }
                }
            }
            "suspended_local_effect" => {
                let suspended_evidence = expected.closed_evidence.as_ref().ok_or_else(invalid)?;
                match decode_suspended_terminal_projection_v2(
                    &self.terminal_outcome_name,
                    &self.terminal_projection_bytes,
                )? {
                    RuntimeSuspendedStartupRecoveryTerminalProjectionV2::NoCandidate(evidence) => {
                        validate_closed_recovery_evidence_v2(&evidence, suspended_evidence)?;
                        Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::NoCandidate)
                    }
                    RuntimeSuspendedStartupRecoveryTerminalProjectionV2::Progressed(projection) => {
                        validate_suspended_progressed_projection_v2(
                            &projection,
                            &RuntimeSuspendedStartupRecoveryExpectationV2 {
                                recovery_id: &expected.recovery_id,
                                originating_emergency_generation: expected
                                    .originating_emergency_generation,
                                coordinator_generation: expected.coordinator_generation,
                                action_authority_revision: expected.action_authority_revision,
                                selection_authority_revision: expected.selection_authority_revision,
                                gateway_owner_lease_id: &expected.gateway_owner_lease_id,
                                owner_revision: expected.owner_revision,
                                owner_expires_at: expected.owner_expires_at,
                                evidence: suspended_evidence,
                            },
                        )?;
                        Ok(RuntimeStartupRecoveryDecodedProjectionOutcomeV2::Progressed)
                    }
                }
            }
            _ => Err(invalid()),
        }
    }

    fn require_exact_identity(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        if !matches!(self.journal_outcome_name.as_str(), "applied" | "replayed")
            || self.recovery_id != expected.recovery_id
            || self.originating_emergency_generation != expected.originating_emergency_generation
            || self.coordinator_generation != expected.coordinator_generation
            || self.action_authority_revision != expected.action_authority_revision
            || self.selection_authority_revision != expected.selection_authority_revision
            || self.recovery_class != expected.recovery_class
        {
            Err(invalid())
        } else {
            Ok(())
        }
    }

    fn require_exact_owner(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        let lease_id = &expected.gateway_owner_lease_id;
        if self.observed_gateway_shard_id != lease_id.gateway_shard_id.as_str()
            || self.observed_process_instance_id != lease_id.process_instance_id.as_str()
            || self.observed_lease_epoch
                != i64::try_from(lease_id.lease_epoch.get()).map_err(|_| invalid())?
            || self.observed_runtime_build_revision != lease_id.expected_build_revision.as_str()
            || self.observed_owner_revision != expected.owner_revision
            || self.observed_owner_expires_at != expected.owner_expires_at
        {
            Err(invalid())
        } else {
            Ok(())
        }
    }

    fn require_valid_times(
        &self,
        expected: &RuntimeStartupRecoveryExecutionExpectedV2,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        if self.minimum_database_now != expected.minimum_database_now
            || self.database_now < self.minimum_database_now
            || self.recorded_at < self.minimum_database_now
            || self.database_now < self.recorded_at
            || self.database_now >= self.observed_owner_expires_at
            || self.recorded_at >= self.observed_owner_expires_at
            || (self.journal_outcome_name == "applied" && self.database_now != self.recorded_at)
        {
            return Err(invalid());
        }
        let owner_duration = self
            .observed_owner_expires_at
            .signed_duration_since(self.database_now)
            .to_std()
            .map_err(|_| invalid())?;
        if owner_duration.is_zero() || owner_duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION {
            Err(invalid())
        } else {
            Ok(())
        }
    }
}

fn pending_successor_intent_revision(
    state_bytes: &[u8],
) -> Result<std::num::NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    let revision = serde_json::from_slice::<serde_json::Value>(state_bytes)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .and_then(|object| object.get("intent_revision"))
                .and_then(serde_json::Value::as_u64)
        })
        .filter(|revision| *revision <= i64::MAX as u64)
        .and_then(std::num::NonZeroU64::new)
        .ok_or_else(invalid)?;
    Ok(revision)
}

enum RuntimeStartupRecoveryDecodedProjectionOutcomeV2 {
    NoCandidate,
    Progressed,
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_controller::{
        GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseIdV1,
    };
    use automation_runtime_convergence::ProcessInstanceId;

    use super::*;
    use crate::startup_recovery_execution::digest::{
        startup_recovery_action_digest_v2, RuntimeStartupRecoveryActionProofV2,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn expected_for(recovery_class: &'static str) -> RuntimeStartupRecoveryExecutionExpectedV2 {
        RuntimeStartupRecoveryExecutionExpectedV2 {
            recovery_id: "0123456789abcdef0123456789abcdef".to_owned(),
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
            recovery_class,
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                lease_epoch: NonZeroU64::new(6).unwrap(),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            owner_revision: 7,
            owner_expires_at: at(200),
            minimum_database_now: at(100),
            closed_evidence: None,
        }
    }

    fn expected() -> RuntimeStartupRecoveryExecutionExpectedV2 {
        expected_for("stale_live")
    }

    fn no_candidate_projection(recovery_class: &str) -> Vec<u8> {
        let domain = match recovery_class {
            "stale_live" => b"starring.runtime.startup_recovery.stale_live.terminal.v2".as_slice(),
            "reserved_awaiting_certification" => {
                b"starring.runtime.startup_recovery.reserved_awaiting_certification.terminal.v2"
                    .as_slice()
            }
            _ => panic!(),
        };
        [
            (domain.len() as i64).to_be_bytes().as_slice(),
            domain,
            2_i16.to_be_bytes().as_slice(),
            0_i16.to_be_bytes().as_slice(),
        ]
        .concat()
    }

    fn row() -> RuntimeStartupRecoveryExecutionRowV2 {
        let expected = expected();
        let projection = no_candidate_projection(expected.recovery_class);
        let recorded_at = at(101);
        let digest = startup_recovery_action_digest_v2(&RuntimeStartupRecoveryActionProofV2 {
            recovery_id: &expected.recovery_id,
            originating_emergency_generation: expected.originating_emergency_generation,
            coordinator_generation: expected.coordinator_generation,
            action_authority_revision: expected.action_authority_revision,
            selection_authority_revision: expected.selection_authority_revision,
            recovery_class: expected.recovery_class,
            gateway_shard_id: expected.gateway_owner_lease_id.gateway_shard_id.as_str(),
            owner_process_instance_id: expected.gateway_owner_lease_id.process_instance_id.as_str(),
            owner_lease_epoch: 6,
            owner_runtime_build_revision: expected
                .gateway_owner_lease_id
                .expected_build_revision
                .as_str(),
            owner_revision: expected.owner_revision,
            owner_expires_at: expected.owner_expires_at,
            minimum_database_now: expected.minimum_database_now,
            recorded_at,
            terminal_projection_bytes: &projection,
        })
        .unwrap();
        RuntimeStartupRecoveryExecutionRowV2 {
            journal_outcome_name: "applied".to_owned(),
            terminal_outcome_name: "no_candidate".to_owned(),
            recovery_id: expected.recovery_id,
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
            recovery_class: "stale_live".to_owned(),
            observed_gateway_shard_id: "shard:0".to_owned(),
            observed_process_instance_id: "process:1".to_owned(),
            observed_lease_epoch: 6,
            observed_runtime_build_revision: "build:1".to_owned(),
            observed_owner_revision: 7,
            database_now: recorded_at,
            observed_owner_expires_at: at(200),
            minimum_database_now: at(100),
            recorded_at,
            terminal_projection_bytes: projection,
            terminal_digest: digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        }
    }

    #[test]
    fn no_candidate_recomputes_digest_before_discarding_it_from_the_receipt() {
        let decoded = row().decode(&expected()).unwrap();
        assert!(matches!(
            decoded.outcome,
            RuntimeStartupRecoveryExecutionDatabaseOutcomeV2::NoCandidate
        ));
        assert_eq!(decoded.owner_receipt.database_now, at(101));

        let mut tampered = row();
        tampered.terminal_digest.replace_range(0..2, "ff");
        assert!(tampered.decode(&expected()).is_err());
    }

    #[test]
    fn reserved_no_candidate_uses_its_own_projection_domain() {
        let expected = expected_for("reserved_awaiting_certification");
        let mut reserved = row();
        reserved.recovery_class = expected.recovery_class.to_owned();
        reserved.terminal_projection_bytes = no_candidate_projection(expected.recovery_class);
        reserved.terminal_digest =
            startup_recovery_action_digest_v2(&RuntimeStartupRecoveryActionProofV2 {
                recovery_id: &expected.recovery_id,
                originating_emergency_generation: expected.originating_emergency_generation,
                coordinator_generation: expected.coordinator_generation,
                action_authority_revision: expected.action_authority_revision,
                selection_authority_revision: expected.selection_authority_revision,
                recovery_class: expected.recovery_class,
                gateway_shard_id: expected.gateway_owner_lease_id.gateway_shard_id.as_str(),
                owner_process_instance_id: expected
                    .gateway_owner_lease_id
                    .process_instance_id
                    .as_str(),
                owner_lease_epoch: 6,
                owner_runtime_build_revision: expected
                    .gateway_owner_lease_id
                    .expected_build_revision
                    .as_str(),
                owner_revision: expected.owner_revision,
                owner_expires_at: expected.owner_expires_at,
                minimum_database_now: expected.minimum_database_now,
                recorded_at: reserved.recorded_at,
                terminal_projection_bytes: &reserved.terminal_projection_bytes,
            })
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert!(matches!(
            reserved.decode(&expected).unwrap().outcome,
            RuntimeStartupRecoveryExecutionDatabaseOutcomeV2::NoCandidate
        ));
    }

    #[test]
    fn replay_accepts_later_database_now_but_preserves_recorded_proof() {
        let mut replayed = row();
        replayed.journal_outcome_name = "replayed".to_owned();
        replayed.database_now = at(102);
        assert!(replayed.decode(&expected()).is_ok());
    }

    #[test]
    fn pending_replay_separates_requested_clock_floor_from_persisted_action_minimum() {
        let mut expected = expected_for("pending_runtime_drain_intent");
        expected.minimum_database_now = at(102);
        let mut replayed = row();
        replayed.recovery_class = expected.recovery_class.to_owned();
        replayed.journal_outcome_name = "replayed".to_owned();
        replayed.database_now = at(103);
        assert!(replayed
            .require_valid_pending_times(&expected, true)
            .is_ok());
        assert!(replayed
            .require_valid_pending_times(&expected, false)
            .is_err());

        replayed.minimum_database_now = at(103);
        assert!(replayed
            .require_valid_pending_times(&expected, true)
            .is_err());
        replayed.minimum_database_now = at(100);
        replayed.database_now = at(101);
        assert!(replayed
            .require_valid_pending_times(&expected, true)
            .is_err());
    }

    #[test]
    fn every_echoed_identity_and_owner_field_is_exact() {
        type RowMutation = Box<dyn Fn(&mut RuntimeStartupRecoveryExecutionRowV2)>;
        let mut mismatches: Vec<RowMutation> = vec![
            Box::new(|row| row.recovery_id = "fedcba9876543210fedcba9876543210".to_owned()),
            Box::new(|row| row.originating_emergency_generation += 1),
            Box::new(|row| row.coordinator_generation += 1),
            Box::new(|row| row.action_authority_revision += 1),
            Box::new(|row| row.selection_authority_revision += 1),
            Box::new(|row| row.recovery_class = "suspended_local_effect".to_owned()),
            Box::new(|row| row.observed_gateway_shard_id = "shard:1".to_owned()),
            Box::new(|row| row.observed_process_instance_id = "process:2".to_owned()),
            Box::new(|row| row.observed_lease_epoch += 1),
            Box::new(|row| row.observed_runtime_build_revision = "build:2".to_owned()),
            Box::new(|row| row.observed_owner_revision += 1),
            Box::new(|row| row.observed_owner_expires_at = at(201)),
            Box::new(|row| row.minimum_database_now = at(99)),
        ];
        for mismatch in mismatches.drain(..) {
            let mut invalid_row = row();
            mismatch(&mut invalid_row);
            assert!(invalid_row.decode(&expected()).is_err());
        }
    }
}
