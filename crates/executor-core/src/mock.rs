use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId};

use crate::adapter::{AdapterError, ChannelSpec, DiscordAdapter, RoleSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterCall {
    CreateRole {
        guild: GuildId,
        spec: RoleSpec,
    },
    UpdateRole {
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    },
    DeleteRole {
        guild: GuildId,
        id: RoleId,
    },
    CreateChannel {
        guild: GuildId,
        spec: ChannelSpec,
    },
    UpdateChannel {
        guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    },
    DeleteChannel {
        guild: GuildId,
        id: ChannelId,
    },
    UpsertOverwrite {
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
}

pub struct MockDiscordAdapter {
    next_id: AtomicU64,
    calls: Mutex<Vec<AdapterCall>>,
    fail_on: Vec<(usize, AdapterError)>,
}

impl MockDiscordAdapter {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(900_000),
            calls: Mutex::new(Vec::new()),
            fail_on: Vec::new(),
        }
    }

    pub fn with_failure(call_number: usize, error: AdapterError) -> Self {
        Self {
            next_id: AtomicU64::new(900_000),
            calls: Mutex::new(Vec::new()),
            fail_on: vec![(call_number, error)],
        }
    }

    pub fn with_failures(failures: Vec<(usize, AdapterError)>) -> Self {
        Self {
            next_id: AtomicU64::new(900_000),
            calls: Mutex::new(Vec::new()),
            fail_on: failures,
        }
    }

    pub fn calls(&self) -> Vec<AdapterCall> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: AdapterCall) -> usize {
        let mut calls = self.calls.lock().unwrap();
        calls.push(call);
        calls.len()
    }

    fn check_fail(&self, call_number: usize) -> Result<(), AdapterError> {
        for (n, err) in &self.fail_on {
            if *n == call_number {
                return Err(err.clone());
            }
        }
        Ok(())
    }

    fn next_role(&self) -> RoleId {
        RoleId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    fn next_channel(&self) -> ChannelId {
        ChannelId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for MockDiscordAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordAdapter for MockDiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError> {
        let n = self.record(AdapterCall::CreateRole { guild, spec });
        self.check_fail(n)?;
        Ok(self.next_role())
    }

    async fn update_role(
        &self,
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    ) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::UpdateRole { guild, id, spec });
        self.check_fail(n)
    }

    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::DeleteRole { guild, id });
        self.check_fail(n)
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: ChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        let n = self.record(AdapterCall::CreateChannel { guild, spec });
        self.check_fail(n)?;
        Ok(self.next_channel())
    }

    async fn update_channel(
        &self,
        guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    ) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::UpdateChannel { guild, id, spec });
        self.check_fail(n)
    }

    async fn delete_channel(&self, guild: GuildId, id: ChannelId) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::DeleteChannel { guild, id });
        self.check_fail(n)
    }

    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::UpsertOverwrite {
            guild,
            channel,
            target,
            allow,
            deny,
        });
        self.check_fail(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterErrorKind;
    use futures::executor::block_on;

    #[test]
    fn create_role_returns_fake_id_and_records() {
        let mock = MockDiscordAdapter::new();
        let id = block_on(mock.create_role(
            GuildId(1),
            RoleSpec {
                name: Some("VIP".to_string()),
                permissions: None,
            },
        ))
        .unwrap();
        assert_eq!(id, RoleId(900_000));
        assert_eq!(mock.calls().len(), 1);
        assert!(matches!(mock.calls()[0], AdapterCall::CreateRole { .. }));
    }

    #[test]
    fn fail_on_triggers_at_call_number() {
        let mock = MockDiscordAdapter::with_failure(
            1,
            AdapterError::new(AdapterErrorKind::RateLimited, "rl"),
        );
        let r = block_on(mock.create_role(
            GuildId(1),
            RoleSpec {
                name: None,
                permissions: None,
            },
        ));
        assert_eq!(r.unwrap_err().kind, AdapterErrorKind::RateLimited);
    }

    #[test]
    fn with_failures_fails_multiple_calls() {
        let mock = MockDiscordAdapter::with_failures(vec![
            (1, AdapterError::new(AdapterErrorKind::RateLimited, "a")),
            (2, AdapterError::new(AdapterErrorKind::Forbidden, "b")),
        ]);
        assert!(block_on(mock.create_role(
            GuildId(1),
            RoleSpec {
                name: None,
                permissions: None
            }
        ))
        .is_err());
        assert!(block_on(mock.delete_role(GuildId(1), RoleId(5))).is_err());
        assert!(block_on(mock.delete_role(GuildId(1), RoleId(6))).is_ok());
    }
}
