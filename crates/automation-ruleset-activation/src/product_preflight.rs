use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroU64;

use automation_ruleset::{ExpectedActiveRuleSet, RuleSetVersion, RuleSetVersionIdentity};
use automation_ruleset_readiness::{
    check_readiness, GuildCapabilities, ReadinessError, RuleSetReadinessInput,
};
use desired_state::ResourceKey;
use discord_model::Permissions;
use resource_resolution::{
    approval_binding_fingerprint_v1, project_required_bindings, ResourceBindingMap,
};

use crate::{
    ActivationTarget, ApprovalBindingContextV1, ExpectedActiveBaselineV1, ProductApprovalContextV1,
    SupersessionReasonV1,
};

pub struct ActivationEnvironment {
    pub binding_revision: Option<NonZeroU64>,
    pub bindings: ResourceBindingMap,
    pub guild_capabilities: GuildCapabilities,
    pub role_permissions: BTreeMap<ResourceKey, Permissions>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductPreflightErrorCodeV1 {
    TargetCorrupt,
    BindingRevisionUnavailable,
    UnsupportedSchema,
    StructurallyInvalid,
    HashComputationFailed,
    HashMismatch,
    BindingInvalid,
    BlockingPolicy,
    MissingCapabilities,
}

#[derive(Clone)]
pub(crate) enum ProductPreflightCauseV1 {
    TargetCorrupt,
    BindingRevisionUnavailable,
    Readiness(Box<ReadinessError>),
}

#[derive(Clone)]
pub struct ProductPreflightErrorV1 {
    cause: ProductPreflightCauseV1,
}

impl ProductPreflightErrorV1 {
    fn target_corrupt() -> Self {
        Self {
            cause: ProductPreflightCauseV1::TargetCorrupt,
        }
    }

    fn binding_revision_unavailable() -> Self {
        Self {
            cause: ProductPreflightCauseV1::BindingRevisionUnavailable,
        }
    }

    fn from_readiness(error: ReadinessError) -> Self {
        Self {
            cause: ProductPreflightCauseV1::Readiness(Box::new(error)),
        }
    }

    pub fn code(&self) -> ProductPreflightErrorCodeV1 {
        match &self.cause {
            ProductPreflightCauseV1::TargetCorrupt => ProductPreflightErrorCodeV1::TargetCorrupt,
            ProductPreflightCauseV1::BindingRevisionUnavailable => {
                ProductPreflightErrorCodeV1::BindingRevisionUnavailable
            }
            ProductPreflightCauseV1::Readiness(error) => match error.as_ref() {
                ReadinessError::UnsupportedSchema(_) => {
                    ProductPreflightErrorCodeV1::UnsupportedSchema
                }
                ReadinessError::StructurallyInvalid(_) => {
                    ProductPreflightErrorCodeV1::StructurallyInvalid
                }
                ReadinessError::HashComputation(_) => {
                    ProductPreflightErrorCodeV1::HashComputationFailed
                }
                ReadinessError::HashMismatch => ProductPreflightErrorCodeV1::HashMismatch,
                ReadinessError::BindingInvalid(_) => ProductPreflightErrorCodeV1::BindingInvalid,
                ReadinessError::BlockingPolicy(_) => ProductPreflightErrorCodeV1::BlockingPolicy,
                ReadinessError::MissingCapabilities { .. } => {
                    ProductPreflightErrorCodeV1::MissingCapabilities
                }
            },
        }
    }

    pub(crate) fn into_cause(self) -> ProductPreflightCauseV1 {
        self.cause
    }
}

impl Debug for ProductPreflightErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPreflightErrorV1")
            .field("code", &self.code())
            .finish()
    }
}

