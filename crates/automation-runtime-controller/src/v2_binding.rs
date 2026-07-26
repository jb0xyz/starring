use std::num::NonZeroU64;

use automation_runtime_convergence::{
    BindingRevision, InstallationId, RuntimeDeploymentTargetV1, TenantId,
};
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Serialize};

use crate::RuntimeDeploymentScopeV1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBindingPinV1 {
    pub tenant_id: TenantId,
    pub installation_id: InstallationId,
    pub installation_authority_revision: NonZeroU64,
    pub binding_revision: BindingRevision,
    pub binding_fingerprint: ResourceBindingFingerprint,
}

impl RuntimeBindingPinV1 {
    pub fn matches_scope(&self, scope: &RuntimeDeploymentScopeV1) -> bool {
        self.tenant_id == scope.tenant_id && self.installation_id == scope.installation_id
    }

    pub fn matches_target(&self, target: &RuntimeDeploymentTargetV1) -> bool {
        self.binding_revision == target.binding_revision
            && self.binding_fingerprint == target.binding_fingerprint
    }

    pub fn matches(
        &self,
        scope: &RuntimeDeploymentScopeV1,
        target: &RuntimeDeploymentTargetV1,
    ) -> bool {
        self.matches_scope(scope) && self.matches_target(target)
    }
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, DeploymentId, InstallationId, RuntimeDeploymentTargetV1, TenantId,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::RuntimeBindingPinV1;
    use crate::RuntimeDeploymentScopeV1;

    fn pin() -> RuntimeBindingPinV1 {
        RuntimeBindingPinV1 {
            tenant_id: TenantId::parse("tenant").unwrap(),
            installation_id: InstallationId::parse("installation").unwrap(),
            installation_authority_revision: std::num::NonZeroU64::new(7).unwrap(),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        }
    }

    fn scope() -> RuntimeDeploymentScopeV1 {
        RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse("tenant").unwrap(),
            installation_id: InstallationId::parse("installation").unwrap(),
            deployment_id: DeploymentId::parse("deployment").unwrap(),
        }
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(1),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        }
    }

    #[test]
    fn binding_pin_matches_only_the_exact_scope_and_target_authority() {
        let pin = pin();
        let scope = scope();
        let target = target();

        assert!(pin.matches(&scope, &target));

        let mut wrong_scope = scope.clone();
        wrong_scope.installation_id = InstallationId::parse("other").unwrap();
        assert!(!pin.matches(&wrong_scope, &target));

        let mut wrong_target = target.clone();
        wrong_target.binding_revision = BindingRevision::new(4).unwrap();
        assert!(!pin.matches(&scope, &wrong_target));
    }

    #[test]
    fn binding_pin_wire_contract_is_canonical_and_strict() {
        let pin = pin();
        let encoded = serde_json::to_string(&pin).unwrap();

        assert_eq!(
            encoded,
            concat!(
                "{\"tenant_id\":\"tenant\",\"installation_id\":\"installation\",",
                "\"installation_authority_revision\":7,\"binding_revision\":3,",
                "\"binding_fingerprint\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}"
            )
        );
        assert_eq!(
            serde_json::from_str::<RuntimeBindingPinV1>(&encoded).unwrap(),
            pin
        );
    }

    #[test]
    fn binding_pin_wire_contract_rejects_unknown_or_invalid_authority() {
        let encoded = serde_json::to_value(pin()).unwrap();

        let mut unknown = encoded.clone();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<RuntimeBindingPinV1>(unknown).is_err());

        let mut zero = encoded.clone();
        zero.as_object_mut().unwrap().insert(
            "installation_authority_revision".to_string(),
            serde_json::json!(0),
        );
        assert!(serde_json::from_value::<RuntimeBindingPinV1>(zero).is_err());

        let mut fingerprint = encoded;
        fingerprint.as_object_mut().unwrap().insert(
            "binding_fingerprint".to_string(),
            serde_json::json!("A".repeat(64)),
        );
        assert!(serde_json::from_value::<RuntimeBindingPinV1>(fingerprint).is_err());
    }
}
