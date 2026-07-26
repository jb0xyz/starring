use std::collections::BTreeSet;

use automation_panel_installation::strict::{
    validate_strict_panel_key_v1, validate_strict_panel_operation_v1, StrictPanelInstallationStore,
    StrictPanelJournalError, StrictPanelOperationJournal, StrictPanelOperationKeyV1,
    StrictPanelOperationV1, MAX_STRICT_PANEL_RECORDS_PER_SLOT,
};
use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, PanelInstallationStore, PanelInstallationStoreError,
};
use automation_ruleset::RuleSetKey;
use discord_model::GuildId;
use sqlx::types::Json;

use crate::authority::bind_runtime_panel_authority;
use crate::contract::{
    INSTALLATION_REMOVE_QUERY, INSTALLATION_UPSERT_QUERY, JOURNAL_PUT_QUERY, JOURNAL_REMOVE_QUERY,
};
use crate::error::{map_mutation_commit_error, map_mutation_error, stable_error_code};
use crate::row::{state_tag, valid_hash, RECORD_FORMAT_VERSION};
use crate::session::{PostgresFencedStrictPanelStoreV1, RuntimePanelStoreStateV1, VersionedV1};
use crate::RuntimePanelPersistenceErrorV1;

impl PostgresFencedStrictPanelStoreV1 {
    async fn installation_get_typed(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if let Err(error) = self.validate_installation_key(key) {
            state.record_error(&error);
            return Err(error);
        }
        let value = state
            .installations
            .get(&key.panel_key)
            .map(|record| record.value.clone());
        state.record_success();
        Ok(value)
    }

    async fn installation_list_typed(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<PanelInstallation>, RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if guild_id != self.authority.guild_id || ruleset_key != &self.authority.ruleset_key {
            let error = RuntimePanelPersistenceErrorV1::InvalidAuthority;
            state.record_error(&error);
            return Err(error);
        }
        let values = state
            .installations
            .values()
            .map(|record| record.value.clone())
            .collect();
        state.record_success();
        Ok(values)
    }

