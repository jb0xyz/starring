use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, FencingToken, InstallationId, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionActionPlanDigestV1,
    InteractionEffectActionDigestV1, InteractionEffectActionIdentityV1,
    InteractionEffectActionIndexV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectGuildIdV1, InteractionEffectInputDigestV1, InteractionEffectKindV1,
    InteractionEffectPayloadDigestV1, InteractionEffectPlanDefinitionV1,
    InteractionEffectPlannedPreimageV1, InteractionEffectPlannedRecoveryInputV1,
    InteractionEffectPlannedTargetV1, InteractionExpectedRouteV1,
    InteractionGatewayOwnerIdentityV1, InteractionGatewayOwnerLeaseEpochV1,
    InteractionGatewayOwnerRevisionV1, InteractionGatewayShardIdentityV1,
    InteractionPreflightCertificateDigestV1, InteractionPreflightPlanDigestV1,
    InteractionPreflightSnapshotDigestV1, InteractionProductScopeV1,
    InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1, InteractionRequestDigestV1,
    InteractionRouteAttestationDigestV1, InteractionRouteBindingV1, InteractionRouteIncarnationV1,
    InteractionRuntimeBuildRevisionV1, InteractionServingLeaseEpochV1,
    InteractionServingLeaseRevisionV1, InteractionServingRouteIdentityV1,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use crate::{InteractionActionPreflightCertificateV1, InteractionEffectJournalPlanEntryV1};

pub(crate) fn certificate_v1() -> InteractionActionPreflightCertificateV1 {
    certificate_with_seed_v1(1)
}

pub(crate) fn certificate_with_seed_v1(seed: u64) -> InteractionActionPreflightCertificateV1 {
    let digit = char::from_digit((seed % 10) as u32, 10).unwrap();
    let content_hash = RuleSetContentHash::parse_hex(&digit.to_string().repeat(64)).unwrap();
    let process_identity = RuntimeProcessIdentityV1 {
        target: RuntimeDeploymentTargetV1 {
            guild_id: GuildId(100 + seed),
            ruleset_key: RuleSetKey::parse("effect-contract").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash,
            binding_revision: BindingRevision::new(seed).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        },
        runtime_generation: RuntimeGeneration::new(seed).unwrap(),
        process_instance_id: ProcessInstanceId::parse(format!("effect-contract-process-{seed}"))
            .unwrap(),
    };
    let serving_identity = InteractionServingRouteIdentityV1::new(
        InteractionRouteAttestationDigestV1::parse(digit.to_string().repeat(64)).unwrap(),
        InteractionServingLeaseEpochV1::new(seed).unwrap(),
        InteractionServingLeaseRevisionV1::new(seed).unwrap(),
        InteractionGatewayOwnerIdentityV1::new(
            InteractionGatewayShardIdentityV1::parse(format!("effect-contract-shard-{seed}"))
                .unwrap(),
            InteractionGatewayOwnerLeaseEpochV1::new(seed).unwrap(),
            InteractionGatewayOwnerRevisionV1::new(seed).unwrap(),
            InteractionRuntimeBuildRevisionV1::parse(format!("effect-contract-build-{seed}"))
                .unwrap(),
        ),
        FencingToken::new(seed).unwrap(),
        InteractionRouteIncarnationV1::new(seed).unwrap(),
    );
    let route = InteractionRouteBindingV1::new_static(
        InteractionProductScopeV1::new(
            TenantId::parse(format!("effect-contract-tenant-{seed}")).unwrap(),
            InstallationId::parse(format!("effect-contract-installation-{seed}")).unwrap(),
            DeploymentId::parse(format!("effect-contract-deployment-{seed}")).unwrap(),
        ),
        process_identity,
        serving_identity,
    )
    .unwrap();
    let identity = InteractionReceiptIdentityV1::new(
        DiscordApplicationIdV1::new(10 + seed).unwrap(),
        DiscordInteractionIdV1::new(20 + seed).unwrap(),
    );
    let claim_root = InteractionReceiptClaimCandidateV1::new(
        identity,
        InteractionExpectedRouteV1::from_authoritative(&route),
        InteractionRequestDigestV1::parse(digit.to_string().repeat(64)).unwrap(),
    )
    .bind_authoritative(route)
    .unwrap();
    InteractionActionPreflightCertificateV1::issue(
        &claim_root,
        InteractionActionPlanDigestV1::parse(digit.to_string().repeat(64)).unwrap(),
        InteractionPreflightPlanDigestV1::from_canonical_bytes(
            format!("preflight-plan-{seed}").as_bytes(),
        ),
        InteractionPreflightSnapshotDigestV1::from_canonical_bytes(
            format!("snapshot-{seed}").as_bytes(),
        ),
    )
}

