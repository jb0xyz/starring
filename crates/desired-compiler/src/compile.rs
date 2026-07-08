use std::collections::BTreeMap;

use desired_state::{
    AccessGrant, ChannelIntent, DesiredState, FeatureIntent, OverwriteOp, OverwriteTargetIntent,
    PermissionOverwriteIntent, RoleIntent, VerificationIntent,
};
use discord_model::Permissions;

use crate::capability::capabilities_to_permissions;
use crate::error::CompileError;
use crate::normalized::{
    NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
    NormalizedTarget, NormalizedVerificationPanel,
};

pub fn compile(desired: &DesiredState) -> Result<NormalizedDesiredState, Vec<CompileError>> {
    let mut errors = Vec::new();

    let roles = desired.roles.iter().map(normalize_role).collect();
    let verification_panels = desired
        .features
        .iter()
        .filter_map(normalize_feature)
        .collect();

    let mut channels = Vec::new();
    for channel in &desired.channels {
        match normalize_channel(channel) {
            Ok(normalized_channel) => channels.push(normalized_channel),
            Err(mut channel_errors) => errors.append(&mut channel_errors),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NormalizedDesiredState {
        mode: desired.mode,
        scope: desired.scope.clone(),
        roles,
        channels,
        verification_panels,
    })
}

fn normalize_role(role: &RoleIntent) -> NormalizedRole {
    NormalizedRole {
        identity: role.identity.clone(),
        name: role.name.clone(),
        permissions: role.permissions,
    }
}

fn normalize_feature(feature: &FeatureIntent) -> Option<NormalizedVerificationPanel> {
    match feature {
        FeatureIntent::Verification(verification) => Some(normalize_verification(verification)),
        FeatureIntent::Moderation(_) | FeatureIntent::Logging(_) => None,
    }
}

fn normalize_verification(verification: &VerificationIntent) -> NormalizedVerificationPanel {
    NormalizedVerificationPanel {
        identity: verification.identity.clone(),
        channel: verification.channel.clone(),
        grants_role: verification.grants_role.clone(),
    }
}

fn normalize_channel(channel: &ChannelIntent) -> Result<NormalizedChannel, Vec<CompileError>> {
    let mut map: BTreeMap<NormalizedTarget, (Permissions, Permissions)> = BTreeMap::new();

    if let Some(access) = &channel.access {
        if let Some(grant) = &access.everyone {
            apply_grant(map.entry(NormalizedTarget::Everyone).or_default(), grant);
        }
        for (key, grant) in &access.roles {
            apply_grant(
                map.entry(NormalizedTarget::Role(key.clone())).or_default(),
                grant,
            );
        }
    }

    if let Some(raw_overwrite_list) = &channel.raw_overwrites {
        for raw_overwrite in raw_overwrite_list {
            apply_raw(
                map.entry(raw_target(&raw_overwrite.target)).or_default(),
                raw_overwrite,
            );
        }
    }

    let mut errors = Vec::new();
    let mut overwrites = Vec::new();
    for (target, (allow, deny)) in map {
        if allow.bits() & deny.bits() != 0 {
            errors.push(CompileError::PermissionConflict {
                channel: channel.identity.key.0.clone(),
                target: target_label(&target),
            });
            continue;
        }
        overwrites.push(NormalizedOverwrite {
            target,
            allow,
            deny,
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NormalizedChannel {
        identity: channel.identity.clone(),
        name: channel.name.clone(),
        channel_type: channel.channel_type,
        parent: channel.parent.clone(),
        overwrites,
    })
}

fn apply_grant(entry: &mut (Permissions, Permissions), grant: &AccessGrant) {
    entry.0 |= capabilities_to_permissions(&grant.allow);
    entry.1 |= capabilities_to_permissions(&grant.deny);
}

fn apply_raw(entry: &mut (Permissions, Permissions), raw: &PermissionOverwriteIntent) {
    match raw.op {
        OverwriteOp::Add => {
            entry.0 |= raw.allow;
            entry.1 |= raw.deny;
        }
        OverwriteOp::Remove => {
            entry.0 = Permissions::from_bits_retain(entry.0.bits() & !raw.allow.bits());
            entry.1 = Permissions::from_bits_retain(entry.1.bits() & !raw.deny.bits());
        }
        OverwriteOp::Replace => {
            entry.0 = raw.allow;
            entry.1 = raw.deny;
        }
    }
}

fn raw_target(target: &OverwriteTargetIntent) -> NormalizedTarget {
    match target {
        OverwriteTargetIntent::Role(key) => NormalizedTarget::Role(key.clone()),
        OverwriteTargetIntent::Member(id) => NormalizedTarget::Member(id.clone()),
    }
}

fn target_label(target: &NormalizedTarget) -> String {
    match target {
        NormalizedTarget::Everyone => "everyone".to_string(),
        NormalizedTarget::Role(key) => key.0.clone(),
        NormalizedTarget::Member(id) => id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CompileError;
    use crate::normalized::NormalizedTarget;
    use desired_state::{
        AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, FeatureIntent,
        Identity, OverwriteOp, OverwriteTargetIntent, PermissionOverwriteIntent, ResourceKey,
        RoleIntent, VerificationIntent,
    };
    use discord_model::Permissions;
    use std::collections::BTreeMap;

    fn channel_with(
        access: Option<AccessIntent>,
        raw: Option<Vec<PermissionOverwriteIntent>>,
    ) -> ChannelIntent {
        ChannelIntent {
            identity: Identity {
                key: ResourceKey("c".to_string()),
                ..Default::default()
            },
            name: Some("c".to_string()),
            channel_type: None,
            parent: None,
            access,
            raw_overwrites: raw,
        }
    }

    fn find<'a>(
        nc: &'a crate::normalized::NormalizedChannel,
        t: &NormalizedTarget,
    ) -> &'a crate::normalized::NormalizedOverwrite {
        nc.overwrites.iter().find(|o| &o.target == t).unwrap()
    }

    #[test]
    fn lowers_everyone_and_role_access() {
        let mut roles = BTreeMap::new();
        roles.insert(
            ResourceKey("verified".to_string()),
            AccessGrant {
                allow: vec![Capability::View, Capability::Send],
                deny: vec![],
            },
        );
        let access = AccessIntent {
            everyone: Some(AccessGrant {
                allow: vec![],
                deny: vec![Capability::View],
            }),
            roles,
        };
        let ds = DesiredState {
            channels: vec![channel_with(Some(access), None)],
            ..Default::default()
        };
        let out = compile(&ds).unwrap();
        let ch = &out.channels[0];
        let everyone = find(ch, &NormalizedTarget::Everyone);
        assert_eq!(everyone.deny, Permissions::VIEW_CHANNEL);
        let verified = find(
            ch,
            &NormalizedTarget::Role(ResourceKey("verified".to_string())),
        );
        assert_eq!(
            verified.allow,
            Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES
        );
    }

    #[test]
    fn raw_add_and_remove_merge() {
        let raw = vec![PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified".to_string())),
            op: OverwriteOp::Add,
            allow: Permissions::EMBED_LINKS,
            deny: Permissions::empty(),
        }];
        let mut roles = BTreeMap::new();
        roles.insert(
            ResourceKey("verified".to_string()),
            AccessGrant {
                allow: vec![Capability::View],
                deny: vec![],
            },
        );
        let access = AccessIntent {
            everyone: None,
            roles,
        };
        let ds = DesiredState {
            channels: vec![channel_with(Some(access), Some(raw))],
            ..Default::default()
        };
        let out = compile(&ds).unwrap();
        let v = find(
            &out.channels[0],
            &NormalizedTarget::Role(ResourceKey("verified".to_string())),
        );
        assert_eq!(
            v.allow,
            Permissions::VIEW_CHANNEL | Permissions::EMBED_LINKS
        );
    }

    #[test]
    fn raw_replace_overrides() {
        let raw = vec![PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified".to_string())),
            op: OverwriteOp::Replace,
            allow: Permissions::SPEAK,
            deny: Permissions::empty(),
        }];
        let mut roles = BTreeMap::new();
        roles.insert(
            ResourceKey("verified".to_string()),
            AccessGrant {
                allow: vec![Capability::View],
                deny: vec![],
            },
        );
        let ds = DesiredState {
            channels: vec![channel_with(
                Some(AccessIntent {
                    everyone: None,
                    roles,
                }),
                Some(raw),
            )],
            ..Default::default()
        };
        let out = compile(&ds).unwrap();
        let v = find(
            &out.channels[0],
            &NormalizedTarget::Role(ResourceKey("verified".to_string())),
        );
        assert_eq!(v.allow, Permissions::SPEAK);
    }

    #[test]
    fn conflict_when_allow_and_deny_overlap() {
        let raw = vec![PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified".to_string())),
            op: OverwriteOp::Add,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        }];
        let mut roles = BTreeMap::new();
        roles.insert(
            ResourceKey("verified".to_string()),
            AccessGrant {
                allow: vec![Capability::View],
                deny: vec![],
            },
        );
        let ds = DesiredState {
            channels: vec![channel_with(
                Some(AccessIntent {
                    everyone: None,
                    roles,
                }),
                Some(raw),
            )],
            ..Default::default()
        };
        let err = compile(&ds).unwrap_err();
        assert!(matches!(err[0], CompileError::PermissionConflict { .. }));
    }

    #[test]
    fn passthrough_roles_verification_mode() {
        let ds = DesiredState {
            roles: vec![RoleIntent {
                identity: Identity {
                    key: ResourceKey("r".to_string()),
                    ..Default::default()
                },
                name: Some("r".to_string()),
                permissions: Some(Permissions::empty()),
            }],
            features: vec![
                FeatureIntent::Verification(VerificationIntent {
                    identity: Identity {
                        key: ResourceKey("p".to_string()),
                        ..Default::default()
                    },
                    channel: ResourceKey("c".to_string()),
                    grants_role: ResourceKey("r".to_string()),
                }),
                FeatureIntent::Moderation(Default::default()),
            ],
            ..Default::default()
        };
        let out = compile(&ds).unwrap();
        assert_eq!(out.roles.len(), 1);
        assert_eq!(out.verification_panels.len(), 1);
    }
}
