use std::time::Duration;

use automation_instance::{InstanceId, InstanceStore, InstanceStoreError};
use automation_instance_teardown::{InstanceTeardownService, TeardownError, TeardownOutcome};
use discord_model::GuildId;
use futures::{stream, StreamExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumeConfig {
    pub max_concurrency: usize,
    pub per_instance_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumeEntry {
    Completed {
        instance_id: InstanceId,
        outcome: TeardownOutcome,
    },
    Failed {
        instance_id: InstanceId,
        error: TeardownError,
    },
    TimedOut {
        instance_id: InstanceId,
    },
}

impl ResumeEntry {
    fn instance_id(&self) -> &InstanceId {
        match self {
            Self::Completed { instance_id, .. }
            | Self::Failed { instance_id, .. }
            | Self::TimedOut { instance_id } => instance_id,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResumeReport {
    pub entries: Vec<ResumeEntry>,
}

pub async fn resume_deleting_instances<S, T>(
    guild_id: GuildId,
    store: &S,
    teardown: &T,
    config: ResumeConfig,
) -> Result<ResumeReport, InstanceStoreError>
where
    S: InstanceStore,
    T: InstanceTeardownService,
{
    let pending = store.list_deleting(guild_id).await?;
    let timeout = config.per_instance_timeout;
    let mut entries = stream::iter(pending.into_iter().map(|instance| {
        let instance_id = instance.id;
        async move {
            match tokio::time::timeout(timeout, teardown.teardown(guild_id, instance_id.clone()))
                .await
            {
                Ok(Ok(outcome)) => ResumeEntry::Completed {
                    instance_id,
                    outcome,
                },
                Ok(Err(error)) => ResumeEntry::Failed { instance_id, error },
                Err(_) => ResumeEntry::TimedOut { instance_id },
            }
        }
    }))
    .buffer_unordered(config.max_concurrency.max(1))
    .collect::<Vec<_>>()
    .await;
    entries.sort_by(|left, right| left.instance_id().cmp(right.instance_id()));
    Ok(ResumeReport { entries })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use automation_instance::{
        AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
        InstanceRuleSetVersion, InstanceStatus, InstanceStore,
    };
    use automation_instance_teardown::{InstanceTeardownService, TeardownError, TeardownOutcome};
    use discord_model::{GuildId, UserId};

    use super::{resume_deleting_instances, ResumeConfig, ResumeEntry};

    const GUILD: GuildId = GuildId(7);

    fn instance(id: &str) -> AutomationInstance {
        AutomationInstance {
            id: InstanceId::parse(id).unwrap(),
            guild_id: GUILD,
            ruleset_key: "studyroom_demo".to_string(),
            ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
            kind: InstanceKind("study_room".to_string()),
            created_by: UserId(42),
            resources: InstanceResources {
                roles: BTreeMap::new(),
                channels: BTreeMap::new(),
                messages: BTreeMap::new(),
            },
            status: InstanceStatus::Active,
        }
    }

    struct SweepService {
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    impl SweepService {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
            }
        }

        fn max_active(&self) -> usize {
            self.max_active.load(Ordering::SeqCst)
        }
    }

    impl InstanceTeardownService for SweepService {
        async fn teardown(
            &self,
            _: GuildId,
            instance_id: InstanceId,
        ) -> Result<TeardownOutcome, TeardownError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            let result = match instance_id.as_str() {
                "a" => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    Ok(TeardownOutcome::ResumedAndCompleted)
                }
                "b" => Err(TeardownError::InstanceNotFound),
                "c" => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(TeardownOutcome::ResumedAndCompleted)
                }
                _ => unreachable!(),
            };
            self.active.fetch_sub(1, Ordering::SeqCst);
            result
        }
    }

    #[tokio::test]
    async fn sweep_is_bounded_times_out_and_collects_failures() {
        let store = InMemoryInstanceStore::new();
        for id in ["c", "a", "b"] {
            let parsed = InstanceId::parse(id).unwrap();
            store.register(instance(id)).await.unwrap();
            store.transition_to_deleting(GUILD, &parsed).await.unwrap();
        }
        let service = Arc::new(SweepService::new());

        let report = resume_deleting_instances(
            GUILD,
            &store,
            service.as_ref(),
            ResumeConfig {
                max_concurrency: 2,
                per_instance_timeout: Duration::from_millis(20),
            },
        )
        .await
        .unwrap();

        assert!(service.max_active() <= 2);
        assert_eq!(report.entries.len(), 3);
        assert!(matches!(
            &report.entries[0],
            ResumeEntry::Completed { instance_id, .. } if instance_id.as_str() == "a"
        ));
        assert!(matches!(
            &report.entries[1],
            ResumeEntry::Failed { instance_id, .. } if instance_id.as_str() == "b"
        ));
        assert!(matches!(
            &report.entries[2],
            ResumeEntry::TimedOut { instance_id } if instance_id.as_str() == "c"
        ));
    }
}