impl Display for ProductPreflightErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self.code() {
            ProductPreflightErrorCodeV1::TargetCorrupt => "product target artifact is corrupt",
            ProductPreflightErrorCodeV1::BindingRevisionUnavailable => {
                "authoritative product binding revision is unavailable"
            }
            ProductPreflightErrorCodeV1::UnsupportedSchema => {
                "product target schema is unsupported"
            }
            ProductPreflightErrorCodeV1::StructurallyInvalid => {
                "product target structure is invalid"
            }
            ProductPreflightErrorCodeV1::HashComputationFailed => {
                "product target hash could not be verified"
            }
            ProductPreflightErrorCodeV1::HashMismatch => {
                "product target hash does not match its content"
            }
            ProductPreflightErrorCodeV1::BindingInvalid => "product target bindings are invalid",
            ProductPreflightErrorCodeV1::BlockingPolicy => {
                "product target violates a blocking policy"
            }
            ProductPreflightErrorCodeV1::MissingCapabilities => {
                "product target requires unavailable capabilities"
            }
        })
    }
}

impl std::error::Error for ProductPreflightErrorV1 {}

impl PartialEq for ProductPreflightErrorV1 {
    fn eq(&self, other: &Self) -> bool {
        self.code() == other.code()
    }
}

impl Eq for ProductPreflightErrorV1 {}

#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedProductTargetV1 {
    target: ActivationTarget,
}

impl ValidatedProductTargetV1 {
    pub fn target(&self) -> &ActivationTarget {
        &self.target
    }
}

impl Debug for ValidatedProductTargetV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedProductTargetV1")
            .field("guild_id", &self.target.guild_id)
            .field("ruleset_key", &self.target.ruleset_key)
            .field("version", &self.target.version)
            .field("content_hash", &self.target.content_hash)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProductTargetReadyV1 {
    target: ValidatedProductTargetV1,
    binding: ApprovalBindingContextV1,
    target_is_active: bool,
    expected_active: ExpectedActiveRuleSet,
}

