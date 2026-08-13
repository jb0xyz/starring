use std::collections::BTreeMap;

use discord_model::{ChannelId, GuildId, UserId};
use zeroize::Zeroize;

use crate::{
    build_interaction_request_digest_v1, InteractionReceiptClaimRootV1,
    InteractionRequestDigestErrorV1, InteractionRequestDigestInputV1, InteractionRequestPayloadV1,
};

/// The token-free request material that was accepted at the shared gateway boundary.
///
/// This type intentionally does not implement `Debug`: modal input values may contain private
/// user text. It is only an input to [`VerifiedInteractionRequestV1::verify`].
#[derive(Clone, PartialEq, Eq)]
pub enum InteractionRequestMaterialV1 {
    Button {
        guild_id: GuildId,
        channel_id: ChannelId,
        actor_id: UserId,
        locale: Option<String>,
        custom_id: String,
    },
    ModalSubmit {
        guild_id: GuildId,
        channel_id: ChannelId,
        actor_id: UserId,
        locale: Option<String>,
        custom_id: String,
        inputs: BTreeMap<String, String>,
    },
}

impl Drop for InteractionRequestMaterialV1 {
    fn drop(&mut self) {
        if let Self::ModalSubmit { inputs, .. } = self {
            for value in inputs.values_mut() {
                value.zeroize();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifiedInteractionRequestKindV1 {
    Button,
    ModalSubmit,
}

/// Proof that exact raw interaction material hashes to the request digest in an authoritative
/// durable receipt claim.
///
/// Construction consumes the claim root and recomputes the existing canonical request digest.
/// Callers cannot independently substitute an actor, channel, custom ID, or modal payload after
/// verification. The type intentionally has a redacted `Debug` implementation.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedInteractionRequestV1 {
    claim_root: InteractionReceiptClaimRootV1,
    guild_id: GuildId,
    channel_id: ChannelId,
    actor_id: UserId,
    locale: Option<String>,
    custom_id: String,
    inputs: BTreeMap<String, String>,
    kind: VerifiedInteractionRequestKindV1,
}

impl VerifiedInteractionRequestV1 {
    pub fn verify(
        claim_root: InteractionReceiptClaimRootV1,
        mut material: InteractionRequestMaterialV1,
    ) -> Result<Self, VerifiedInteractionRequestErrorV1> {
        let route_guild_id = claim_root.route().process_identity().target.guild_id;
        let (guild_id, channel_id, actor_id, locale, custom_id, inputs, kind) = match &mut material
        {
            InteractionRequestMaterialV1::Button {
                guild_id,
                channel_id,
                actor_id,
                locale,
                custom_id,
            } => (
                *guild_id,
                *channel_id,
                *actor_id,
                locale.take(),
                std::mem::take(custom_id),
                BTreeMap::new(),
                VerifiedInteractionRequestKindV1::Button,
            ),
            InteractionRequestMaterialV1::ModalSubmit {
                guild_id,
                channel_id,
                actor_id,
                locale,
                custom_id,
                inputs,
            } => (
                *guild_id,
                *channel_id,
                *actor_id,
                locale.take(),
                std::mem::take(custom_id),
                std::mem::take(inputs),
                VerifiedInteractionRequestKindV1::ModalSubmit,
            ),
        };
        if guild_id != route_guild_id {
            return Err(VerifiedInteractionRequestErrorV1::GuildMismatch);
        }
        let payload = match kind {
            VerifiedInteractionRequestKindV1::Button => InteractionRequestPayloadV1::Button {
                custom_id: &custom_id,
            },
            VerifiedInteractionRequestKindV1::ModalSubmit => {
                InteractionRequestPayloadV1::ModalSubmit {
                    custom_id: &custom_id,
                    inputs: &inputs,
                }
            }
        };
        let recomputed = build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
            receipt_identity: claim_root.identity(),
            guild_id,
            channel_id,
            actor_id,
            locale: locale.as_deref(),
            payload,
        })?;
        if &recomputed != claim_root.request_digest() {
            return Err(VerifiedInteractionRequestErrorV1::RequestDigestMismatch);
        }
        Ok(Self {
            claim_root,
            guild_id,
            channel_id,
            actor_id,
            locale,
            custom_id,
            inputs,
            kind,
        })
    }

    pub fn claim_root(&self) -> &InteractionReceiptClaimRootV1 {
        &self.claim_root
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    pub fn actor_id(&self) -> UserId {
        self.actor_id
    }

    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }

    pub fn inputs(&self) -> &BTreeMap<String, String> {
        &self.inputs
    }

    pub fn kind(&self) -> VerifiedInteractionRequestKindV1 {
        self.kind
    }
}

impl Drop for VerifiedInteractionRequestV1 {
    fn drop(&mut self) {
        for value in self.inputs.values_mut() {
            value.zeroize();
        }
    }
}

impl std::fmt::Debug for VerifiedInteractionRequestV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedInteractionRequestV1")
            .field("receipt_identity", &self.claim_root.identity())
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("actor_id", &self.actor_id)
            .field("kind", &self.kind)
            .field("locale_present", &self.locale.is_some())
            .field("custom_id", &"[redacted]")
            .field("input_count", &self.inputs.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerifiedInteractionRequestErrorV1 {
    #[error("verified interaction request guild does not match its authoritative route")]
    GuildMismatch,
    #[error("verified interaction request digest does not match its durable receipt claim")]
    RequestDigestMismatch,
    #[error("verified interaction request material is invalid")]
    RequestDigest(#[from] InteractionRequestDigestErrorV1),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_support::static_route, DiscordApplicationIdV1, DiscordInteractionIdV1,
        InteractionExpectedRouteV1, InteractionReceiptClaimCandidateV1,
        InteractionReceiptIdentityV1,
    };

    fn identity() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(11).unwrap(),
            DiscordInteractionIdV1::new(12).unwrap(),
        )
    }

