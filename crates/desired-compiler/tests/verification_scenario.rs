use std::collections::BTreeMap;

use desired_compiler::{compile, NormalizedTarget};
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, DesiredStateMode,
    FeatureIntent, Identity, ResourceKey, RoleIntent, VerificationIntent,
};
use discord_model::{ChannelType, Permissions};

#[test]
fn compiles_verification_scenario() {
    let verified = ResourceKey("verified_member".to_string());

    let general = {
        let mut roles = BTreeMap::new();
        roles.insert(
            verified.clone(),
            AccessGrant {
                allow: vec![Capability::View, Capability::Send],
                deny: vec![],
            },
        );
        ChannelIntent {
            identity: Identity {
                key: ResourceKey("general_channel".to_string()),
                ..Default::default()
            },
            name: Some("일반".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant {
                    allow: vec![],
                    deny: vec![Capability::View],
                }),
                roles,
            }),
            raw_overwrites: None,
        }
    };

    let ds = DesiredState {
        mode: DesiredStateMode::Patch,
        scope: None,
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified.clone(),
                ..Default::default()
            },
            name: Some("인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![general],
        features: vec![FeatureIntent::Verification(VerificationIntent {
            identity: Identity {
                key: ResourceKey("panel".to_string()),
                ..Default::default()
            },
            channel: ResourceKey("verification_channel".to_string()),
            grants_role: verified.clone(),
        })],
    };

    let out = compile(&ds).unwrap();

    let ch = &out.channels[0];
    let everyone = ch
        .overwrites
        .iter()
        .find(|overwrite| overwrite.target == NormalizedTarget::Everyone)
        .unwrap();
    assert_eq!(everyone.deny, Permissions::VIEW_CHANNEL);
    let v = ch
        .overwrites
        .iter()
        .find(|overwrite| overwrite.target == NormalizedTarget::Role(verified.clone()))
        .unwrap();
    assert_eq!(
        v.allow,
        Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES
    );

    assert_eq!(out.roles.len(), 1);
    assert_eq!(out.verification_panels.len(), 1);
    assert_eq!(out.mode, DesiredStateMode::Patch);
}
