use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use automation_instance::{AutomationInstance, InstanceResources};
use automation_runtime_interaction::{
    build_interaction_effect_recovery_correlation_v1, DiscordApplicationIdV1,
    DiscordInteractionIdV1, InteractionEffectChannelIdV1, InteractionEffectCorrelationV1,
    InteractionEffectExpectedPostimageDigestV1, InteractionEffectGuildIdV1,
    InteractionEffectIdentityDigestV1, InteractionEffectInstanceTargetV1,
    InteractionEffectMessageIdV1, InteractionEffectPayloadDigestV1,
    InteractionEffectPermissionTargetV1, InteractionEffectPermissionValueV1,
    InteractionEffectPlannedIdentityDigestV1, InteractionEffectRoleIdV1,
    InteractionEffectRoleMembershipTargetV1, InteractionEffectUserIdV1,
    InteractionReceiptIdentityV1,
};
use static_assertions::assert_not_impl_any;

use super::*;

fn hex(value: char) -> String {
    value.to_string().repeat(64)
}

fn planned_digest() -> InteractionEffectPlannedIdentityDigestV1 {
    InteractionEffectPlannedIdentityDigestV1::parse(hex('a')).unwrap()
}

fn expected_digest() -> InteractionEffectExpectedPostimageDigestV1 {
    InteractionEffectExpectedPostimageDigestV1::parse(hex('c')).unwrap()
}

fn guild() -> InteractionEffectGuildIdV1 {
    InteractionEffectGuildIdV1::new(11).unwrap()
}

fn ruleset_key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

fn correlation(class: InteractionEffectCorrelationClassV1) -> InteractionEffectCorrelationV1 {
    build_interaction_effect_recovery_correlation_v1(&planned_digest(), class)
}

fn binding(
    target: InteractionEffectRecoveryTargetV1,
    preimage: InteractionEffectPreimageV1,
    class: InteractionEffectCorrelationClassV1,
) -> InteractionEffectRecoveryBindingV1 {
    InteractionEffectRecoveryBindingV1::new(
        target,
        preimage,
        planned_digest(),
        InteractionEffectIdentityDigestV1::parse(hex('b')).unwrap(),
        expected_digest(),
        correlation(class),
    )
    .unwrap()
}

fn plain_definition(
    binding: InteractionEffectRecoveryBindingV1,
) -> RuntimeInteractionEffectRecoveryDefinitionV1 {
    RuntimeInteractionEffectRecoveryDefinitionV1::new(binding, None, None, ruleset_key()).unwrap()
}

fn create_role_binding() -> InteractionEffectRecoveryBindingV1 {
    binding(
        InteractionEffectRecoveryTargetV1::CreateRole { guild_id: guild() },
        InteractionEffectPreimageV1::None,
        InteractionEffectCorrelationClassV1::AuditLogReason,
    )
}

fn create_channel_binding() -> InteractionEffectRecoveryBindingV1 {
    binding(
        InteractionEffectRecoveryTargetV1::CreateChannel { guild_id: guild() },
        InteractionEffectPreimageV1::None,
        InteractionEffectCorrelationClassV1::AuditLogReason,
    )
}

fn role_membership_target() -> InteractionEffectRoleMembershipTargetV1 {
    InteractionEffectRoleMembershipTargetV1::new(
        guild(),
        InteractionEffectUserIdV1::new(12).unwrap(),
        InteractionEffectRoleIdV1::new(13).unwrap(),
    )
}

fn grant_role_binding() -> InteractionEffectRecoveryBindingV1 {
    let target = role_membership_target();
    binding(
        InteractionEffectRecoveryTargetV1::GrantRole { target },
        InteractionEffectPreimageV1::RoleMembership {
            target,
            present: false,
        },
        InteractionEffectCorrelationClassV1::AuditLogReason,
    )
}

