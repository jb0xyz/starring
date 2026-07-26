use std::sync::Mutex;

use automation_panel_installation::strict::{
    reconcile_declared_panels_strict, StrictPanelInstallationStore, StrictPanelInstaller,
    StrictPanelOperationJournal, StrictPanelReconcileErrorV1, StrictPanelReconcileRequestV1,
    StrictPanelReportV1,
};
use automation_panel_installation::{
    FencedStrictPanelInstallerV1, StrictPanelExternalCallFence,
    StrictPanelExternalCallFenceErrorV1, StrictPanelExternalCallV1,
};

use crate::{
    PostgresFencedStrictPanelStoreV1, RuntimePanelErrorClassV1, RuntimePanelLatchedErrorV1,
    RuntimePanelPersistenceErrorV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimePanelReconciliationOutcomeV1 {
    Eligible(StrictPanelReportV1),
    Ineligible(StrictPanelReportV1),
}

impl RuntimePanelReconciliationOutcomeV1 {
    pub fn report(&self) -> &StrictPanelReportV1 {
        match self {
            Self::Eligible(report) | Self::Ineligible(report) => report,
        }
    }

    pub fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePanelReconciliationErrorV1 {
    #[error(transparent)]
    Persistence(#[from] RuntimePanelPersistenceErrorV1),
    #[error("strict panel reconciliation failed")]
    Strict(StrictPanelReconcileErrorV1),
}

impl RuntimePanelReconciliationErrorV1 {
    pub fn persistence_class(&self) -> Option<RuntimePanelErrorClassV1> {
        match self {
            Self::Persistence(error) => Some(error.class()),
            Self::Strict(_) => None,
        }
    }
}

pub struct PostgresRuntimePanelReconciliationV1 {
    store: PostgresFencedStrictPanelStoreV1,
}

impl PostgresRuntimePanelReconciliationV1 {
    pub fn new(store: PostgresFencedStrictPanelStoreV1) -> Self {
        Self { store }
    }

    pub async fn run<I>(
        self,
        request: StrictPanelReconcileRequestV1<'_>,
        installer: &I,
    ) -> Result<RuntimePanelReconciliationOutcomeV1, RuntimePanelReconciliationErrorV1>
    where
        I: StrictPanelInstaller,
    {
        run_one_shot(self.store, request, installer).await
    }
}

#[allow(async_fn_in_trait)]
trait OneShotPanelStoreV1:
    StrictPanelInstallationStore + StrictPanelOperationJournal + StrictPanelExternalCallFence
{
    async fn prime_for_reconciliation(&self) -> Result<(), RuntimePanelPersistenceErrorV1>;

    async fn terminal_latch(&self) -> Option<RuntimePanelLatchedErrorV1>;
}

impl OneShotPanelStoreV1 for PostgresFencedStrictPanelStoreV1 {
    async fn prime_for_reconciliation(&self) -> Result<(), RuntimePanelPersistenceErrorV1> {
        self.prime().await
    }

    async fn terminal_latch(&self) -> Option<RuntimePanelLatchedErrorV1> {
        self.latched_error().await
    }
}

struct RecordingFenceV1<'a, F> {
    fence: &'a F,
    first_error: Mutex<Option<RuntimePanelPersistenceErrorV1>>,
}

impl<'a, F> RecordingFenceV1<'a, F> {
    fn new(fence: &'a F) -> Self {
        Self {
            fence,
            first_error: Mutex::new(None),
        }
    }

    fn first_error(&self) -> Option<RuntimePanelPersistenceErrorV1> {
        self.first_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl<F> StrictPanelExternalCallFence for RecordingFenceV1<'_, F>
where
    F: StrictPanelExternalCallFence,
{
    async fn check_external_call(
        &self,
        call: StrictPanelExternalCallV1,
    ) -> Result<(), StrictPanelExternalCallFenceErrorV1> {
        match self.fence.check_external_call(call).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let persistence = persistence_error_from_code(error.code())
                    .unwrap_or(RuntimePanelPersistenceErrorV1::Indeterminate);
                let mut first_error = self
                    .first_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if first_error.is_none() {
                    first_error.replace(persistence);
                }
                Err(error)
            }
        }
    }
}

async fn run_one_shot<S, I>(
    store: S,
    request: StrictPanelReconcileRequestV1<'_>,
    installer: &I,
) -> Result<RuntimePanelReconciliationOutcomeV1, RuntimePanelReconciliationErrorV1>
where
    S: OneShotPanelStoreV1,
    I: StrictPanelInstaller,
{
    store.prime_for_reconciliation().await?;
    let recording_fence = RecordingFenceV1::new(&store);
    let fenced_installer = FencedStrictPanelInstallerV1::new(&recording_fence, installer);
    let reconciliation =
        reconcile_declared_panels_strict(request, &store, &fenced_installer, &store).await;
    let terminal_latch = store.terminal_latch().await;
    let external_error = recording_fence.first_error();
    if let Some(latch) = terminal_latch {
        return Err(latched_persistence_error(latch).into());
    }
    if let Some(error) = external_error {
        return Err(error.into());
    }
    match reconciliation {
        Ok(report) if report.is_eligible() => {
            Ok(RuntimePanelReconciliationOutcomeV1::Eligible(report))
        }
        Ok(report) => Ok(RuntimePanelReconciliationOutcomeV1::Ineligible(report)),
        Err(error) => match persistence_error_from_reconcile(&error) {
            Some(persistence) => Err(persistence.into()),
            None => Err(RuntimePanelReconciliationErrorV1::Strict(error)),
        },
    }
}

fn latched_persistence_error(latch: RuntimePanelLatchedErrorV1) -> RuntimePanelPersistenceErrorV1 {
    match latch {
        RuntimePanelLatchedErrorV1::OwnershipLost => RuntimePanelPersistenceErrorV1::OwnershipLost,
        RuntimePanelLatchedErrorV1::AuthorityChanged => {
            RuntimePanelPersistenceErrorV1::AuthorityChanged
        }
        RuntimePanelLatchedErrorV1::Conflict => RuntimePanelPersistenceErrorV1::Conflict,
        RuntimePanelLatchedErrorV1::Indeterminate => RuntimePanelPersistenceErrorV1::Indeterminate,
    }
}

fn persistence_error_from_reconcile(
    error: &StrictPanelReconcileErrorV1,
) -> Option<RuntimePanelPersistenceErrorV1> {
    match error {
        StrictPanelReconcileErrorV1::Store(
            automation_panel_installation::PanelInstallationStoreError::Backend(code),
        ) => persistence_error_from_code(code),
        StrictPanelReconcileErrorV1::Journal(
            automation_panel_installation::strict::StrictPanelJournalError(code),
        ) => persistence_error_from_code(code),
        _ => None,
    }
}

fn persistence_error_from_code(code: &str) -> Option<RuntimePanelPersistenceErrorV1> {
    match code {
        "runtime_panel_invalid_authority" => Some(RuntimePanelPersistenceErrorV1::InvalidAuthority),
        "runtime_panel_invalid_duration" => Some(RuntimePanelPersistenceErrorV1::InvalidDuration),
        "runtime_panel_randomness_unavailable" => {
            Some(RuntimePanelPersistenceErrorV1::RandomnessUnavailable)
        }
        "runtime_panel_ownership_lost" => Some(RuntimePanelPersistenceErrorV1::OwnershipLost),
        "runtime_panel_authority_changed" => Some(RuntimePanelPersistenceErrorV1::AuthorityChanged),
        "runtime_panel_conflict" => Some(RuntimePanelPersistenceErrorV1::Conflict),
        "runtime_panel_capacity" => Some(RuntimePanelPersistenceErrorV1::Capacity),
        "runtime_panel_persistence_corrupt" => {
            Some(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        }
        "runtime_panel_timeout" => Some(RuntimePanelPersistenceErrorV1::Timeout),
        "runtime_panel_unavailable" => Some(RuntimePanelPersistenceErrorV1::Unavailable),
        "runtime_panel_indeterminate" => Some(RuntimePanelPersistenceErrorV1::Indeterminate),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use automation_panel_installation::strict::{
        StrictDeclaredPanelV1, StrictDeleteOutcomeV1, StrictExternalPostResultV1,
        StrictObservedMessageV1, StrictPanelJournalError, StrictPanelOperationKeyV1,
        StrictPanelOperationV1,
    };
    use automation_panel_installation::{
        InstallerError, PanelInstallation, PanelInstallationKey, PanelInstallationStore,
        PanelInstallationStoreError,
    };
    use automation_ruleset::{RuleSetKey, RuleSetVersionId};
    use discord_model::{ChannelId, GuildId, MessageId};
    use resource_resolution::ResourceBindingMap;

    use super::*;

    const GUILD: GuildId = GuildId(7);

    struct FakeStoreV1 {
        prime_error: Option<RuntimePanelPersistenceErrorV1>,
        latch: Option<RuntimePanelLatchedErrorV1>,
        fence_error: Option<RuntimePanelPersistenceErrorV1>,
        prime_count: Arc<AtomicUsize>,
        installations: Mutex<BTreeMap<String, PanelInstallation>>,
        journal: Mutex<BTreeMap<String, StrictPanelOperationV1>>,
    }

    impl FakeStoreV1 {
        fn new() -> Self {
            Self {
                prime_error: None,
                latch: None,
                fence_error: None,
                prime_count: Arc::new(AtomicUsize::new(0)),
                installations: Mutex::new(BTreeMap::new()),
                journal: Mutex::new(BTreeMap::new()),
            }
        }

        fn with_installation(self, installation: PanelInstallation) -> Self {
            self.installations
                .lock()
                .unwrap()
                .insert(installation.panel_key.clone(), installation);
            self
        }

        fn with_prime_error(mut self, error: RuntimePanelPersistenceErrorV1) -> Self {
            self.prime_error = Some(error);
            self
        }

        fn with_fence_error(mut self, error: RuntimePanelPersistenceErrorV1) -> Self {
            self.fence_error = Some(error);
            self
        }

        fn with_latch(mut self, latch: RuntimePanelLatchedErrorV1) -> Self {
            self.latch = Some(latch);
            self
        }

        fn prime_counter(&self) -> Arc<AtomicUsize> {
            Arc::clone(&self.prime_count)
        }
    }

    impl PanelInstallationStore for FakeStoreV1 {
        async fn get(
            &self,
            key: &PanelInstallationKey,
        ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError> {
            Ok(self
                .installations
                .lock()
                .unwrap()
                .get(&key.panel_key)
                .cloned())
        }

        async fn upsert(
            &self,
            installation: PanelInstallation,
        ) -> Result<(), PanelInstallationStoreError> {
            self.installations
                .lock()
                .unwrap()
                .insert(installation.panel_key.clone(), installation);
            Ok(())
        }
    }

    impl StrictPanelInstallationStore for FakeStoreV1 {
        async fn list_slot(
            &self,
            guild_id: GuildId,
            ruleset_key: &RuleSetKey,
        ) -> Result<Vec<PanelInstallation>, PanelInstallationStoreError> {
            Ok(self
                .installations
                .lock()
                .unwrap()
                .values()
                .filter(|installation| {
                    installation.guild_id == guild_id && &installation.ruleset_key == ruleset_key
                })
                .cloned()
                .collect())
        }

        async fn remove(
            &self,
            key: &PanelInstallationKey,
        ) -> Result<(), PanelInstallationStoreError> {
            self.installations.lock().unwrap().remove(&key.panel_key);
            Ok(())
        }
    }

    impl StrictPanelOperationJournal for FakeStoreV1 {
        async fn list_slot(
            &self,
            guild_id: GuildId,
            ruleset_key: &RuleSetKey,
        ) -> Result<Vec<StrictPanelOperationV1>, StrictPanelJournalError> {
            Ok(self
                .journal
                .lock()
                .unwrap()
                .values()
                .filter(|operation| {
                    operation.key.guild_id == guild_id && &operation.key.ruleset_key == ruleset_key
                })
                .cloned()
                .collect())
        }

        async fn put(
            &self,
            operation: StrictPanelOperationV1,
        ) -> Result<(), StrictPanelJournalError> {
            self.journal
                .lock()
                .unwrap()
                .insert(operation.key.panel_key.clone(), operation);
            Ok(())
        }

        async fn remove(
            &self,
            key: &StrictPanelOperationKeyV1,
        ) -> Result<(), StrictPanelJournalError> {
            self.journal.lock().unwrap().remove(&key.panel_key);
            Ok(())
        }
    }

    impl StrictPanelExternalCallFence for FakeStoreV1 {
        async fn check_external_call(
            &self,
            _call: StrictPanelExternalCallV1,
        ) -> Result<(), StrictPanelExternalCallFenceErrorV1> {
            match &self.fence_error {
                Some(error) => Err(StrictPanelExternalCallFenceErrorV1::new(
                    crate::error::stable_error_code(error),
                )),
                None => Ok(()),
            }
        }
    }

    impl OneShotPanelStoreV1 for FakeStoreV1 {
        async fn prime_for_reconciliation(&self) -> Result<(), RuntimePanelPersistenceErrorV1> {
            self.prime_count.fetch_add(1, Ordering::SeqCst);
            match &self.prime_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        async fn terminal_latch(&self) -> Option<RuntimePanelLatchedErrorV1> {
            self.latch
        }
    }

    #[derive(Default)]
    struct FakeInstallerV1 {
        calls: AtomicUsize,
    }

    impl StrictPanelInstaller for FakeInstallerV1 {
        async fn observe_message(
            &self,
            _channel_id: ChannelId,
            _message_id: MessageId,
        ) -> Result<StrictObservedMessageV1, InstallerError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(StrictObservedMessageV1::Missing)
        }

        async fn post_message(
            &self,
            _channel_id: ChannelId,
            _guild_id: GuildId,
            _ruleset_key: &str,
            _panel: &StrictDeclaredPanelV1,
        ) -> StrictExternalPostResultV1 {
            self.calls.fetch_add(1, Ordering::SeqCst);
            StrictExternalPostResultV1::DefinitelyNotApplied
        }

        async fn delete_message(
            &self,
            _channel_id: ChannelId,
            _message_id: MessageId,
        ) -> StrictDeleteOutcomeV1 {
            self.calls.fetch_add(1, Ordering::SeqCst);
            StrictDeleteOutcomeV1::Deleted
        }
    }

    fn ruleset_key() -> RuleSetKey {
        RuleSetKey::parse("studyroom").unwrap()
    }

    fn installation() -> PanelInstallation {
        PanelInstallation {
            guild_id: GUILD,
            ruleset_key: ruleset_key(),
            panel_key: "entry".to_string(),
            installed_version: RuleSetVersionId::FIRST,
            channel_id: ChannelId(10),
            message_id: MessageId(100),
            spec_hash: "a".repeat(64),
        }
    }

    async fn run_fake(
        store: FakeStoreV1,
        installer: &FakeInstallerV1,
    ) -> Result<RuntimePanelReconciliationOutcomeV1, RuntimePanelReconciliationErrorV1> {
        let key = ruleset_key();
        let bindings = ResourceBindingMap::default();
        run_one_shot(
            store,
            StrictPanelReconcileRequestV1 {
                guild_id: GUILD,
                ruleset_key: &key,
                ruleset_version: RuleSetVersionId::FIRST,
                render_revision: 1,
                panels: &[],
                bindings: &bindings,
            },
            installer,
        )
        .await
    }

    #[tokio::test]
    async fn empty_reconciliation_is_eligible_and_primes_once() {
        let installer = FakeInstallerV1::default();
        let store = FakeStoreV1::new();
        let prime_count = store.prime_counter();
        let outcome = run_fake(store, &installer).await.unwrap();
        assert!(outcome.is_eligible());
        assert_eq!(outcome.report().declared_count, 0);
        assert_eq!(prime_count.load(Ordering::SeqCst), 1);
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejected_fence_preserves_class_and_never_calls_installer() {
        let installer = FakeInstallerV1::default();
        let error = run_fake(
            FakeStoreV1::new()
                .with_installation(installation())
                .with_fence_error(RuntimePanelPersistenceErrorV1::OwnershipLost),
            &installer,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            RuntimePanelReconciliationErrorV1::Persistence(
                RuntimePanelPersistenceErrorV1::OwnershipLost
            )
        );
        assert_eq!(
            error.persistence_class(),
            Some(RuntimePanelErrorClassV1::OwnershipLost)
        );
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn prime_failure_preserves_class_and_never_calls_installer() {
        let installer = FakeInstallerV1::default();
        let error = run_fake(
            FakeStoreV1::new().with_prime_error(RuntimePanelPersistenceErrorV1::Unavailable),
            &installer,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            RuntimePanelReconciliationErrorV1::Persistence(
                RuntimePanelPersistenceErrorV1::Unavailable
            )
        );
        assert_eq!(
            error.persistence_class(),
            Some(RuntimePanelErrorClassV1::Unavailable)
        );
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn terminal_latch_overrides_an_otherwise_eligible_report() {
        let installer = FakeInstallerV1::default();
        let error = run_fake(
            FakeStoreV1::new().with_latch(RuntimePanelLatchedErrorV1::AuthorityChanged),
            &installer,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            RuntimePanelReconciliationErrorV1::Persistence(
                RuntimePanelPersistenceErrorV1::AuthorityChanged
            )
        );
        assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
    }
}