impl Debug for ProductTargetReadyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductTargetReadyV1")
            .field("target", &self.target)
            .field("target_is_active", &self.target_is_active)
            .field("expected_active", &self.expected_active)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductTargetAssessmentV1 {
    Ready(ProductTargetReadyV1),
    Superseded { reason: SupersessionReasonV1 },
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProductPreflightReadyV1 {
    target: ValidatedProductTargetV1,
    binding: ApprovalBindingContextV1,
    target_is_active: bool,
    expected_active: ExpectedActiveRuleSet,
    activation_notices: Vec<String>,
}

impl ProductPreflightReadyV1 {
    pub fn target(&self) -> &ValidatedProductTargetV1 {
        &self.target
    }

    pub fn binding(&self) -> &ApprovalBindingContextV1 {
        &self.binding
    }

    pub fn target_is_active(&self) -> bool {
        self.target_is_active
    }

    pub fn expected_active(&self) -> &ExpectedActiveRuleSet {
        &self.expected_active
    }

    pub fn activation_notices(&self) -> &[String] {
        &self.activation_notices
    }

    pub fn into_activation_notices(self) -> Vec<String> {
        self.activation_notices
    }
}

impl Debug for ProductPreflightReadyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPreflightReadyV1")
            .field("target", &self.target)
            .field("binding_revision", &self.binding.revision)
            .field("binding_fingerprint", &self.binding.fingerprint)
            .field("target_is_active", &self.target_is_active)
            .field("expected_active", &self.expected_active)
            .field("activation_notice_count", &self.activation_notices.len())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductReadinessAssessmentV1 {
    Ready(ProductPreflightReadyV1),
    Superseded { reason: SupersessionReasonV1 },
}

pub fn validate_product_target_v1(
    target: &ActivationTarget,
    artifact: &RuleSetVersion,
) -> Result<ValidatedProductTargetV1, ProductPreflightErrorV1> {
    if artifact.guild_id != target.guild_id
        || artifact.ruleset_key != target.ruleset_key
        || artifact.version != target.version
        || artifact.content_hash != target.content_hash
    {
        return Err(ProductPreflightErrorV1::target_corrupt());
    }
    Ok(ValidatedProductTargetV1 {
        target: target.clone(),
    })
}

pub fn assess_product_target_v1(
    target: ValidatedProductTargetV1,
    context: &ProductApprovalContextV1,
    active: Option<&RuleSetVersionIdentity>,
) -> ProductTargetAssessmentV1 {
    let target_is_active = active.is_some_and(|active| {
        active.version == target.target.version && active.content_hash == target.target.content_hash
    });
    let observed = product_active_baseline_v1(active);
    if !target_is_active && observed != context.baseline {
        ProductTargetAssessmentV1::Superseded {
            reason: SupersessionReasonV1::ActiveBaselineDrift {
                expected: context.baseline.clone(),
                observed,
            },
        }
    } else {
        ProductTargetAssessmentV1::Ready(ProductTargetReadyV1 {
            target,
            binding: context.binding.clone(),
            target_is_active,
            expected_active: expected_active_ruleset(&context.baseline),
        })
    }
}

pub fn check_product_readiness_v1(
    target: ProductTargetReadyV1,
    artifact: &RuleSetVersion,
    environment: &ActivationEnvironment,
) -> Result<ProductReadinessAssessmentV1, ProductPreflightErrorV1> {
    validate_product_target_v1(target.target.target(), artifact)?;
    if let Some(reason) = product_binding_drift(
        target.target.target().guild_id,
        &target.binding,
        environment,
    )? {
        return Ok(ProductReadinessAssessmentV1::Superseded { reason });
    }
    let runtime = check_readiness(RuleSetReadinessInput {
        artifact,
        bindings: &environment.bindings,
        guild_capabilities: &environment.guild_capabilities,
        role_permissions: &environment.role_permissions,
    })
    .map_err(ProductPreflightErrorV1::from_readiness)?;
    Ok(ProductReadinessAssessmentV1::Ready(
        ProductPreflightReadyV1 {
            target: target.target,
            binding: target.binding,
            target_is_active: target.target_is_active,
            expected_active: target.expected_active,
            activation_notices: runtime
                .notices
                .into_iter()
                .map(|notice| format!("{notice:?}"))
                .collect(),
        },
    ))
}

pub fn product_active_baseline_v1(
    active: Option<&RuleSetVersionIdentity>,
) -> ExpectedActiveBaselineV1 {
    match active {
        Some(active) => ExpectedActiveBaselineV1::Exact {
            version: active.version,
            content_hash: active.content_hash,
        },
        None => ExpectedActiveBaselineV1::Absent,
    }
}

fn expected_active_ruleset(baseline: &ExpectedActiveBaselineV1) -> ExpectedActiveRuleSet {
    match baseline {
        ExpectedActiveBaselineV1::Absent => ExpectedActiveRuleSet::Absent,
        ExpectedActiveBaselineV1::Exact {
            version,
            content_hash,
        } => ExpectedActiveRuleSet::Exact {
            identity: RuleSetVersionIdentity {
                version: *version,
                content_hash: *content_hash,
            },
        },
    }
}

fn product_binding_drift(
    guild_id: discord_model::GuildId,
    binding: &ApprovalBindingContextV1,
    environment: &ActivationEnvironment,
) -> Result<Option<SupersessionReasonV1>, ProductPreflightErrorV1> {
    let observed_revision = environment
        .binding_revision
        .ok_or_else(ProductPreflightErrorV1::binding_revision_unavailable)?;
    let observed_bindings =
        project_required_bindings(&binding.required_bindings, &environment.bindings).ok();
    let observed_fingerprint = observed_bindings.as_ref().and_then(|bindings| {
        approval_binding_fingerprint_v1(guild_id, observed_revision, bindings).ok()
    });
    if observed_revision == binding.revision
        && observed_bindings.as_ref() == Some(&binding.required_bindings)
        && observed_fingerprint.as_ref() == Some(&binding.fingerprint)
    {
        Ok(None)
    } else {
        Ok(Some(SupersessionReasonV1::BindingDrift {
            expected_revision: binding.revision,
            observed_revision,
            expected_fingerprint: binding.fingerprint.clone(),
            observed_fingerprint,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::{NonZeroU32, NonZeroU64};

    use automation_ruleset::{
        content_hash, RuleSetKey, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
    };
    use automation_ruleset_readiness::GuildCapabilities;
    use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
    use discord_model::{GuildId, Permissions, UserId};
    use resource_resolution::{approval_binding_fingerprint_v1, ResourceBindingMap};

    use super::*;
    use crate::{
        approval_policy_digest_v1, ActivationDigest, ActivationPromotionId,
        ApprovalBindingContextV1, ApprovalPolicyBindingV1,
    };

    const GUILD: GuildId = GuildId(7);

    fn definition(create_role: bool) -> InteractionRuleSet {
        let mut actions = vec![ActionSpec::DeferEphemeral];
        if create_role {
            actions.push(ActionSpec::CreateRole {
                key: "sensitive_member_key".to_string(),
                name: "sensitive member name".to_string(),
            });
        }
        actions.push(ActionSpec::EditResponse {
            content: "sensitive response".to_string(),
        });
        InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: vec![InteractionRule {
                key: "sensitive_rule_key".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "apply".to_string(),
                },
                actions,
            }],
        }
    }

    fn artifact(create_role: bool) -> RuleSetVersion {
        let definition = definition(create_role);
        RuleSetVersion {
            guild_id: GUILD,
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            schema_version: CURRENT_RULESET_SCHEMA_VERSION,
            content_hash: content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap(),
            definition,
            created_by: UserId(1),
        }
    }

    fn target(artifact: &RuleSetVersion) -> ActivationTarget {
        ActivationTarget {
            guild_id: artifact.guild_id,
            ruleset_key: artifact.ruleset_key.clone(),
            version: artifact.version,
            content_hash: artifact.content_hash,
        }
    }

    fn digest(value: char) -> ActivationDigest {
        ActivationDigest::parse(&value.to_string().repeat(64)).unwrap()
    }

    fn context() -> ProductApprovalContextV1 {
        let binding_revision = NonZeroU64::new(3).unwrap();
        let policy_revision = NonZeroU64::new(7).unwrap();
        let required_approvals = NonZeroU32::new(1).unwrap();
        let ttl_seconds = NonZeroU64::new(1_800).unwrap();
        ProductApprovalContextV1 {
            promotion_id: ActivationPromotionId::parse(&"a".repeat(64)).unwrap(),
            promotion_request_digest: digest('b'),
            approval_payload_digest: digest('c'),
            approval_context_digest: digest('d'),
            binding: ApprovalBindingContextV1 {
                revision: binding_revision,
                required_bindings: Vec::new(),
                fingerprint: approval_binding_fingerprint_v1(GUILD, binding_revision, &[]).unwrap(),
            },
            baseline: ExpectedActiveBaselineV1::Absent,
            policy: ApprovalPolicyBindingV1 {
                revision: policy_revision,
                required_approvals,
                ttl_seconds,
                digest: approval_policy_digest_v1(policy_revision, required_approvals, ttl_seconds),
            },
        }
    }

    fn environment(
        revision: Option<NonZeroU64>,
        permissions: Permissions,
    ) -> ActivationEnvironment {
        ActivationEnvironment {
            binding_revision: revision,
            bindings: ResourceBindingMap::default(),
            guild_capabilities: GuildCapabilities {
                base_permissions: permissions,
            },
            role_permissions: BTreeMap::new(),
        }
    }

    fn target_ready(
        artifact: &RuleSetVersion,
        context: &ProductApprovalContextV1,
        active: Option<&RuleSetVersionIdentity>,
    ) -> ProductTargetReadyV1 {
        let validated = validate_product_target_v1(&target(artifact), artifact).unwrap();
        match assess_product_target_v1(validated, context, active) {
            ProductTargetAssessmentV1::Ready(ready) => ready,
            ProductTargetAssessmentV1::Superseded { .. } => panic!("target superseded"),
        }
    }

    #[test]
    fn corrupt_target_returns_only_a_stable_redacted_error() {
        let artifact = artifact(false);
        let mut wrong = target(&artifact);
        wrong.version = RuleSetVersionId::new(2).unwrap();

        let error = validate_product_target_v1(&wrong, &artifact).unwrap_err();

        assert_eq!(error.code(), ProductPreflightErrorCodeV1::TargetCorrupt);
        assert_eq!(
            format!("{error:?}"),
            "ProductPreflightErrorV1 { code: TargetCorrupt }"
        );
        assert!(!format!("{error:?}").contains("sensitive"));
    }

    #[test]
    fn baseline_drift_supersedes_before_environment_readiness() {
        let artifact = artifact(false);
        let context = context();
        let competing = RuleSetVersionIdentity {
            version: RuleSetVersionId::new(2).unwrap(),
            content_hash: artifact.content_hash,
        };
        let validated = validate_product_target_v1(&target(&artifact), &artifact).unwrap();

        let assessment = assess_product_target_v1(validated, &context, Some(&competing));

        assert!(matches!(
            assessment,
            ProductTargetAssessmentV1::Superseded {
                reason: SupersessionReasonV1::ActiveBaselineDrift { .. }
            }
        ));
    }

    #[test]
    fn exact_active_target_bypasses_baseline_drift_but_keeps_readiness() {
        let artifact = artifact(false);
        let context = context();
        let active = RuleSetVersionIdentity::from(&artifact);
        let ready = target_ready(&artifact, &context, Some(&active));

        let assessment = check_product_readiness_v1(
            ready,
            &artifact,
            &environment(NonZeroU64::new(3), Permissions::ADMINISTRATOR),
        )
        .unwrap();

        assert!(matches!(
            assessment,
            ProductReadinessAssessmentV1::Ready(ready) if ready.target_is_active()
        ));
    }

    #[test]
    fn missing_binding_revision_fails_closed_with_a_stable_code() {
        let artifact = artifact(false);
        let context = context();
        let ready = target_ready(&artifact, &context, None);

        let error = check_product_readiness_v1(
            ready,
            &artifact,
            &environment(None, Permissions::ADMINISTRATOR),
        )
        .unwrap_err();

        assert_eq!(
            error.code(),
            ProductPreflightErrorCodeV1::BindingRevisionUnavailable
        );
        assert!(!format!("{error:?}").contains("sensitive"));
    }

    #[test]
    fn binding_revision_drift_is_a_typed_supersession() {
        let artifact = artifact(false);
        let context = context();
        let ready = target_ready(&artifact, &context, None);

        let assessment = check_product_readiness_v1(
            ready,
            &artifact,
            &environment(NonZeroU64::new(4), Permissions::ADMINISTRATOR),
        )
        .unwrap();

        assert!(matches!(
            assessment,
            ProductReadinessAssessmentV1::Superseded {
                reason: SupersessionReasonV1::BindingDrift {
                    expected_revision,
                    observed_revision,
                    ..
                }
            } if expected_revision.get() == 3 && observed_revision.get() == 4
        ));
    }

    #[test]
    fn readiness_details_are_private_and_errors_expose_only_stable_codes() {
        let artifact = artifact(true);
        let context = context();
        let ready = target_ready(&artifact, &context, None);

        let error = check_product_readiness_v1(
            ready,
            &artifact,
            &environment(NonZeroU64::new(3), Permissions::SEND_MESSAGES),
        )
        .unwrap_err();

        assert_eq!(
            error.code(),
            ProductPreflightErrorCodeV1::MissingCapabilities
        );
        assert_eq!(
            format!("{error}"),
            "product target requires unavailable capabilities"
        );
        assert!(!format!("{error:?}").contains("sensitive"));
    }

    #[test]
    fn ready_projection_preserves_activation_notices_without_debugging_content() {
        let artifact = artifact(true);
        let context = context();
        let ready = target_ready(&artifact, &context, None);

        let assessment = check_product_readiness_v1(
            ready,
            &artifact,
            &environment(NonZeroU64::new(3), Permissions::ADMINISTRATOR),
        )
        .unwrap();
        let ProductReadinessAssessmentV1::Ready(ready) = assessment else {
            panic!("target superseded");
        };

        assert_eq!(ready.target().target(), &target(&artifact));
        assert_eq!(ready.binding(), &context.binding);
        assert!(!ready.activation_notices().is_empty());
        assert!(!format!("{ready:?}").contains("sensitive"));
    }
}