fn permission_target() -> InteractionEffectPermissionTargetV1 {
    InteractionEffectPermissionTargetV1::new(
        guild(),
        InteractionEffectChannelIdV1::new(14).unwrap(),
        InteractionEffectOverwriteTargetV1::Role(InteractionEffectRoleIdV1::new(15).unwrap()),
    )
}

fn permission_value() -> InteractionEffectPermissionValueV1 {
    InteractionEffectPermissionValueV1::new(1, 2).unwrap()
}

fn overwrite_binding() -> InteractionEffectRecoveryBindingV1 {
    let target = permission_target();
    binding(
        InteractionEffectRecoveryTargetV1::UpsertOverwrite {
            target,
            desired: permission_value(),
        },
        InteractionEffectPreimageV1::PermissionOverwrite {
            target,
            before: InteractionEffectPermissionStateV1::Absent,
        },
        InteractionEffectCorrelationClassV1::AuditLogReason,
    )
}

fn post_panel_binding() -> InteractionEffectRecoveryBindingV1 {
    binding(
        InteractionEffectRecoveryTargetV1::PostPanel {
            guild_id: guild(),
            channel_id: InteractionEffectChannelIdV1::new(16).unwrap(),
            payload_digest: InteractionEffectPayloadDigestV1::parse(hex('d')).unwrap(),
        },
        InteractionEffectPreimageV1::None,
        InteractionEffectCorrelationClassV1::Unsupported,
    )
}

fn edit_response_binding() -> InteractionEffectRecoveryBindingV1 {
    binding(
        InteractionEffectRecoveryTargetV1::EditResponse {
            receipt_identity: InteractionReceiptIdentityV1::new(
                DiscordApplicationIdV1::new(17).unwrap(),
                DiscordInteractionIdV1::new(18).unwrap(),
            ),
            payload_digest: InteractionEffectPayloadDigestV1::parse(hex('e')).unwrap(),
        },
        InteractionEffectPreimageV1::None,
        InteractionEffectCorrelationClassV1::InteractionReceipt,
    )
}

fn instance_parts() -> (
    InstanceId,
    InteractionEffectInstanceTargetV1,
    RuntimeInteractionEffectInstanceRegistrationIdentityV1,
) {
    let instance_id = InstanceId::parse("room-1").unwrap();
    let planned = InteractionEffectPlannedInstanceTargetV1::new(guild(), instance_id.clone());
    let target =
        InteractionEffectInstanceTargetV1::new(guild(), planned.instance_identity_digest().clone());
    let registration = RuntimeInteractionEffectInstanceRegistrationIdentityV1::new(
        ruleset_key(),
        InstanceRuleSetVersion::new(7).unwrap(),
        InstanceKind("study_room".to_string()),
        UserId(19),
        InteractionInstanceManifestDigestV1::parse(hex('1')).unwrap(),
    );
    (instance_id, target, registration)
}

fn register_binding() -> InteractionEffectRecoveryBindingV1 {
    let (_, target, registration) = instance_parts();
    binding(
        InteractionEffectRecoveryTargetV1::RegisterInstance {
            target: target.clone(),
            kind: registration.kind().clone(),
            manifest_digest: InteractionEffectPayloadDigestV1::parse(hex('2')).unwrap(),
        },
        InteractionEffectPreimageV1::InstanceRegistration {
            target,
            before: InteractionEffectInstanceStateV1::Absent,
        },
        InteractionEffectCorrelationClassV1::InternalIdempotencyKey,
    )
}

fn register_definition() -> RuntimeInteractionEffectRecoveryDefinitionV1 {
    let (instance_id, _, registration) = instance_parts();
    RuntimeInteractionEffectRecoveryDefinitionV1::new(
        register_binding(),
        Some(instance_id),
        Some(registration),
        ruleset_key(),
    )
    .unwrap()
}

