use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;
use std::time::Duration;

use automation_panel_installation::strict::MAX_STRICT_PANEL_RECORDS_PER_SLOT;
use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, StrictPanelExternalCallFence,
    StrictPanelExternalCallFenceErrorV1, StrictPanelExternalCallV1,
};
use automation_runtime_controller::RuntimeExecutionGuardV1;
use automation_runtime_convergence_postgres::RuntimeExactTargetV1;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::authority::{bind_runtime_panel_authority, RuntimePanelAuthorityV1};
use crate::contract::{CHECK_QUERY, CLAIM_QUERY, SNAPSHOT_QUERY};
use crate::database::{begin_panel_transaction, RuntimePanelDatabaseTimeoutsV1};
use crate::error::{
    map_mutation_commit_error, map_mutation_error, map_query_error, stable_error_code,
    validate_millisecond_duration,
};
use crate::row::{DecodedSnapshotRecordV1, RuntimePanelSnapshotRowV1};
use crate::{RuntimePanelErrorClassV1, RuntimePanelLatchedErrorV1, RuntimePanelPersistenceErrorV1};

pub const MAX_RUNTIME_PANEL_LEASE_HEADROOM: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimePanelSessionIdV1(String);

impl RuntimePanelSessionIdV1 {
    pub fn generate() -> Result<Self, RuntimePanelPersistenceErrorV1> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|_| RuntimePanelPersistenceErrorV1::RandomnessUnavailable)?;
        let mut encoded = String::with_capacity(64);
        for byte in bytes {
            use std::fmt::Write;
            write!(&mut encoded, "{byte:02x}")
                .map_err(|_| RuntimePanelPersistenceErrorV1::RandomnessUnavailable)?;
        }
        Ok(Self(encoded))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for RuntimePanelSessionIdV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePanelSessionReceiptV1 {
    pub session_id: RuntimePanelSessionIdV1,
    pub session_record_revision: NonZeroU64,
    pub checked_at: DateTime<Utc>,
    pub controller_lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePanelSessionCheckV1 {
    pub checked_at: DateTime<Utc>,
    pub controller_lease_expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SessionRowV1 {
    session_record_revision: i64,
    checked_at: DateTime<Utc>,
    controller_lease_expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SessionCheckRowV1 {
    checked_at: DateTime<Utc>,
    controller_lease_expires_at: DateTime<Utc>,
}

pub(crate) struct VersionedV1<T> {
    pub(crate) revision: u64,
    pub(crate) value: T,
}

pub(crate) struct RuntimePanelStoreStateV1 {
    pub(crate) primed: bool,
    pub(crate) installations: BTreeMap<String, VersionedV1<PanelInstallation>>,
    pub(crate) journal: BTreeMap<
        String,
        VersionedV1<automation_panel_installation::strict::StrictPanelOperationV1>,
    >,
    pub(crate) latch: Option<RuntimePanelLatchedErrorV1>,
    pub(crate) last_error: Option<RuntimePanelErrorClassV1>,
}

impl RuntimePanelStoreStateV1 {
    fn unprimed() -> Self {
        Self {
            primed: false,
            installations: BTreeMap::new(),
            journal: BTreeMap::new(),
            latch: None,
            last_error: None,
        }
    }

    pub(crate) fn require_active(&self) -> Result<(), RuntimePanelPersistenceErrorV1> {
        match self.latch {
            Some(RuntimePanelLatchedErrorV1::OwnershipLost) => {
                Err(RuntimePanelPersistenceErrorV1::OwnershipLost)
            }
            Some(RuntimePanelLatchedErrorV1::AuthorityChanged) => {
                Err(RuntimePanelPersistenceErrorV1::AuthorityChanged)
            }
            Some(RuntimePanelLatchedErrorV1::Conflict) => {
                Err(RuntimePanelPersistenceErrorV1::Conflict)
            }
            Some(RuntimePanelLatchedErrorV1::Indeterminate) => {
                Err(RuntimePanelPersistenceErrorV1::Indeterminate)
            }
            None if self.primed => Ok(()),
            None => Err(RuntimePanelPersistenceErrorV1::InvalidAuthority),
        }
    }

    pub(crate) fn record_error(&mut self, error: &RuntimePanelPersistenceErrorV1) {
        self.last_error = Some(error.class());
        if self.latch.is_none() {
            self.latch = error.latch();
        }
    }

    pub(crate) fn record_success(&mut self) {
        self.last_error = None;
    }
}

pub struct PostgresFencedStrictPanelStoreV1 {
    pub(crate) pool: PgPool,
    pub(crate) authority: RuntimePanelAuthorityV1,
    pub(crate) receipt: RuntimePanelSessionReceiptV1,
    pub(crate) side_effect_headroom: Duration,
    pub(crate) database_timeouts: RuntimePanelDatabaseTimeoutsV1,
    pub(crate) state: Mutex<RuntimePanelStoreStateV1>,
}

impl PostgresFencedStrictPanelStoreV1 {
    pub async fn claim(
        pool: PgPool,
        guard: RuntimeExecutionGuardV1,
        exact_target: RuntimeExactTargetV1,
        side_effect_headroom: Duration,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        let session_id = RuntimePanelSessionIdV1::generate()?;
        Self::claim_with_session_id_and_timeouts(
            pool,
            guard,
            exact_target,
            side_effect_headroom,
            &session_id,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
    }

    pub async fn claim_with_timeouts(
        pool: PgPool,
        guard: RuntimeExecutionGuardV1,
        exact_target: RuntimeExactTargetV1,
        side_effect_headroom: Duration,
        database_timeouts: RuntimePanelDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        let session_id = RuntimePanelSessionIdV1::generate()?;
        Self::claim_with_session_id_and_timeouts(
            pool,
            guard,
            exact_target,
            side_effect_headroom,
            &session_id,
            database_timeouts,
        )
        .await
    }

    pub async fn claim_with_session_id(
        pool: PgPool,
        guard: RuntimeExecutionGuardV1,
        exact_target: RuntimeExactTargetV1,
        side_effect_headroom: Duration,
        session_id: &RuntimePanelSessionIdV1,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        Self::claim_with_session_id_and_timeouts(
            pool,
            guard,
            exact_target,
            side_effect_headroom,
            session_id,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
    }

    pub async fn claim_with_session_id_and_timeouts(
        pool: PgPool,
        guard: RuntimeExecutionGuardV1,
        exact_target: RuntimeExactTargetV1,
        side_effect_headroom: Duration,
        session_id: &RuntimePanelSessionIdV1,
        database_timeouts: RuntimePanelDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        validate_millisecond_duration(side_effect_headroom, MAX_RUNTIME_PANEL_LEASE_HEADROOM)?;
        let authority = RuntimePanelAuthorityV1::new(guard, exact_target)?;
        let mut transaction = begin_panel_transaction(&pool, database_timeouts).await?;
        let query = bind_runtime_panel_authority!(
            sqlx::query_as::<_, SessionRowV1>(CLAIM_QUERY),
            &authority,
            session_id
        );
        let rows = query
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let [row] = rows.as_slice() else {
            return Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt);
        };
        let session_record_revision = u64::try_from(row.session_record_revision)
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)?;
        if row.checked_at >= row.controller_lease_expires_at {
            return Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt);
        }
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(Self {
            pool,
            authority,
            receipt: RuntimePanelSessionReceiptV1 {
                session_id: session_id.clone(),
                session_record_revision,
                checked_at: row.checked_at,
                controller_lease_expires_at: row.controller_lease_expires_at,
            },
            side_effect_headroom,
            database_timeouts,
            state: Mutex::new(RuntimePanelStoreStateV1::unprimed()),
        })
    }

    pub fn receipt(&self) -> &RuntimePanelSessionReceiptV1 {
        &self.receipt
    }

    pub async fn prime(&self) -> Result<(), RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        if state.primed {
            return state.require_active();
        }
        if let Some(latch) = state.latch {
            return Err(latched_error(latch));
        }
        let mut transaction =
            match begin_panel_transaction(&self.pool, self.database_timeouts).await {
                Ok(transaction) => transaction,
                Err(error) => {
                    state.record_error(&error);
                    return Err(error);
                }
            };
        let query = bind_runtime_panel_authority!(
            sqlx::query_as::<_, RuntimePanelSnapshotRowV1>(SNAPSHOT_QUERY),
            &self.authority,
            &self.receipt.session_id
        )
        .bind(self.receipt.session_record_revision.get() as i64);
        let rows = match query.fetch_all(&mut *transaction).await {
            Ok(rows) => rows,
            Err(database) => {
                let error = map_query_error(&database);
                state.record_error(&error);
                return Err(error);
            }
        };
        if let Err(database) = transaction.commit().await {
            let error = map_query_error(&database);
            state.record_error(&error);
            return Err(error);
        }
        let mut installations = BTreeMap::new();
        let mut journal = BTreeMap::new();
        for row in rows {
            let decoded = match row.decode(self.authority.guild_id, &self.authority.ruleset_key) {
                Ok(decoded) => decoded,
                Err(error) => {
                    state.record_error(&error);
                    return Err(error);
                }
            };
            match decoded {
                DecodedSnapshotRecordV1::Installation { revision, value } => {
                    let panel_key = value.panel_key.clone();
                    if installations
                        .insert(panel_key, VersionedV1 { revision, value })
                        .is_some()
                    {
                        let error = RuntimePanelPersistenceErrorV1::PersistenceCorrupt;
                        state.record_error(&error);
                        return Err(error);
                    }
                }
                DecodedSnapshotRecordV1::Journal { revision, value } => {
                    let panel_key = value.key.panel_key.clone();
                    if journal
                        .insert(panel_key, VersionedV1 { revision, value })
                        .is_some()
                    {
                        let error = RuntimePanelPersistenceErrorV1::PersistenceCorrupt;
                        state.record_error(&error);
                        return Err(error);
                    }
                }
            }
        }
        let resident = installations
            .keys()
            .chain(journal.keys())
            .collect::<BTreeSet<_>>();
        if resident.len() > MAX_STRICT_PANEL_RECORDS_PER_SLOT {
            let error = RuntimePanelPersistenceErrorV1::PersistenceCorrupt;
            state.record_error(&error);
            return Err(error);
        }
        state.installations = installations;
        state.journal = journal;
        state.primed = true;
        state.record_success();
        Ok(())
    }

    pub async fn check_session(
        &self,
        required_lease_headroom: Duration,
    ) -> Result<RuntimePanelSessionCheckV1, RuntimePanelPersistenceErrorV1> {
        let required_lease_headroom_ms = validate_millisecond_duration(
            required_lease_headroom,
            MAX_RUNTIME_PANEL_LEASE_HEADROOM,
        )?;
        let mut state = self.state.lock().await;
        state.require_active()?;
        let mut transaction =
            match begin_panel_transaction(&self.pool, self.database_timeouts).await {
                Ok(transaction) => transaction,
                Err(error) => {
                    state.record_error(&error);
                    return Err(error);
                }
            };
        let query = bind_runtime_panel_authority!(
            sqlx::query_as::<_, SessionCheckRowV1>(CHECK_QUERY),
            &self.authority,
            &self.receipt.session_id
        )
        .bind(self.receipt.session_record_revision.get() as i64)
        .bind(required_lease_headroom_ms);
        let rows = match query.fetch_all(&mut *transaction).await {
            Ok(rows) => rows,
            Err(database) => {
                let error = map_query_error(&database);
                state.record_error(&error);
                return Err(error);
            }
        };
        let [row] = rows.as_slice() else {
            let error = RuntimePanelPersistenceErrorV1::PersistenceCorrupt;
            state.record_error(&error);
            return Err(error);
        };
        if row.checked_at >= row.controller_lease_expires_at
            || row.controller_lease_expires_at != self.receipt.controller_lease_expires_at
        {
            let error = RuntimePanelPersistenceErrorV1::PersistenceCorrupt;
            state.record_error(&error);
            return Err(error);
        }
        if let Err(database) = transaction.commit().await {
            let error = map_query_error(&database);
            state.record_error(&error);
            return Err(error);
        }
        state.record_success();
        Ok(RuntimePanelSessionCheckV1 {
            checked_at: row.checked_at,
            controller_lease_expires_at: row.controller_lease_expires_at,
        })
    }

    pub async fn latched_error(&self) -> Option<RuntimePanelLatchedErrorV1> {
        self.state.lock().await.latch
    }

    pub async fn last_error_class(&self) -> Option<RuntimePanelErrorClassV1> {
        self.state.lock().await.last_error
    }

    pub fn slot(&self) -> (discord_model::GuildId, &automation_ruleset::RuleSetKey) {
        (self.authority.guild_id, &self.authority.ruleset_key)
    }

    pub(crate) fn validate_installation_key(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        if key.guild_id != self.authority.guild_id
            || key.ruleset_key != self.authority.ruleset_key
            || automation_panel_installation::strict::validate_strict_panel_key_v1(&key.panel_key)
                .is_err()
        {
            return Err(RuntimePanelPersistenceErrorV1::InvalidAuthority);
        }
        Ok(())
    }
}

impl StrictPanelExternalCallFence for PostgresFencedStrictPanelStoreV1 {
    async fn check_external_call(
        &self,
        call: StrictPanelExternalCallV1,
    ) -> Result<(), StrictPanelExternalCallFenceErrorV1> {
        let headroom = match call {
            StrictPanelExternalCallV1::Observe => Duration::from_millis(1),
            StrictPanelExternalCallV1::Post | StrictPanelExternalCallV1::Delete => {
                self.side_effect_headroom
            }
        };
        self.check_session(headroom)
            .await
            .map(|_| ())
            .map_err(|error| StrictPanelExternalCallFenceErrorV1::new(stable_error_code(&error)))
    }
}

fn latched_error(latch: RuntimePanelLatchedErrorV1) -> RuntimePanelPersistenceErrorV1 {
    match latch {
        RuntimePanelLatchedErrorV1::OwnershipLost => RuntimePanelPersistenceErrorV1::OwnershipLost,
        RuntimePanelLatchedErrorV1::AuthorityChanged => {
            RuntimePanelPersistenceErrorV1::AuthorityChanged
        }
        RuntimePanelLatchedErrorV1::Conflict => RuntimePanelPersistenceErrorV1::Conflict,
        RuntimePanelLatchedErrorV1::Indeterminate => RuntimePanelPersistenceErrorV1::Indeterminate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_session_id_is_lower_hex_and_unique() {
        let first = RuntimePanelSessionIdV1::generate().unwrap();
        let second = RuntimePanelSessionIdV1::generate().unwrap();
        assert_ne!(first, second);
        for session in [first, second] {
            assert_eq!(session.as_str().len(), 64);
            assert!(session
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }
    }

    #[test]
    fn unprimed_and_latched_state_fail_closed() {
        let mut state = RuntimePanelStoreStateV1::unprimed();
        assert_eq!(
            state.require_active(),
            Err(RuntimePanelPersistenceErrorV1::InvalidAuthority)
        );
        state.primed = true;
        state.record_error(&RuntimePanelPersistenceErrorV1::Conflict);
        assert_eq!(
            state.require_active(),
            Err(RuntimePanelPersistenceErrorV1::Conflict)
        );
        state.record_success();
        assert_eq!(
            state.require_active(),
            Err(RuntimePanelPersistenceErrorV1::Conflict)
        );
        let mut authority = RuntimePanelStoreStateV1::unprimed();
        authority.primed = true;
        authority.record_error(&RuntimePanelPersistenceErrorV1::AuthorityChanged);
        assert_eq!(
            authority.require_active(),
            Err(RuntimePanelPersistenceErrorV1::AuthorityChanged)
        );
    }
}