    async fn installation_upsert_typed(
        &self,
        installation: PanelInstallation,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if let Err(error) = self.validate_installation(&installation) {
            state.record_error(&error);
            return Err(error);
        }
        if let Err(error) = ensure_union_capacity(
            &state,
            &installation.panel_key,
            RecordCollectionV1::Installation,
        ) {
            state.record_error(&error);
            return Err(error);
        }
        let expected_record_revision = state
            .installations
            .get(&installation.panel_key)
            .map_or(0, |record| record.revision);
        let expected_journal_record_revision = state
            .journal
            .get(&installation.panel_key)
            .map_or(0, |record| record.revision);
        let mut transaction = match self.begin_transaction().await {
            Ok(transaction) => transaction,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        let query = bind_runtime_panel_authority!(
            sqlx::query_scalar::<_, i64>(INSTALLATION_UPSERT_QUERY),
            &self.authority,
            &self.receipt.session_id
        )
        .bind(self.receipt.session_record_revision.get() as i64)
        .bind(expected_record_revision as i64)
        .bind(&installation.panel_key)
        .bind(i64::from(installation.installed_version.get()))
        .bind(installation.channel_id.to_string())
        .bind(installation.message_id.to_string())
        .bind(&installation.spec_hash)
        .bind(expected_journal_record_revision as i64);
        let revision = match fetch_mutation_revision(
            query.fetch_all(&mut *transaction).await,
            expected_record_revision,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        if let Err(database) = transaction.commit().await {
            let error = map_mutation_commit_error(&database);
            state.record_error(&error);
            return Err(error);
        }
        state.installations.insert(
            installation.panel_key.clone(),
            VersionedV1 {
                revision,
                value: installation,
            },
        );
        state.record_success();
        Ok(())
    }

    async fn installation_remove_typed(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if let Err(error) = self.validate_installation_key(key) {
            state.record_error(&error);
            return Err(error);
        }
        let expected_record_revision = state
            .installations
            .get(&key.panel_key)
            .map_or(0, |record| record.revision);
        let mut transaction = match self.begin_transaction().await {
            Ok(transaction) => transaction,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        let query = bind_runtime_panel_authority!(
            sqlx::query_scalar::<_, i64>(INSTALLATION_REMOVE_QUERY),
            &self.authority,
            &self.receipt.session_id
        )
        .bind(self.receipt.session_record_revision.get() as i64)
        .bind(expected_record_revision as i64)
        .bind(&key.panel_key);
        let _revision = match fetch_mutation_revision(
            query.fetch_all(&mut *transaction).await,
            expected_record_revision,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        if let Err(database) = transaction.commit().await {
            let error = map_mutation_commit_error(&database);
            state.record_error(&error);
            return Err(error);
        }
        state.installations.remove(&key.panel_key);
        state.record_success();
        Ok(())
    }

    async fn journal_list_typed(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<StrictPanelOperationV1>, RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if guild_id != self.authority.guild_id || ruleset_key != &self.authority.ruleset_key {
            let error = RuntimePanelPersistenceErrorV1::InvalidAuthority;
            state.record_error(&error);
            return Err(error);
        }
        let values = state
            .journal
            .values()
            .map(|record| record.value.clone())
            .collect();
        state.record_success();
        Ok(values)
    }

    async fn journal_put_typed(
        &self,
        operation: StrictPanelOperationV1,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if let Err(error) = self.validate_operation(&operation) {
            state.record_error(&error);
            return Err(error);
        }
        if let Err(error) = ensure_union_capacity(
            &state,
            &operation.key.panel_key,
            RecordCollectionV1::Journal,
        ) {
            state.record_error(&error);
            return Err(error);
        }
        let expected_record_revision = state
            .journal
            .get(&operation.key.panel_key)
            .map_or(0, |record| record.revision);
        let mut transaction = match self.begin_transaction().await {
            Ok(transaction) => transaction,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        let query = bind_runtime_panel_authority!(
            sqlx::query_scalar::<_, i64>(JOURNAL_PUT_QUERY),
            &self.authority,
            &self.receipt.session_id
        )
        .bind(self.receipt.session_record_revision.get() as i64)
        .bind(expected_record_revision as i64)
        .bind(RECORD_FORMAT_VERSION)
        .bind(&operation.key.panel_key)
        .bind(state_tag(&operation.state))
        .bind(Json(&operation));
        let revision = match fetch_mutation_revision(
            query.fetch_all(&mut *transaction).await,
            expected_record_revision,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        if let Err(database) = transaction.commit().await {
            let error = map_mutation_commit_error(&database);
            state.record_error(&error);
            return Err(error);
        }
        state.journal.insert(
            operation.key.panel_key.clone(),
            VersionedV1 {
                revision,
                value: operation,
            },
        );
        state.record_success();
        Ok(())
    }

    async fn journal_remove_typed(
        &self,
        key: &StrictPanelOperationKeyV1,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        let mut state = self.state.lock().await;
        state.require_active()?;
        if let Err(error) = self.validate_operation_key(key) {
            state.record_error(&error);
            return Err(error);
        }
        let expected_record_revision = state
            .journal
            .get(&key.panel_key)
            .map_or(0, |record| record.revision);
        let mut transaction = match self.begin_transaction().await {
            Ok(transaction) => transaction,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        let query = bind_runtime_panel_authority!(
            sqlx::query_scalar::<_, i64>(JOURNAL_REMOVE_QUERY),
            &self.authority,
            &self.receipt.session_id
        )
        .bind(self.receipt.session_record_revision.get() as i64)
        .bind(expected_record_revision as i64)
        .bind(&key.panel_key);
        let _revision = match fetch_mutation_revision(
            query.fetch_all(&mut *transaction).await,
            expected_record_revision,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                state.record_error(&error);
                return Err(error);
            }
        };
        if let Err(database) = transaction.commit().await {
            let error = map_mutation_commit_error(&database);
            state.record_error(&error);
            return Err(error);
        }
        state.journal.remove(&key.panel_key);
        state.record_success();
        Ok(())
    }

    fn validate_installation(
        &self,
        installation: &PanelInstallation,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        self.validate_installation_key(&PanelInstallationKey {
            guild_id: installation.guild_id,
            ruleset_key: installation.ruleset_key.clone(),
            panel_key: installation.panel_key.clone(),
        })?;
        if installation.installed_version != self.authority.target_version
            || installation.channel_id.0 == 0
            || installation.message_id.0 == 0
            || !valid_hash(&installation.spec_hash)
        {
            return Err(RuntimePanelPersistenceErrorV1::InvalidAuthority);
        }
        Ok(())
    }

    fn validate_operation(
        &self,
        operation: &StrictPanelOperationV1,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        self.validate_operation_key(&operation.key)?;
        validate_strict_panel_operation_v1(operation)
            .map_err(|_| RuntimePanelPersistenceErrorV1::InvalidAuthority)
    }

    fn validate_operation_key(
        &self,
        key: &StrictPanelOperationKeyV1,
    ) -> Result<(), RuntimePanelPersistenceErrorV1> {
        if key.guild_id != self.authority.guild_id
            || key.ruleset_key != self.authority.ruleset_key
            || validate_strict_panel_key_v1(&key.panel_key).is_err()
        {
            return Err(RuntimePanelPersistenceErrorV1::InvalidAuthority);
        }
        Ok(())
    }
}

fn fetch_mutation_revision(
    result: Result<Vec<i64>, sqlx::Error>,
    expected_revision: u64,
) -> Result<u64, RuntimePanelPersistenceErrorV1> {
    let rows = result.map_err(|error| map_mutation_error(&error))?;
    let [revision] = rows.as_slice() else {
        return Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt);
    };
    let revision = u64::try_from(*revision)
        .ok()
        .filter(|revision| *revision > 0)
        .ok_or(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)?;
    if revision < expected_revision {
        return Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(revision)
}

#[derive(Clone, Copy)]
enum RecordCollectionV1 {
    Installation,
    Journal,
}

fn ensure_union_capacity(
    state: &RuntimePanelStoreStateV1,
    panel_key: &str,
    collection: RecordCollectionV1,
) -> Result<(), RuntimePanelPersistenceErrorV1> {
    let already_present = match collection {
        RecordCollectionV1::Installation => state.installations.contains_key(panel_key),
        RecordCollectionV1::Journal => state.journal.contains_key(panel_key),
    };
    if already_present {
        return Ok(());
    }
    let mut resident = state
        .installations
        .keys()
        .chain(state.journal.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    resident.insert(panel_key.to_string());
    if resident.len() > MAX_STRICT_PANEL_RECORDS_PER_SLOT {
        Err(RuntimePanelPersistenceErrorV1::Capacity)
    } else {
        Ok(())
    }
}

fn installation_error(error: RuntimePanelPersistenceErrorV1) -> PanelInstallationStoreError {
    PanelInstallationStoreError::Backend(stable_error_code(&error).to_string())
}

fn journal_error(error: RuntimePanelPersistenceErrorV1) -> StrictPanelJournalError {
    StrictPanelJournalError(stable_error_code(&error).to_string())
}

impl PanelInstallationStore for PostgresFencedStrictPanelStoreV1 {
    async fn get(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError> {
        self.installation_get_typed(key)
            .await
            .map_err(installation_error)
    }

    async fn upsert(
        &self,
        installation: PanelInstallation,
    ) -> Result<(), PanelInstallationStoreError> {
        self.installation_upsert_typed(installation)
            .await
            .map_err(installation_error)
    }
}

impl StrictPanelInstallationStore for PostgresFencedStrictPanelStoreV1 {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<PanelInstallation>, PanelInstallationStoreError> {
        self.installation_list_typed(guild_id, ruleset_key)
            .await
            .map_err(installation_error)
    }

    async fn remove(&self, key: &PanelInstallationKey) -> Result<(), PanelInstallationStoreError> {
        self.installation_remove_typed(key)
            .await
            .map_err(installation_error)
    }
}

impl StrictPanelOperationJournal for PostgresFencedStrictPanelStoreV1 {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<StrictPanelOperationV1>, StrictPanelJournalError> {
        self.journal_list_typed(guild_id, ruleset_key)
            .await
            .map_err(journal_error)
    }

    async fn put(&self, operation: StrictPanelOperationV1) -> Result<(), StrictPanelJournalError> {
        self.journal_put_typed(operation)
            .await
            .map_err(journal_error)
    }

    async fn remove(&self, key: &StrictPanelOperationKeyV1) -> Result<(), StrictPanelJournalError> {
        self.journal_remove_typed(key).await.map_err(journal_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_revision_accepts_exact_replay_and_advance() {
        assert_eq!(fetch_mutation_revision(Ok(vec![7]), 7), Ok(7));
        assert_eq!(fetch_mutation_revision(Ok(vec![9]), 7), Ok(9));
    }

    #[test]
    fn mutation_revision_rejects_stale_or_malformed_results() {
        for rows in [vec![], vec![0], vec![6], vec![7, 8]] {
            assert_eq!(
                fetch_mutation_revision(Ok(rows), 7),
                Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
            );
        }
    }
}