fn teardown_binding() -> InteractionEffectRecoveryBindingV1 {
    let (instance_id, target, _) = instance_parts();
    let planned = InteractionEffectPlannedInstanceTargetV1::new(guild(), instance_id);
    assert_eq!(
        planned.instance_identity_digest(),
        target.instance_identity_digest()
    );
    binding(
        InteractionEffectRecoveryTargetV1::TeardownInstance {
            target: target.clone(),
        },
        InteractionEffectPreimageV1::InstanceRegistration {
            target,
            before: InteractionEffectInstanceStateV1::Present {
                manifest_digest: InteractionEffectPayloadDigestV1::parse(hex('1')).unwrap(),
            },
        },
        InteractionEffectCorrelationClassV1::InternalIdempotencyKey,
    )
}

fn exact_evidence(
    class: InteractionEffectCorrelationClassV1,
) -> DiscordEffectObservationEvidenceV1 {
    DiscordEffectObservationEvidenceV1::new(class, 1, 0, true, true, true)
}

fn known_failure(
    class: InteractionEffectKnownFailureClassV1,
    status: u16,
) -> InteractionEffectKnownFailureV1 {
    InteractionEffectKnownFailureV1::new(class, Some(status)).unwrap()
}

#[test]
fn definition_requires_full_registration_identity_and_separates_digest_domains() {
    let definition = register_definition();
    let registration = definition.registration_identity().unwrap();
    let InteractionEffectRecoveryTargetV1::RegisterInstance {
        manifest_digest, ..
    } = definition.binding().target()
    else {
        panic!()
    };
    assert_ne!(
        manifest_digest.as_str(),
        registration.resolved_instance_manifest_digest().as_str()
    );
    let (instance_id, _, identity) = instance_parts();
    assert_eq!(
        RuntimeInteractionEffectRecoveryDefinitionV1::new(
            register_binding(),
            Some(instance_id.clone()),
            None,
            ruleset_key(),
        ),
        Err(RuntimeInteractionEffectRecoveryDefinitionErrorV1::MissingRegistrationIdentity)
    );
    let mut wrong_kind = identity.clone();
    wrong_kind.kind = InstanceKind("other".to_string());
    assert_eq!(
        RuntimeInteractionEffectRecoveryDefinitionV1::new(
            register_binding(),
            Some(instance_id.clone()),
            Some(wrong_kind),
            ruleset_key(),
        ),
        Err(RuntimeInteractionEffectRecoveryDefinitionErrorV1::RegistrationIdentityMismatch)
    );
    assert_eq!(
        RuntimeInteractionEffectRecoveryDefinitionV1::new(
            create_role_binding(),
            None,
            Some(identity),
            ruleset_key(),
        ),
        Err(RuntimeInteractionEffectRecoveryDefinitionErrorV1::UnexpectedRegistrationIdentity)
    );
    assert!(RuntimeInteractionEffectRecoveryDefinitionV1::new(
        register_binding(),
        Some(InstanceId::parse("other").unwrap()),
        Some(instance_parts().2),
        ruleset_key(),
    )
    .is_err());
}

