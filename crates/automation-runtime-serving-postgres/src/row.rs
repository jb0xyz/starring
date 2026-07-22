use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeDisconnectServingV1, RuntimeHeartbeatServingV1, RuntimeServingIdentityV1,
    RuntimeServingReceiptV1,
};
use chrono::{DateTime, Utc};

use crate::RuntimeServingPersistenceErrorV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeServingMutationKindV1 {
    Heartbeat,
    Disconnect,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeServingMutationRowV1 {
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    guild_id: String,
    ruleset_key: String,
    attestation_id: String,
    process_instance_id: String,
    runtime_generation: i64,
    lease_epoch: i64,
    revision: i64,
    acquired_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    connected: bool,
    serving: bool,
}

impl RuntimeServingMutationRowV1 {
    pub(crate) fn decode_heartbeat(
        &self,
        request: &RuntimeHeartbeatServingV1,
    ) -> Result<RuntimeServingReceiptV1, RuntimeServingPersistenceErrorV1> {
        let expected_duration = chrono::Duration::from_std(request.lease_for)
            .map_err(|_| RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
        if !heartbeat_replay_contract(
            request.identity.expected_revision.get(),
            self.revision,
            self.acquired_at,
            self.last_heartbeat_at,
            self.expires_at,
            expected_duration,
        ) {
            return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
        }
        self.decode(&request.identity, RuntimeServingMutationKindV1::Heartbeat)
    }

    pub(crate) fn decode_disconnect(
        &self,
        request: &RuntimeDisconnectServingV1,
    ) -> Result<RuntimeServingReceiptV1, RuntimeServingPersistenceErrorV1> {
        self.decode(&request.identity, RuntimeServingMutationKindV1::Disconnect)
    }

    fn decode(
        &self,
        expected: &RuntimeServingIdentityV1,
        kind: RuntimeServingMutationKindV1,
    ) -> Result<RuntimeServingReceiptV1, RuntimeServingPersistenceErrorV1> {
        let invalid = || RuntimeServingPersistenceErrorV1::PersistenceCorrupt;
        let runtime_generation = positive_i64(self.runtime_generation).ok_or_else(invalid)?;
        let lease_epoch = positive_i64(self.lease_epoch).ok_or_else(invalid)?;
        let revision = positive_i64(self.revision)
            .and_then(NonZeroU64::new)
            .ok_or_else(invalid)?;
        let expected_revision = expected.expected_revision.get();
        let next_revision = expected_revision.checked_add(1).ok_or_else(invalid)?;
        if self.tenant_id != expected.scope.tenant_id.as_str()
            || self.installation_id != expected.scope.installation_id.as_str()
            || self.deployment_id != expected.scope.deployment_id.as_str()
            || self.attestation_id != expected.attestation_id.as_str()
            || self.process_instance_id != expected.process_instance_id.as_str()
            || runtime_generation != expected.runtime_generation.get()
            || lease_epoch != expected.lease_epoch.get()
            || !canonical_snowflake(&self.guild_id)
            || !canonical_ruleset_key(&self.ruleset_key)
            || self.acquired_at > self.last_heartbeat_at
            || self.last_heartbeat_at > self.expires_at
        {
            return Err(invalid());
        }
        match kind {
            RuntimeServingMutationKindV1::Heartbeat => {
                if !self.connected
                    || !self.serving
                    || self.last_heartbeat_at >= self.expires_at
                    || revision.get() != next_revision
                {
                    return Err(invalid());
                }
            }
            RuntimeServingMutationKindV1::Disconnect => {
                if self.connected
                    || self.serving
                    || self.last_heartbeat_at != self.expires_at
                    || revision.get() != next_revision
                {
                    return Err(invalid());
                }
            }
        }
        Ok(RuntimeServingReceiptV1 {
            identity: RuntimeServingIdentityV1 {
                scope: expected.scope.clone(),
                attestation_id: expected.attestation_id.clone(),
                process_instance_id: expected.process_instance_id.clone(),
                runtime_generation: expected.runtime_generation,
                lease_epoch: expected.lease_epoch,
                expected_revision: revision,
            },
            runtime_generation: expected.runtime_generation,
            acquired_at: self.acquired_at,
            last_heartbeat_at: self.last_heartbeat_at,
            expires_at: self.expires_at,
            connected: self.connected,
            serving: self.serving,
        })
    }
}

fn positive_i64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|value| *value != 0)
}

fn canonical_snowflake(value: &str) -> bool {
    value
        .parse::<u64>()
        .ok()
        .is_some_and(|parsed| parsed != 0 && parsed.to_string() == value)
}

fn canonical_ruleset_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn heartbeat_replay_contract(
    expected_revision: u64,
    observed_revision: i64,
    acquired_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    expected_duration: chrono::Duration,
) -> bool {
    let Some(next_revision) = expected_revision.checked_add(1) else {
        return false;
    };
    let Ok(observed_revision) = u64::try_from(observed_revision) else {
        return false;
    };
    observed_revision == next_revision
        && last_heartbeat_at >= acquired_at
        && expires_at.signed_duration_since(last_heartbeat_at) == expected_duration
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_slot_identifiers_are_canonical() {
        for valid in ["1", "42", "18446744073709551615"] {
            assert!(canonical_snowflake(valid));
        }
        for invalid in ["", "0", "01", "+1", " 1", "18446744073709551616"] {
            assert!(!canonical_snowflake(invalid));
        }
        for valid in ["studyroom", "study-room", "StudyRoom_1", &"a".repeat(64)] {
            assert!(canonical_ruleset_key(valid));
        }
        for invalid in ["", "bad key", "bad/key", &"a".repeat(65)] {
            assert!(!canonical_ruleset_key(invalid));
        }
    }

    #[test]
    fn heartbeat_replay_requires_the_exact_one_step_duration_outcome() {
        let acquired_at = DateTime::from_timestamp(1_000, 0).unwrap();
        let heartbeat_at = DateTime::from_timestamp(1_010, 0).unwrap();
        let expires_at = DateTime::from_timestamp(1_070, 0).unwrap();
        let duration = chrono::Duration::seconds(60);
        assert!(heartbeat_replay_contract(
            4,
            5,
            acquired_at,
            heartbeat_at,
            expires_at,
            duration
        ));
        assert!(heartbeat_replay_contract(
            4,
            5,
            acquired_at,
            acquired_at,
            DateTime::from_timestamp(1_060, 0).unwrap(),
            duration
        ));
        assert!(heartbeat_replay_contract(
            4,
            5,
            acquired_at,
            heartbeat_at,
            expires_at,
            duration
        ));
        for (revision, heartbeat, expiry) in [
            (4, heartbeat_at, expires_at),
            (6, heartbeat_at, expires_at),
            (5, acquired_at, expires_at),
            (5, heartbeat_at, DateTime::from_timestamp(1_069, 0).unwrap()),
        ] {
            assert!(!heartbeat_replay_contract(
                4,
                revision,
                acquired_at,
                heartbeat,
                expiry,
                duration
            ));
        }
    }
}
