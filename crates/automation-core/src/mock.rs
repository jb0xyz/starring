use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use automation_state::ButtonSpec;
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};

use crate::adapter::{
    AdapterError, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter, InteractionResponder,
    PostPanelSpec,
};
use crate::plan::ModalPresentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationCall {
    GrantRole {
        guild: GuildId,
        member: UserId,
        role: RoleId,
    },
    CreateChannel {
        guild: GuildId,
        name: String,
    },
    CreateRole {
        guild: GuildId,
        name: String,
    },
    UpsertOverwrite {
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
    PostPanel {
        guild: GuildId,
        channel: ChannelId,
        content: String,
        buttons: Vec<ButtonSpec>,
    },
}

#[derive(Default)]
pub struct MockMutationAdapter {
    calls: Mutex<Vec<MutationCall>>,
    fail: Option<AdapterError>,
    next_id: AtomicU64,
}

impl MockMutationAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn failing(error: AdapterError) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail: Some(error),
            next_id: AtomicU64::new(0),
        }
    }

    pub fn calls(&self) -> Vec<MutationCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl DiscordMutationAdapter for MockMutationAdapter {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError> {
        self.calls.lock().unwrap().push(MutationCall::GrantRole {
            guild,
            member,
            role,
        });
        match &self.fail {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(MutationCall::CreateChannel {
                guild,
                name: spec.name,
            });
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        Ok(ChannelId(
            800_000 + self.next_id.fetch_add(1, Ordering::SeqCst),
        ))
    }

    async fn create_role(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        self.calls.lock().unwrap().push(MutationCall::CreateRole {
            guild,
            name: spec.name,
        });
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        Ok(RoleId(
            800_000 + self.next_id.fetch_add(1, Ordering::SeqCst),
        ))
    }

    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(MutationCall::UpsertOverwrite {
                guild,
                channel,
                target,
                allow,
                deny,
            });
        match &self.fail {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    async fn post_panel(
        &self,
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        self.calls.lock().unwrap().push(MutationCall::PostPanel {
            guild,
            channel,
            content: spec.content,
            buttons: spec.buttons,
        });
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        Ok(MessageId(
            800_000 + self.next_id.fetch_add(1, Ordering::SeqCst),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponderCall {
    RespondEphemeral { content: String },
    OpenModal { modal: String },
    DeferEphemeral,
    EditResponse { content: String },
}

#[derive(Default)]
pub struct MockInteractionResponder {
    calls: Mutex<Vec<ResponderCall>>,
}

impl MockInteractionResponder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> Vec<ResponderCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl InteractionResponder for MockInteractionResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(ResponderCall::RespondEphemeral { content });
        Ok(())
    }

    async fn open_modal(&self, modal: &ModalPresentation) -> Result<(), AdapterError> {
        self.calls.lock().unwrap().push(ResponderCall::OpenModal {
            modal: modal.key.clone(),
        });
        Ok(())
    }

    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(ResponderCall::DeferEphemeral);
        Ok(())
    }

    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(ResponderCall::EditResponse { content });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterErrorKind;
    use discord_model::{GuildId, RoleId, UserId};
    use futures::executor::block_on;

    #[test]
    fn mutation_records_and_succeeds() {
        let mock = MockMutationAdapter::new();
        block_on(mock.grant_role(GuildId(1), UserId(42), RoleId(7))).unwrap();
        assert_eq!(
            mock.calls(),
            vec![MutationCall::GrantRole {
                guild: GuildId(1),
                member: UserId(42),
                role: RoleId(7),
            }]
        );
    }

    #[test]
    fn mutation_can_fail() {
        let mock =
            MockMutationAdapter::failing(AdapterError::new(AdapterErrorKind::Forbidden, "no"));
        let result = block_on(mock.grant_role(GuildId(1), UserId(42), RoleId(7)));
        assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn responder_records() {
        let mock = MockInteractionResponder::new();
        block_on(mock.respond_ephemeral("hi".to_string())).unwrap();
        assert_eq!(
            mock.calls(),
            vec![ResponderCall::RespondEphemeral {
                content: "hi".to_string(),
            }]
        );
    }

    #[test]
    fn responder_records_open_modal() {
        let mock = MockInteractionResponder::new();
        let presentation = ModalPresentation {
            key: "study_room_modal".to_string(),
            title: "Create study room".to_string(),
            fields: vec![],
        };
        block_on(mock.open_modal(&presentation)).unwrap();
        assert_eq!(
            mock.calls(),
            vec![ResponderCall::OpenModal {
                modal: "study_room_modal".to_string(),
            }]
        );
    }
}