#[test]
fn discord_routes_cover_every_mutation_and_panel_success_is_never_adopted() {
    let role = create_role_binding();
    assert!(matches!(
        discord_observation_request_v1(&role, UserId(20)),
        Some(RuntimeInteractionEffectDiscordObservationRequestV1::CreateRole { .. })
    ));
    let channel = create_channel_binding();
    assert!(matches!(
        discord_observation_request_v1(&channel, UserId(20)),
        Some(RuntimeInteractionEffectDiscordObservationRequestV1::CreateChannel { .. })
    ));
    let grant = grant_role_binding();
    assert!(matches!(
        discord_observation_request_v1(&grant, UserId(20)),
        Some(RuntimeInteractionEffectDiscordObservationRequestV1::GrantRole { .. })
    ));
    let overwrite = overwrite_binding();
    assert!(matches!(
        discord_observation_request_v1(&overwrite, UserId(20)),
        Some(RuntimeInteractionEffectDiscordObservationRequestV1::UpsertOverwrite { .. })
    ));
    let panel = post_panel_binding();
    assert!(matches!(
        discord_observation_request_v1(&panel, UserId(20)),
        Some(RuntimeInteractionEffectDiscordObservationRequestV1::PostPanel { .. })
    ));
    let adopted = map_discord_observation_v1(
        &role,
        DiscordEffectObservationOutcomeV1::ExactMatch {
            output: RuntimeInteractionEffectDiscordObservedOutputV1::CreatedRole(RoleId(21)),
            evidence: exact_evidence(InteractionEffectCorrelationClassV1::AuditLogReason),
        },
    );
    assert!(matches!(
        adopted,
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        )
    ));
    assert!(interaction_output_v1(
        &channel,
        RuntimeInteractionEffectDiscordObservedOutputV1::CreatedChannel(ChannelId(22)),
    )
    .is_some());
    assert!(interaction_output_v1(
        &grant,
        RuntimeInteractionEffectDiscordObservedOutputV1::RoleMembership(true),
    )
    .is_some());
    assert!(interaction_output_v1(
        &overwrite,
        RuntimeInteractionEffectDiscordObservedOutputV1::PermissionOverwrite(
            InteractionEffectPermissionStateV1::Present(permission_value()),
        ),
    )
    .is_some());
    let panel_adoption = map_discord_observation_v1(
        &panel,
        DiscordEffectObservationOutcomeV1::ExactMatch {
            output: RuntimeInteractionEffectDiscordObservedOutputV1::PostedPanel(MessageId(22)),
            evidence: exact_evidence(InteractionEffectCorrelationClassV1::Unsupported),
        },
    );
    assert_eq!(
        panel_adoption,
        RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
            RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
        )
    );
}

#[test]
fn observation_failures_have_stable_safe_dispositions() {
    assert_eq!(
        map_discord_read_failure_v1(
            DiscordEffectReadFailureV1::KnownFailed(known_failure(
                InteractionEffectKnownFailureClassV1::Forbidden,
                403,
            )),
            false,
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::RouteBlocked(
            RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden,
        )
    );
    assert_eq!(
        map_discord_read_failure_v1(
            DiscordEffectReadFailureV1::KnownFailed(known_failure(
                InteractionEffectKnownFailureClassV1::Forbidden,
                403,
            )),
            true,
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
            RuntimeInteractionEffectRecoveryRequiredV1::ResponseTokenUnavailable,
        )
    );
    assert!(matches!(
        map_discord_read_failure_v1(
            DiscordEffectReadFailureV1::KnownFailed(known_failure(
                InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch,
                429,
            )),
            false,
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(_)
    ));
    for class in [
        InteractionEffectIndeterminateClassV1::DeadlineElapsed,
        InteractionEffectIndeterminateClassV1::MalformedResponse,
        InteractionEffectIndeterminateClassV1::ConnectionLost,
    ] {
        assert!(matches!(
            map_discord_read_failure_v1(DiscordEffectReadFailureV1::Indeterminate(class), false,),
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(_)
        ));
    }
}

#[test]
fn compensation_attempt_and_observation_statuses_are_complete() {
    let binding = create_role_binding();
    assert!(matches!(
        map_compensation_attempt_v1(&binding, DiscordEffectAttemptOutcomeV1::KnownSucceeded(())),
        RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
            InteractionEffectCompensationOutcomeV1::Succeeded { .. }
        )
    ));
    assert_eq!(
        map_compensation_attempt_v1(
            &binding,
            DiscordEffectAttemptOutcomeV1::KnownFailed(known_failure(
                InteractionEffectKnownFailureClassV1::Forbidden,
                403,
            )),
        ),
        RuntimeInteractionEffectRecoveryCompensationDispositionV1::RouteBlocked(
            RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden,
        )
    );
    assert!(matches!(
        map_compensation_attempt_v1(
            &binding,
            DiscordEffectAttemptOutcomeV1::KnownFailed(known_failure(
                InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch,
                429,
            )),
        ),
        RuntimeInteractionEffectRecoveryCompensationDispositionV1::Deferred(_)
    ));
    assert!(matches!(
        map_compensation_attempt_v1(
            &binding,
            DiscordEffectAttemptOutcomeV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::ConnectionLost,
            ),
        ),
        RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
            InteractionEffectCompensationOutcomeV1::Indeterminate(_)
        )
    ));
    let evidence = exact_evidence(InteractionEffectCorrelationClassV1::AuditLogReason);
    assert!(matches!(
        map_discord_compensation_observation_v1(
            &binding,
            DiscordEffectCompensationObservationOutcomeV1::Restored { evidence },
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Restored { .. }
        )
    ));
    assert!(matches!(
        map_discord_compensation_observation_v1(
            &binding,
            DiscordEffectCompensationObservationOutcomeV1::Pending { evidence },
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Pending { .. }
        )
    ));
    assert!(matches!(
        map_discord_compensation_observation_v1(
            &binding,
            DiscordEffectCompensationObservationOutcomeV1::Conflict { evidence },
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Conflict { .. }
        )
    ));
    assert!(matches!(
        map_discord_compensation_observation_v1(
            &binding,
            DiscordEffectCompensationObservationOutcomeV1::Unsupported { evidence },
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Unsupported { .. }
        )
    ));
}

