use discord_model::{ChannelId, GuildId, MessageId};

use crate::strict::{
    StrictDeclaredPanelV1, StrictDeleteOutcomeV1, StrictExternalPostResultV1,
    StrictObservedMessageV1, StrictPanelInstaller,
};
use crate::InstallerError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrictPanelExternalCallV1 {
    Observe,
    Post,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrictPanelExternalCallFenceErrorV1(String);

impl StrictPanelExternalCallFenceErrorV1 {
    pub fn new(code: impl Into<String>) -> Self {
        let code = code.into();
        let valid = !code.is_empty()
            && code.len() <= 64
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        if valid {
            Self(code)
        } else {
            Self("runtime_panel_fence_rejected".to_string())
        }
    }

    pub fn code(&self) -> &str {
        &self.0
    }
}

#[allow(async_fn_in_trait)]
pub trait StrictPanelExternalCallFence {
    async fn check_external_call(
        &self,
        call: StrictPanelExternalCallV1,
    ) -> Result<(), StrictPanelExternalCallFenceErrorV1>;
}

pub struct FencedStrictPanelInstallerV1<'a, F, I> {
    fence: &'a F,
    installer: &'a I,
}

impl<'a, F, I> FencedStrictPanelInstallerV1<'a, F, I> {
    pub fn new(fence: &'a F, installer: &'a I) -> Self {
        Self { fence, installer }
    }
}

impl<F, I> StrictPanelInstaller for FencedStrictPanelInstallerV1<'_, F, I>
where
    F: StrictPanelExternalCallFence,
    I: StrictPanelInstaller,
{
    async fn observe_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> Result<StrictObservedMessageV1, InstallerError> {
        self.fence
            .check_external_call(StrictPanelExternalCallV1::Observe)
            .await
            .map_err(|error| InstallerError::new(error.code()))?;
        self.installer.observe_message(channel_id, message_id).await
    }

    async fn post_message(
        &self,
        channel_id: ChannelId,
        guild_id: GuildId,
        ruleset_key: &str,
        panel: &StrictDeclaredPanelV1,
    ) -> StrictExternalPostResultV1 {
        if self
            .fence
            .check_external_call(StrictPanelExternalCallV1::Post)
            .await
            .is_err()
        {
            return StrictExternalPostResultV1::DefinitelyNotApplied;
        }
        self.installer
            .post_message(channel_id, guild_id, ruleset_key, panel)
            .await
    }

    async fn delete_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> StrictDeleteOutcomeV1 {
        if self
            .fence
            .check_external_call(StrictPanelExternalCallV1::Delete)
            .await
            .is_err()
        {
            return StrictDeleteOutcomeV1::DefinitelyNotApplied;
        }
        self.installer.delete_message(channel_id, message_id).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use automation_state::PanelSpec;
    use desired_state::ResourceKey;

    use super::*;
    use crate::strict::StrictPanelMessagePayloadV1;

    struct Gate {
        allowed: bool,
        calls: Mutex<Vec<StrictPanelExternalCallV1>>,
    }

    impl StrictPanelExternalCallFence for Gate {
        async fn check_external_call(
            &self,
            call: StrictPanelExternalCallV1,
        ) -> Result<(), StrictPanelExternalCallFenceErrorV1> {
            self.calls.lock().unwrap().push(call);
            if self.allowed {
                Ok(())
            } else {
                Err(StrictPanelExternalCallFenceErrorV1::new(
                    "runtime_panel_ownership_lost",
                ))
            }
        }
    }

    #[derive(Default)]
    struct Installer {
        calls: Mutex<Vec<StrictPanelExternalCallV1>>,
    }

    impl StrictPanelInstaller for Installer {
        async fn observe_message(
            &self,
            _: ChannelId,
            _: MessageId,
        ) -> Result<StrictObservedMessageV1, InstallerError> {
            self.calls
                .lock()
                .unwrap()
                .push(StrictPanelExternalCallV1::Observe);
            Ok(StrictObservedMessageV1::Missing)
        }

        async fn post_message(
            &self,
            _: ChannelId,
            _: GuildId,
            _: &str,
            _: &StrictDeclaredPanelV1,
        ) -> StrictExternalPostResultV1 {
            self.calls
                .lock()
                .unwrap()
                .push(StrictPanelExternalCallV1::Post);
            StrictExternalPostResultV1::Applied(MessageId(9))
        }

        async fn delete_message(&self, _: ChannelId, _: MessageId) -> StrictDeleteOutcomeV1 {
            self.calls
                .lock()
                .unwrap()
                .push(StrictPanelExternalCallV1::Delete);
            StrictDeleteOutcomeV1::Deleted
        }
    }

    fn panel() -> StrictDeclaredPanelV1 {
        StrictDeclaredPanelV1 {
            spec: PanelSpec {
                key: "entry".to_string(),
                channel: ResourceKey("hub".to_string()),
                content: "Welcome".to_string(),
                buttons: Vec::new(),
            },
            expected_payload: StrictPanelMessagePayloadV1 {
                content: "Welcome".to_string(),
                action_rows: Vec::new(),
            },
        }
    }

    #[test]
    fn rejected_fence_prevents_every_external_call() {
        futures::executor::block_on(async {
            let gate = Gate {
                allowed: false,
                calls: Mutex::new(Vec::new()),
            };
            let installer = Installer::default();
            let fenced = FencedStrictPanelInstallerV1::new(&gate, &installer);
            assert!(fenced
                .observe_message(ChannelId(1), MessageId(2))
                .await
                .is_err());
            assert_eq!(
                fenced
                    .post_message(ChannelId(1), GuildId(2), "studyroom", &panel())
                    .await,
                StrictExternalPostResultV1::DefinitelyNotApplied
            );
            assert_eq!(
                fenced.delete_message(ChannelId(1), MessageId(2)).await,
                StrictDeleteOutcomeV1::DefinitelyNotApplied
            );
            assert_eq!(installer.calls.lock().unwrap().as_slice(), &[]);
            assert_eq!(
                gate.calls.lock().unwrap().as_slice(),
                &[
                    StrictPanelExternalCallV1::Observe,
                    StrictPanelExternalCallV1::Post,
                    StrictPanelExternalCallV1::Delete,
                ]
            );
        });
    }

    #[test]
    fn accepted_fence_delegates_every_external_call() {
        futures::executor::block_on(async {
            let gate = Gate {
                allowed: true,
                calls: Mutex::new(Vec::new()),
            };
            let installer = Installer::default();
            let fenced = FencedStrictPanelInstallerV1::new(&gate, &installer);
            assert_eq!(
                fenced
                    .observe_message(ChannelId(1), MessageId(2))
                    .await
                    .unwrap(),
                StrictObservedMessageV1::Missing
            );
            assert_eq!(
                fenced
                    .post_message(ChannelId(1), GuildId(2), "studyroom", &panel())
                    .await,
                StrictExternalPostResultV1::Applied(MessageId(9))
            );
            assert_eq!(
                fenced.delete_message(ChannelId(1), MessageId(2)).await,
                StrictDeleteOutcomeV1::Deleted
            );
            assert_eq!(
                installer.calls.lock().unwrap().as_slice(),
                &[
                    StrictPanelExternalCallV1::Observe,
                    StrictPanelExternalCallV1::Post,
                    StrictPanelExternalCallV1::Delete,
                ]
            );
        });
    }
}
