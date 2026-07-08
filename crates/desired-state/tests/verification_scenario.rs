use std::collections::BTreeMap;

use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, DesiredStateMode,
    FeatureIntent, Identity, Ownership, ResourceKey, RoleIntent, VerificationIntent,
};
use discord_model::{ChannelType, Permissions};

#[test]
fn verification_scenario_validates_and_roundtrips() {
    let verified = ResourceKey("verified_member".to_string());
    let verify_ch = ResourceKey("verification_channel".to_string());
    let general_ch = ResourceKey("general_channel".to_string());

    let role = RoleIntent {
        identity: Identity {
            key: verified.clone(),
            ..Default::default()
        },
        name: Some("인증됨".to_string()),
        permissions: Some(Permissions::empty()),
    };

    let verification_channel = ChannelIntent {
        identity: Identity {
            key: verify_ch.clone(),
            ..Default::default()
        },
        name: Some("인증".to_string()),
        channel_type: Some(ChannelType::Text),
        parent: None,
        access: Some(AccessIntent {
            everyone: Some(AccessGrant {
                allow: vec![Capability::View],
                deny: vec![],
            }),
            roles: BTreeMap::new(),
        }),
        raw_overwrites: None,
    };

    let mut general_roles = BTreeMap::new();
    general_roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    let general_channel = ChannelIntent {
        identity: Identity {
            key: general_ch.clone(),
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
            roles: general_roles,
        }),
        raw_overwrites: None,
    };

    let panel = FeatureIntent::Verification(VerificationIntent {
        identity: Identity {
            key: ResourceKey("verify_panel".to_string()),
            ownership: Ownership::Managed,
            ..Default::default()
        },
        channel: verify_ch,
        grants_role: verified,
    });

    let state = DesiredState {
        mode: DesiredStateMode::Patch,
        scope: None,
        roles: vec![role],
        channels: vec![verification_channel, general_channel],
        features: vec![panel],
    };

    assert!(state.validate().is_ok());

    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<DesiredState>(&json).unwrap(), state);
}