#[test]
fn internal_observation_and_compensation_distinguish_preimage_current_and_conflict() {
    let definition = register_definition();
    let current = expected_instance_postimage_v1(definition.binding()).unwrap();
    let preimage = instance_preimage_v1(definition.binding()).unwrap();
    assert!(matches!(
        map_internal_observation_v1(
            &definition,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(current.clone()),
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        )
    ));
    assert!(matches!(
        map_internal_observation_v1(
            &definition,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(preimage.clone()),
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactAbsence { .. }
        )
    ));
    let third = InteractionEffectInstanceStateV1::Present {
        manifest_digest: InteractionEffectPayloadDigestV1::parse(hex('3')).unwrap(),
    };
    assert!(matches!(
        map_internal_observation_v1(
            &definition,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(third.clone()),
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::Conflict { .. }
        )
    ));
    assert!(matches!(
        map_internal_compensation_observation_v1(
            &definition,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(preimage),
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Restored { .. }
        )
    ));
    assert!(matches!(
        map_internal_compensation_observation_v1(
            &definition,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(current),
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Pending { .. }
        )
    ));
    assert!(matches!(
        map_internal_compensation_observation_v1(
            &definition,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(third),
        ),
        RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
            InteractionEffectCompensationObservationOutcomeV1::Conflict { .. }
        )
    ));
    let (instance_id, _, _) = instance_parts();
    let teardown = RuntimeInteractionEffectRecoveryDefinitionV1::new(
        teardown_binding(),
        Some(instance_id),
        None,
        ruleset_key(),
    )
    .unwrap();
    assert!(matches!(
        map_internal_observation_v1(
            &teardown,
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(
                InteractionEffectInstanceStateV1::Absent,
            ),
        ),
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        )
    ));
}

#[test]
fn original_response_exact_output_is_strictly_adopted() {
    let binding = edit_response_binding();
    let InteractionEffectRecoveryTargetV1::EditResponse {
        receipt_identity,
        payload_digest,
    } = binding.target().clone()
    else {
        panic!()
    };
    let outcome = map_original_response_observation_v1(
        &binding,
        DiscordOriginalResponseObservationOutcomeV1::ExactMatch {
            output: InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity,
                payload_digest,
            },
            evidence: exact_evidence(InteractionEffectCorrelationClassV1::InteractionReceipt),
        },
    );
    assert!(matches!(
        outcome,
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        )
    ));
}

