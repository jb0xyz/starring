use std::collections::BTreeSet;

use thiserror::Error;

use crate::access::OverwriteTargetIntent;
use crate::feature::FeatureIntent;
use crate::identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
use crate::mode::DesiredStateMode;
use crate::state::DesiredState;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("duplicate key: {0}")]
    DuplicateKey(String),
    #[error("dangling reference: {0}")]
    DanglingReference(String),
    #[error("scope present but mode is not scoped_authoritative")]
    ScopeWithoutScopedMode,
    #[error("scoped_authoritative mode requires a scope")]
    ScopedModeWithoutScope,
    #[error("referenced resource cannot be modified: {0}")]
    ReferencedNotMutable(String),
    #[error("absent state requires managed or adopted ownership: {0}")]
    AbsentRequiresOwnership(String),
    #[error("match by_name requires a name: {0}")]
    MatchByNameRequiresName(String),
    #[error("access and raw overwrite target the same role in channel: {0}")]
    AccessRawConflict(String),
}

impl DesiredState {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        self.check_key_uniqueness(&mut errors);
        self.check_reference_integrity(&mut errors);
        self.check_mode_scope(&mut errors);
        self.check_ownership_state(&mut errors);
        self.check_match_name(&mut errors);
        self.check_access_raw_conflict(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn declared_keys(&self) -> Vec<&ResourceKey> {
        let mut keys = Vec::new();
        for role in &self.roles {
            keys.push(&role.identity.key);
        }
        for channel in &self.channels {
            keys.push(&channel.identity.key);
        }
        for feature in &self.features {
            if let Some(identity) = feature.identity() {
                keys.push(&identity.key);
            }
        }
        keys
    }

    fn check_key_uniqueness(&self, errors: &mut Vec<ValidationError>) {
        let mut seen = BTreeSet::new();
        for key in self.declared_keys() {
            if !seen.insert(key.clone()) {
                errors.push(ValidationError::DuplicateKey(key.0.clone()));
            }
        }
    }

    fn check_reference_integrity(&self, errors: &mut Vec<ValidationError>) {
        let declared: BTreeSet<ResourceKey> = self.declared_keys().into_iter().cloned().collect();
        let mut references = Vec::new();
        for channel in &self.channels {
            if let Some(parent) = &channel.parent {
                references.push(parent);
            }
            if let Some(access) = &channel.access {
                references.extend(access.roles.keys());
            }
            if let Some(raw_overwrite_list) = &channel.raw_overwrites {
                for raw_overwrite in raw_overwrite_list {
                    if let OverwriteTargetIntent::Role(key) = &raw_overwrite.target {
                        references.push(key);
                    }
                }
            }
        }
        for feature in &self.features {
            if let FeatureIntent::Verification(verification) = feature {
                references.push(&verification.channel);
                references.push(&verification.grants_role);
            }
        }
        for key in references {
            if !declared.contains(key) {
                errors.push(ValidationError::DanglingReference(key.0.clone()));
            }
        }
    }

    fn check_mode_scope(&self, errors: &mut Vec<ValidationError>) {
        match (self.mode, self.scope.is_some()) {
            (DesiredStateMode::ScopedAuthoritative, false) => {
                errors.push(ValidationError::ScopedModeWithoutScope);
            }
            (DesiredStateMode::ScopedAuthoritative, true) => {}
            (_, true) => errors.push(ValidationError::ScopeWithoutScopedMode),
            (_, false) => {}
        }
    }

    fn check_ownership_state(&self, errors: &mut Vec<ValidationError>) {
        for role in &self.roles {
            let mutated = role.name.is_some() || role.permissions.is_some();
            Self::check_one_ownership_state(&role.identity, mutated, errors);
        }
        for channel in &self.channels {
            let mutated = channel.name.is_some()
                || channel.channel_type.is_some()
                || channel.parent.is_some()
                || channel.access.is_some()
                || channel.raw_overwrites.is_some();
            Self::check_one_ownership_state(&channel.identity, mutated, errors);
        }
    }

    fn check_one_ownership_state(
        identity: &Identity,
        mutated: bool,
        errors: &mut Vec<ValidationError>,
    ) {
        if identity.ownership == Ownership::Referenced && mutated {
            errors.push(ValidationError::ReferencedNotMutable(
                identity.key.0.clone(),
            ));
        }
        if identity.state == ResourceState::Absent && identity.ownership == Ownership::Referenced {
            errors.push(ValidationError::AbsentRequiresOwnership(
                identity.key.0.clone(),
            ));
        }
    }

    fn check_match_name(&self, errors: &mut Vec<ValidationError>) {
        for role in &self.roles {
            if role.identity.match_by == MatchStrategy::ByName && role.name.is_none() {
                errors.push(ValidationError::MatchByNameRequiresName(
                    role.identity.key.0.clone(),
                ));
            }
        }
        for channel in &self.channels {
            if channel.identity.match_by == MatchStrategy::ByName && channel.name.is_none() {
                errors.push(ValidationError::MatchByNameRequiresName(
                    channel.identity.key.0.clone(),
                ));
            }
        }
    }

    fn check_access_raw_conflict(&self, errors: &mut Vec<ValidationError>) {
        for channel in &self.channels {
            let (Some(access), Some(raw_overwrite_list)) =
                (&channel.access, &channel.raw_overwrites)
            else {
                continue;
            };
            let access_role_list: BTreeSet<&ResourceKey> = access.roles.keys().collect();
            for raw_overwrite in raw_overwrite_list {
                if let OverwriteTargetIntent::Role(key) = &raw_overwrite.target {
                    if access_role_list.contains(key) {
                        errors.push(ValidationError::AccessRawConflict(
                            channel.identity.key.0.clone(),
                        ));
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::access::{
        AccessGrant, AccessIntent, Capability, OverwriteOp, OverwriteTargetIntent as OT,
        PermissionOverwriteIntent,
    };
    use crate::channel::ChannelIntent;
    use crate::identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
    use crate::mode::{DesiredStateMode, ResourceScope, Scope};
    use crate::role::RoleIntent;
    use crate::state::DesiredState;
    use crate::validate::ValidationError;
    use discord_model::Permissions;

    fn role(key: &str) -> RoleIntent {
        RoleIntent {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                ..Default::default()
            },
            name: Some(key.to_string()),
            permissions: None,
        }
    }

    #[test]
    fn ok_when_valid() {
        let s = DesiredState {
            roles: vec![role("a")],
            ..Default::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn duplicate_key_detected() {
        let s = DesiredState {
            roles: vec![role("a"), role("a")],
            ..Default::default()
        };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::DuplicateKey("a".to_string())));
    }

    #[test]
    fn dangling_reference_detected() {
        let ch = ChannelIntent {
            identity: Identity {
                key: ResourceKey("c".to_string()),
                ..Default::default()
            },
            name: Some("c".to_string()),
            channel_type: None,
            parent: Some(ResourceKey("missing".to_string())),
            access: None,
            raw_overwrites: None,
        };
        let s = DesiredState {
            channels: vec![ch],
            ..Default::default()
        };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::DanglingReference("missing".to_string())));
    }

    #[test]
    fn scope_mode_mismatch_detected() {
        let s = DesiredState {
            mode: DesiredStateMode::Patch,
            scope: Some(Scope {
                roles: Some(ResourceScope::All),
                channels: None,
            }),
            ..Default::default()
        };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::ScopeWithoutScopedMode));
    }

    #[test]
    fn scoped_mode_requires_scope() {
        let s = DesiredState {
            mode: DesiredStateMode::ScopedAuthoritative,
            scope: None,
            ..Default::default()
        };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::ScopedModeWithoutScope));
    }

    #[test]
    fn referenced_cannot_be_mutated() {
        let role = RoleIntent {
            identity: Identity {
                key: ResourceKey("r".to_string()),
                ownership: Ownership::Referenced,
                ..Default::default()
            },
            name: Some("r".to_string()),
            permissions: Some(Permissions::empty()),
        };
        let state = DesiredState {
            roles: vec![role],
            ..Default::default()
        };
        let err = state.validate().unwrap_err();
        assert!(err.contains(&ValidationError::ReferencedNotMutable("r".to_string())));
    }

    #[test]
    fn referenced_cannot_be_absent() {
        let role = RoleIntent {
            identity: Identity {
                key: ResourceKey("r".to_string()),
                ownership: Ownership::Referenced,
                state: ResourceState::Absent,
                ..Default::default()
            },
            name: None,
            permissions: None,
        };
        let state = DesiredState {
            roles: vec![role],
            ..Default::default()
        };
        let err = state.validate().unwrap_err();
        assert!(err.contains(&ValidationError::AbsentRequiresOwnership("r".to_string())));
    }

    #[test]
    fn match_by_name_requires_name() {
        let role = RoleIntent {
            identity: Identity {
                key: ResourceKey("r".to_string()),
                match_by: MatchStrategy::ByName,
                ..Default::default()
            },
            name: None,
            permissions: None,
        };
        let state = DesiredState {
            roles: vec![role],
            ..Default::default()
        };
        let err = state.validate().unwrap_err();
        assert!(err.contains(&ValidationError::MatchByNameRequiresName("r".to_string())));
    }

    #[test]
    fn access_raw_conflict_detected() {
        let key = ResourceKey("verified".to_string());
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(
            key.clone(),
            AccessGrant {
                allow: vec![Capability::View],
                deny: vec![],
            },
        );
        let channel = ChannelIntent {
            identity: Identity {
                key: ResourceKey("c".to_string()),
                ..Default::default()
            },
            name: Some("c".to_string()),
            channel_type: None,
            parent: None,
            access: Some(AccessIntent {
                everyone: None,
                roles,
            }),
            raw_overwrites: Some(vec![PermissionOverwriteIntent {
                target: OT::Role(key.clone()),
                op: OverwriteOp::Add,
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }]),
        };
        let referenced = RoleIntent {
            identity: Identity {
                key,
                ownership: Ownership::Referenced,
                ..Default::default()
            },
            name: None,
            permissions: None,
        };
        let state = DesiredState {
            roles: vec![referenced],
            channels: vec![channel],
            ..Default::default()
        };
        let err = state.validate().unwrap_err();
        assert!(err.contains(&ValidationError::AccessRawConflict("c".to_string())));
    }
}