pub(crate) fn create_role_entry_v1(
    certificate: &InteractionActionPreflightCertificateV1,
    index: u16,
) -> InteractionEffectJournalPlanEntryV1 {
    journal_entry_v1(certificate, index, InteractionEffectKindV1::CreateRole)
}

pub(crate) fn edit_response_entry_v1(
    certificate: &InteractionActionPreflightCertificateV1,
    index: u16,
) -> InteractionEffectJournalPlanEntryV1 {
    journal_entry_v1(certificate, index, InteractionEffectKindV1::EditResponse)
}

fn journal_entry_v1(
    certificate: &InteractionActionPreflightCertificateV1,
    index: u16,
    kind: InteractionEffectKindV1,
) -> InteractionEffectJournalPlanEntryV1 {
    journal_entry_with_binding_v1(
        certificate,
        index,
        kind,
        certificate.receipt_identity(),
        certificate.action_plan_digest().clone(),
        certificate.digest().clone(),
    )
}

pub(crate) fn edit_response_entry_with_binding_v1(
    certificate: &InteractionActionPreflightCertificateV1,
    index: u16,
    receipt_identity: InteractionReceiptIdentityV1,
    action_plan_digest: InteractionActionPlanDigestV1,
    certificate_digest: InteractionPreflightCertificateDigestV1,
) -> InteractionEffectJournalPlanEntryV1 {
    journal_entry_with_binding_v1(
        certificate,
        index,
        InteractionEffectKindV1::EditResponse,
        receipt_identity,
        action_plan_digest,
        certificate_digest,
    )
}

fn journal_entry_with_binding_v1(
    certificate: &InteractionActionPreflightCertificateV1,
    index: u16,
    kind: InteractionEffectKindV1,
    receipt_identity: InteractionReceiptIdentityV1,
    action_plan_digest: InteractionActionPlanDigestV1,
    certificate_digest: InteractionPreflightCertificateDigestV1,
) -> InteractionEffectJournalPlanEntryV1 {
    let action = InteractionEffectActionIdentityV1::new(
        receipt_identity,
        action_plan_digest,
        certificate_digest,
        InteractionEffectActionIndexV1::new(index).unwrap(),
        kind,
        InteractionEffectActionDigestV1::from_canonical_bytes(
            format!("action-{index}-{}", kind.code()).as_bytes(),
        ),
        InteractionEffectInputDigestV1::from_canonical_bytes(
            format!("input-{index}-{}", kind.code()).as_bytes(),
        ),
    );
    let target = match kind {
        InteractionEffectKindV1::CreateRole => InteractionEffectPlannedTargetV1::CreateRole {
            guild_id: InteractionEffectGuildIdV1::new(101).unwrap(),
        },
        InteractionEffectKindV1::EditResponse => InteractionEffectPlannedTargetV1::EditResponse {
            receipt_identity: certificate.receipt_identity(),
            payload_digest: InteractionEffectPayloadDigestV1::from_canonical_bytes(
                b"response-payload",
            ),
        },
        _ => unreachable!("test helper only constructs create-role and edit-response effects"),
    };
    let recovery = InteractionEffectPlannedRecoveryInputV1::new(
        target,
        InteractionEffectPlannedPreimageV1::None,
    )
    .unwrap();
    let definition = InteractionEffectPlanDefinitionV1::new(action, recovery, Vec::new()).unwrap();
    InteractionEffectJournalPlanEntryV1::new(
        definition,
        InteractionEffectExpectedPostimageDigestV1::from_canonical_bytes(
            format!("postimage-{index}-{}", kind.code()).as_bytes(),
        ),
    )
}