fn role_output() -> InteractionEffectObservedOutputV1 {
    InteractionEffectObservedOutputV1::CreatedRole {
        guild_id: guild(),
        role_id: InteractionEffectRoleIdV1::new(21).unwrap(),
    }
}

fn channel_output() -> InteractionEffectObservedOutputV1 {
    InteractionEffectObservedOutputV1::CreatedChannel {
        guild_id: guild(),
        channel_id: InteractionEffectChannelIdV1::new(22).unwrap(),
    }
}

fn grant_output() -> InteractionEffectObservedOutputV1 {
    InteractionEffectObservedOutputV1::RoleMembership {
        target: role_membership_target(),
        present: true,
    }
}

fn overwrite_output() -> InteractionEffectObservedOutputV1 {
    InteractionEffectObservedOutputV1::PermissionOverwrite {
        target: permission_target(),
        state: InteractionEffectPermissionStateV1::Present(permission_value()),
    }
}

fn panel_output() -> InteractionEffectObservedOutputV1 {
    let InteractionEffectRecoveryTargetV1::PostPanel {
        guild_id,
        channel_id,
        payload_digest,
    } = post_panel_binding().target().clone()
    else {
        panic!()
    };
    InteractionEffectObservedOutputV1::PostedMessage {
        guild_id,
        channel_id,
        message_id: InteractionEffectMessageIdV1::new(23).unwrap(),
        payload_digest,
    }
}

#[test]
fn compensation_requests_bind_exact_outputs_and_preimages() {
    let cases = [
        (plain_definition(create_role_binding()), role_output()),
        (plain_definition(create_channel_binding()), channel_output()),
        (plain_definition(grant_role_binding()), grant_output()),
        (plain_definition(overwrite_binding()), overwrite_output()),
        (plain_definition(post_panel_binding()), panel_output()),
    ];
    for (definition, output) in cases {
        let request =
            RuntimeInteractionEffectRecoveryCompensationRequestV1::new(definition, output).unwrap();
        assert!(discord_compensation_request_v1(&request, UserId(24)).is_some());
    }
    assert_eq!(
        RuntimeInteractionEffectRecoveryCompensationRequestV1::new(
            plain_definition(create_role_binding()),
            channel_output(),
        ),
        Err(RuntimeInteractionEffectRecoveryCompensationRequestErrorV1::Output)
    );
    let definition = register_definition();
    let current = expected_instance_postimage_v1(definition.binding()).unwrap();
    let target = instance_target_v1(definition.binding()).unwrap();
    let request = RuntimeInteractionEffectRecoveryCompensationRequestV1::new(
        definition.clone(),
        InteractionEffectObservedOutputV1::InstanceState {
            target,
            state: current,
        },
    )
    .unwrap();
    let restore = internal_restore_request_v1(&request).unwrap();
    assert_eq!(
        restore.resolved_instance_manifest_digest(),
        definition.instance_manifest_digest().unwrap()
    );
    assert_eq!(
        restore.registration_identity(),
        definition.registration_identity().unwrap()
    );
    let (instance_id, target, _) = instance_parts();
    let teardown = RuntimeInteractionEffectRecoveryDefinitionV1::new(
        teardown_binding(),
        Some(instance_id),
        None,
        ruleset_key(),
    )
    .unwrap();
    let request = RuntimeInteractionEffectRecoveryCompensationRequestV1::new(
        teardown,
        InteractionEffectObservedOutputV1::InstanceState {
            target,
            state: InteractionEffectInstanceStateV1::Absent,
        },
    )
    .unwrap();
    assert!(internal_restore_request_v1(&request).is_none());
}