    fn button_material(actor_id: u64) -> InteractionRequestMaterialV1 {
        InteractionRequestMaterialV1::Button {
            guild_id: GuildId(107),
            channel_id: ChannelId(21),
            actor_id: UserId(actor_id),
            locale: Some("en-US".to_string()),
            custom_id: "starring:107:studyroom:button:join".to_string(),
        }
    }

    fn claim_for(material: &InteractionRequestMaterialV1) -> InteractionReceiptClaimRootV1 {
        let route = static_route(7);
        let digest = match material {
            InteractionRequestMaterialV1::Button {
                guild_id,
                channel_id,
                actor_id,
                locale,
                custom_id,
            } => build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
                receipt_identity: identity(),
                guild_id: *guild_id,
                channel_id: *channel_id,
                actor_id: *actor_id,
                locale: locale.as_deref(),
                payload: InteractionRequestPayloadV1::Button { custom_id },
            })
            .unwrap(),
            InteractionRequestMaterialV1::ModalSubmit {
                guild_id,
                channel_id,
                actor_id,
                locale,
                custom_id,
                inputs,
            } => build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
                receipt_identity: identity(),
                guild_id: *guild_id,
                channel_id: *channel_id,
                actor_id: *actor_id,
                locale: locale.as_deref(),
                payload: InteractionRequestPayloadV1::ModalSubmit { custom_id, inputs },
            })
            .unwrap(),
        };
        InteractionReceiptClaimCandidateV1::new(
            identity(),
            InteractionExpectedRouteV1::from_authoritative(&route),
            digest,
        )
        .bind_authoritative(route)
        .unwrap()
    }

    #[test]
    fn exact_material_verifies_and_debug_redacts_payload() {
        let material = button_material(22);
        let verified =
            VerifiedInteractionRequestV1::verify(claim_for(&material), material).unwrap();

        assert_eq!(verified.actor_id(), UserId(22));
        assert_eq!(verified.custom_id(), "starring:107:studyroom:button:join");
        let debug = format!("{verified:?}");
        assert!(!debug.contains("studyroom"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn substituted_actor_is_rejected_by_request_digest() {
        let original = button_material(22);
        let claim = claim_for(&original);
        let substituted = button_material(23);

        assert_eq!(
            VerifiedInteractionRequestV1::verify(claim, substituted),
            Err(VerifiedInteractionRequestErrorV1::RequestDigestMismatch)
        );
    }

    #[test]
    fn route_guild_mismatch_is_rejected_before_digest_acceptance() {
        let material = button_material(22);
        let claim = claim_for(&material);
        let mismatched = InteractionRequestMaterialV1::Button {
            guild_id: GuildId(108),
            channel_id: ChannelId(21),
            actor_id: UserId(22),
            locale: Some("en-US".to_string()),
            custom_id: "starring:108:studyroom:button:join".to_string(),
        };

        assert_eq!(
            VerifiedInteractionRequestV1::verify(claim, mismatched),
            Err(VerifiedInteractionRequestErrorV1::GuildMismatch)
        );
    }

    #[test]
    fn modal_input_is_digest_bound_and_debug_never_exposes_text() {
        let material = InteractionRequestMaterialV1::ModalSubmit {
            guild_id: GuildId(107),
            channel_id: ChannelId(21),
            actor_id: UserId(22),
            locale: Some("en-US".to_string()),
            custom_id: "starring:107:studyroom:modal:create_room".to_string(),
            inputs: BTreeMap::from([("room_name".to_string(), "private-study-secret".to_string())]),
        };
        let claim = claim_for(&material);
        let verified = VerifiedInteractionRequestV1::verify(claim.clone(), material).unwrap();
        assert_eq!(
            verified.kind(),
            VerifiedInteractionRequestKindV1::ModalSubmit
        );
        assert!(!format!("{verified:?}").contains("private-study-secret"));

        let tampered = InteractionRequestMaterialV1::ModalSubmit {
            guild_id: GuildId(107),
            channel_id: ChannelId(21),
            actor_id: UserId(22),
            locale: Some("en-US".to_string()),
            custom_id: "starring:107:studyroom:modal:create_room".to_string(),
            inputs: BTreeMap::from([("room_name".to_string(), "different-room".to_string())]),
        };
        assert_eq!(
            VerifiedInteractionRequestV1::verify(claim, tampered),
            Err(VerifiedInteractionRequestErrorV1::RequestDigestMismatch)
        );
    }
}
