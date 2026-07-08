use std::collections::BTreeSet;

use thiserror::Error;

use crate::access::OverwriteTargetIntent;
use crate::feature::FeatureIntent;
use crate::identity::ResourceKey;
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
}

impl DesiredState {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        self.check_key_uniqueness(&mut errors);
        self.check_reference_integrity(&mut errors);
        self.check_mode_scope(&mut errors);
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
}

#[cfg(test)]
mod tests {
    use crate::channel::ChannelIntent;
    use crate::identity::{Identity, ResourceKey};
    use crate::mode::{DesiredStateMode, ResourceScope, Scope};
    use crate::role::RoleIntent;
    use crate::state::DesiredState;
    use crate::validate::ValidationError;

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
}