#[test]
fn registration_match_rejects_each_automation_instance_identity_drift() {
    let (instance_id, target, expected) = instance_parts();
    let mut instance = AutomationInstance {
        id: instance_id,
        guild_id: GuildId(guild().get()),
        ruleset_key: expected.ruleset_key().as_str().to_string(),
        ruleset_version: expected.ruleset_version(),
        kind: expected.kind().clone(),
        created_by: expected.created_by(),
        resources: InstanceResources::default(),
        status: InstanceStatus::Active,
    };
    let digest = expected.resolved_instance_manifest_digest().clone();
    assert!(instance_registration_matches_v1(
        &instance, &expected, &digest
    ));
    let teardown_without_identity = RuntimeInteractionEffectInstanceObservationRequestV1 {
        target,
        instance_id: instance.id.clone(),
        expected_postimage: InteractionEffectInstanceStateV1::Absent,
        expected_preimage: InteractionEffectInstanceStateV1::Present {
            manifest_digest: InteractionEffectPayloadDigestV1::parse(hex('1')).unwrap(),
        },
        registration_identity: None,
    };
    assert!(!instance_registration_allowed_v1(
        &instance,
        &teardown_without_identity,
        &digest,
    ));
    instance.ruleset_key = "other".to_string();
    assert!(!instance_registration_matches_v1(
        &instance, &expected, &digest
    ));
    instance.ruleset_key = expected.ruleset_key().as_str().to_string();
    instance.ruleset_version = InstanceRuleSetVersion::new(8).unwrap();
    assert!(!instance_registration_matches_v1(
        &instance, &expected, &digest
    ));
    instance.ruleset_version = expected.ruleset_version();
    instance.kind = InstanceKind("other".to_string());
    assert!(!instance_registration_matches_v1(
        &instance, &expected, &digest
    ));
    instance.kind = expected.kind().clone();
    instance.created_by = UserId(999);
    assert!(!instance_registration_matches_v1(
        &instance, &expected, &digest
    ));
    instance.created_by = expected.created_by();
    assert!(!instance_registration_matches_v1(
        &instance,
        &expected,
        &InteractionInstanceManifestDigestV1::parse(hex('4')).unwrap(),
    ));
}

struct FakeDiscord {
    observations: Mutex<
        VecDeque<
            DiscordEffectObservationOutcomeV1<RuntimeInteractionEffectDiscordObservedOutputV1>,
        >,
    >,
    compensations: Mutex<VecDeque<DiscordEffectAttemptOutcomeV1<()>>>,
    compensation_observations: Mutex<VecDeque<DiscordEffectCompensationObservationOutcomeV1>>,
}

impl FakeDiscord {
    fn new(
        observations: Vec<
            DiscordEffectObservationOutcomeV1<RuntimeInteractionEffectDiscordObservedOutputV1>,
        >,
    ) -> Self {
        Self {
            observations: Mutex::new(observations.into()),
            compensations: Mutex::new(VecDeque::new()),
            compensation_observations: Mutex::new(VecDeque::new()),
        }
    }
}

impl RuntimeInteractionEffectDiscordRecoveryPortV1 for FakeDiscord {
    async fn observe_discord_effect_v1(
        &self,
        _request: RuntimeInteractionEffectDiscordObservationRequestV1,
    ) -> DiscordEffectObservationOutcomeV1<RuntimeInteractionEffectDiscordObservedOutputV1> {
        self.observations.lock().unwrap().pop_front().unwrap()
    }

    async fn compensate_discord_effect_v1(
        &self,
        _request: RuntimeInteractionEffectDiscordCompensationRequestV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        self.compensations.lock().unwrap().pop_front().unwrap()
    }

    async fn observe_discord_compensation_v1(
        &self,
        _request: RuntimeInteractionEffectDiscordCompensationRequestV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        self.compensation_observations
            .lock()
            .unwrap()
            .pop_front()
            .unwrap()
    }
}

struct FakeResponse {
    outcomes: Mutex<VecDeque<DiscordOriginalResponseObservationOutcomeV1>>,
    calls: AtomicUsize,
}

impl DiscordOriginalResponseObserverV1 for FakeResponse {
    async fn observe_original_response_v1(
        &self,
        _request: DiscordOriginalResponseObservationRequestV1,
    ) -> DiscordOriginalResponseObservationOutcomeV1 {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.outcomes.lock().unwrap().pop_front().unwrap()
    }
}

struct FakeInternal {
    observations: Mutex<VecDeque<RuntimeInteractionEffectInstanceObservationOutcomeV1>>,
    restores: Mutex<VecDeque<RuntimeInteractionEffectInstanceRestoreOutcomeV1>>,
}

impl RuntimeInteractionEffectInternalRecoveryPortV1 for FakeInternal {
    async fn observe_instance_state_v1(
        &self,
        _request: RuntimeInteractionEffectInstanceObservationRequestV1,
    ) -> RuntimeInteractionEffectInstanceObservationOutcomeV1 {
        self.observations.lock().unwrap().pop_front().unwrap()
    }

    async fn restore_instance_state_v1(
        &self,
        _request: RuntimeInteractionEffectInstanceRestoreRequestV1,
    ) -> RuntimeInteractionEffectInstanceRestoreOutcomeV1 {
        self.restores.lock().unwrap().pop_front().unwrap()
    }
}

#[tokio::test]
async fn executor_routes_discord_response_and_internal_without_exposing_token() {
    let discord = FakeDiscord::new(vec![DiscordEffectObservationOutcomeV1::ExactMatch {
        output: RuntimeInteractionEffectDiscordObservedOutputV1::CreatedRole(RoleId(21)),
        evidence: exact_evidence(InteractionEffectCorrelationClassV1::AuditLogReason),
    }]);
    let response = FakeResponse {
        outcomes: Mutex::new(
            vec![DiscordOriginalResponseObservationOutcomeV1::ExactAbsence {
                evidence: exact_evidence(InteractionEffectCorrelationClassV1::InteractionReceipt),
            }]
            .into(),
        ),
        calls: AtomicUsize::new(0),
    };
    let internal = FakeInternal {
        observations: Mutex::new(
            vec![RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(
                expected_instance_postimage_v1(register_definition().binding()).unwrap(),
            )]
            .into(),
        ),
        restores: Mutex::new(VecDeque::new()),
    };
    let executor =
        RuntimeInteractionEffectRecoveryExecutorV1::new(&discord, &response, &internal, UserId(24));
    let role = executor
        .observe_v1(
            RuntimeInteractionEffectRecoveryObservationRequestV1::new(
                plain_definition(create_role_binding()),
                None,
            )
            .unwrap(),
        )
        .await;
    assert!(matches!(
        role,
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        )
    ));
    let response_request = RuntimeInteractionEffectRecoveryObservationRequestV1::new(
        plain_definition(edit_response_binding()),
        Some(InteractionTokenV1::new("secret-token").unwrap()),
    )
    .unwrap();
    assert!(!format!("{response_request:?}").contains("secret-token"));
    assert!(matches!(
        executor.observe_v1(response_request).await,
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactAbsence { .. }
        )
    ));
    assert_eq!(response.calls.load(Ordering::Acquire), 1);
    assert!(matches!(
        executor
            .observe_v1(
                RuntimeInteractionEffectRecoveryObservationRequestV1::new(
                    register_definition(),
                    None,
                )
                .unwrap(),
            )
            .await,
        RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
            InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        )
    ));
}

#[test]
fn request_shapes_and_debug_are_fail_closed() {
    assert!(matches!(
        RuntimeInteractionEffectRecoveryObservationRequestV1::new(
            plain_definition(create_role_binding()),
            Some(InteractionTokenV1::new("secret").unwrap()),
        ),
        Err(RuntimeInteractionEffectRecoveryObservationRequestErrorV1::TokenShape)
    ));
    assert!(RuntimeInteractionEffectRecoveryObservationRequestV1::new(
        plain_definition(edit_response_binding()),
        None,
    )
    .is_err());
    assert_not_impl_any!(InteractionTokenV1: Clone);
}
