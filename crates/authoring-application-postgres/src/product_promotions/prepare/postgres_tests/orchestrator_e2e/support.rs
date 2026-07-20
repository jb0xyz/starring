use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use authoring_application::{
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthorizedInstallationScopeV1,
    AuthorizedPromotionSnapshotError, AuthorizedPromotionSnapshotPort,
    AuthorizedPromotionSnapshotV1, InstallationSelectorV1, MutationAuthenticationPort,
    ProductPromotionIdempotencyKeyV1, PromoteOwnedSessionV1, ResolvedPromotionAuthorityV1,
};
use authoring_application_discord::{
    DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordAuthorityConfigV1,
    DiscordAuthoritySourceError, DiscordGuildAuthorityAdapter, DiscordGuildAuthorityClient,
    DiscordGuildAuthoritySnapshotV1, DiscordRoleSnapshotV1, FreshDiscordAuthorityEvidenceV1,
    InstallationAuthorityRecordV1, InstallationAuthoritySource,
};
use chrono::Utc;
use discord_model::{Permissions, RoleId};

use super::super::*;

pub(super) const ACTIVATION_GATE_CLASS: i32 = 18771;

pub(in crate::product_promotions::prepare::postgres_tests) struct Authentication;

impl authoring_application::AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &str,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if credential != "valid-credential" {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse("principal").unwrap(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest(SESSION_DIGEST),
        ))
    }
}

impl MutationAuthenticationPort for Authentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &str,
        csrf: &str,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if csrf != "valid-csrf" {
            return Err(AuthenticationError::InvalidCsrf);
        }
        self.authenticate(credential).await
    }
}

#[derive(Clone)]
pub(in crate::product_promotions::prepare::postgres_tests) struct AuthoritySource {
    record: InstallationAuthorityRecordV1,
}

impl InstallationAuthoritySource for AuthoritySource {
    async fn load_for_actor(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
    ) -> Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError> {
        assert_eq!(actor.principal_id().as_str(), "principal");
        assert_eq!(actor.session_fingerprint().as_bytes(), &SESSION_DIGEST);
        assert_eq!(installation.installation_id(), &self.record.installation_id);
        Ok(self.record.clone())
    }
}

#[derive(Clone)]
pub(in crate::product_promotions::prepare::postgres_tests) struct AuthorityClient {
    application_id: DiscordApplicationIdV1,
    snapshot: DiscordGuildAuthoritySnapshotV1,
}

impl DiscordGuildAuthorityClient for AuthorityClient {
    fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
        assert_eq!(guild_id, self.snapshot.guild_id);
        assert_eq!(user_id, self.snapshot.member_user_id);
        Ok(self.snapshot.clone())
    }
}

pub(in crate::product_promotions::prepare::postgres_tests) struct Snapshot {
    pub(in crate::product_promotions::prepare::postgres_tests) artifact: PreviewReadyArtifactV1,
    pub(in crate::product_promotions::prepare::postgres_tests) authority:
        ResolvedPromotionAuthorityV1,
    pub(in crate::product_promotions::prepare::postgres_tests) expected_generation:
        SessionGeneration,
    pub(in crate::product_promotions::prepare::postgres_tests) calls: Arc<AtomicUsize>,
}

impl AuthorizedPromotionSnapshotPort<FreshDiscordAuthorityEvidenceV1> for Snapshot {
    async fn load_atomic_authorized_snapshot(
        &self,
        actor: &AuthenticatedActorV1,
        scope: &AuthorizedInstallationScopeV1,
        evidence: &FreshDiscordAuthorityEvidenceV1,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        assert_eq!(actor.principal_id().as_str(), "principal");
        assert_eq!(scope.tenant_id().as_str(), "tenant");
        assert_eq!(scope.installation_id().as_str(), "installation");
        assert_eq!(
            evidence.capability(),
            authoring_application::CapabilityV1::Promote
        );
        assert_eq!(session_id.as_str(), "authoring");
        assert_eq!(expected_generation, self.expected_generation);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(AuthorizedPromotionSnapshotV1::from_atomic_authorization(
            self.artifact.clone(),
            self.authority.clone(),
        ))
    }
}

pub(in crate::product_promotions::prepare::postgres_tests) fn authority_adapter(
    plan: &PreparedPromotionPlanV1,
) -> DiscordGuildAuthorityAdapter<AuthoritySource, AuthorityClient> {
    let context = &plan.intent.authority;
    let application_id = DiscordApplicationIdV1::new(2001).unwrap();
    let role_id = RoleId(context.guild_id.0 + 1);
    DiscordGuildAuthorityAdapter::new(
        AuthoritySource {
            record: InstallationAuthorityRecordV1 {
                tenant_id: context.tenant_id.clone(),
                installation_id: context.installation_id.clone(),
                application_id,
                guild_id: context.guild_id,
                acting_user_id: context.requester,
                authority_revision: NonZeroU64::new(1).unwrap(),
                authority_digest: "5".repeat(64),
            },
        },
        AuthorityClient {
            application_id,
            snapshot: DiscordGuildAuthoritySnapshotV1 {
                guild_id: context.guild_id,
                owner_id: UserId(context.requester.0 + 1),
                member_user_id: context.requester,
                member_is_bot: false,
                member_is_system: false,
                member_pending: false,
                member_role_ids: vec![role_id],
                roles: vec![
                    DiscordRoleSnapshotV1 {
                        role_id: RoleId(context.guild_id.0),
                        permissions: Permissions::VIEW_CHANNEL,
                        position: 0,
                        managed: false,
                    },
                    DiscordRoleSnapshotV1 {
                        role_id,
                        permissions: Permissions::MANAGE_GUILD,
                        position: 1,
                        managed: false,
                    },
                ],
            },
        },
        DiscordAuthorityConfigV1::new(
            Duration::from_secs(2),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap(),
    )
}

pub(in crate::product_promotions::prepare::postgres_tests) fn resolved_authority(
    plan: &PreparedPromotionPlanV1,
) -> ResolvedPromotionAuthorityV1 {
    let context = &plan.intent.authority;
    ResolvedPromotionAuthorityV1 {
        guild_id: context.guild_id,
        installation_id: context.installation_id.clone(),
        ruleset_key: context.ruleset_key.clone(),
        requester: context.requester,
        binding_revision: context.binding_revision,
        policy: context.policy.clone(),
    }
}

pub(in crate::product_promotions::prepare::postgres_tests) fn promotion_command(
    idempotency_key: &str,
    expected_generation: SessionGeneration,
) -> PromoteOwnedSessionV1 {
    PromoteOwnedSessionV1 {
        idempotency_key: ProductPromotionIdempotencyKeyV1::parse(idempotency_key).unwrap(),
        session_id: AuthoringSessionId::parse("authoring").unwrap(),
        expected_generation,
    }
}

pub(super) async fn preview_ready_artifact_variant(
    label: &str,
    prompt: &str,
) -> PreviewReadyArtifactV1 {
    let core = LlmResponse::ToolCalls(vec![ToolCall {
        id: "interpret".to_string(),
        name: "interpret_intent_core".to_string(),
        arguments: json!({
            "expected_revision": 0,
            "request_mode": "build",
            "automation_kind": "managed_private_study_room",
            "requested_outcome": "validated_preview",
            "hub_channel": "community_hub",
            "language": "en",
            "close_policy": "disabled",
            "other_unmapped_required_capabilities": [],
            "response": ""
        })
        .to_string(),
    }]);
    let details = LlmResponse::ToolCalls(vec![ToolCall {
        id: "details".to_string(),
        name: "extract_private_study_room_details".to_string(),
        arguments: json!({"copy": {"create_button_label": label}}).to_string(),
    }]);
    let client = ScriptedClient {
        responses: Arc::new(Mutex::new(VecDeque::from([core, details]))),
    };
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
    let mut session = DesignSession::with_intent_recipe(client, bindings);
    let outcome = session.run_burst(prompt).await;
    assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
    session.export_preview_ready_artifact().unwrap()
}

pub(super) async fn create_pending_version(
    pool: &PgPool,
    adapter: &PostgresProductPromotions,
    ring: &ProductActionDigestKeyringV1,
    artifact: PreviewReadyArtifactV1,
    idempotency_key: &str,
    generation: u64,
) -> PreparedCase {
    let plan = promotion_plan_at_generation(idempotency_key, artifact, generation);
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let prepared = PreparedCase::new(
        ring,
        plan,
        idempotency_key,
        &format!("seed-{idempotency_key}"),
        database_now,
        &SESSION_DIGEST,
    );
    let admitted = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &prepared.access,
                &prepared.context,
                &prepared.digests,
                &prepared.plan,
                &prepared.admission,
                &prepared.serialized,
            )
            .await
            .unwrap(),
        &prepared,
    );
    let stage_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap();
    let stage_access = access_args(stage_now, &SESSION_DIGEST);
    let publication = direct_publish(pool, &stage_access, &admitted).await;
    let publication = decode_product_promotion_publication_v1(publication, &admitted).unwrap();
    let published = ProductPromotionAdmittedStageV1 {
        record: publication.record,
        admission: admitted.admission,
        admission_digest: admitted.admission_digest,
        database_now: publication.database_now,
    };
    let environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let environment_access = access_args(environment_now, &SESSION_DIGEST);
    let environment = direct_approval_environment(pool, &environment_access, &published).await;
    let environment =
        decode_product_promotion_approval_environment_v1(environment, &published).unwrap();
    let (resolved, target_artifact, database_now) = match environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("seed approval environment must resolve")
        }
    };
    let proposal = plan_pending_activation_v1(&published.record, resolved.clone()).unwrap();
    let activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: published.record.clone(),
            admission: published.admission.clone(),
            admission_digest: published.admission_digest.clone(),
            database_now,
        },
        resolved,
        target_artifact: *target_artifact,
    };
    let activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let activation_access = access_args(activation_now, &SESSION_DIGEST);
    let activation = direct_activation_link(pool, &activation_access, &published, &proposal).await;
    let activation = decode_product_promotion_activation_link_v1(
        activation,
        ring,
        &prepared.context,
        &activation_access,
        &prepared.digests,
        &activation_environment,
        &proposal,
    )
    .unwrap();
    assert!(matches!(
        activation,
        ProductPromotionActivationStageV1::Finalized(_)
    ));
    prepared
}

pub(super) async fn advance_authoring_generation(
    pool: &PgPool,
    generation: i64,
    artifact: &PreviewReadyArtifactV1,
) {
    let previous_generation = generation - 1;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let inserted = sqlx::query(
        "INSERT INTO public.authoring_session_generations ( \
         session_id, generation, tenant_id, installation_id, snapshot_schema_version, \
         snapshot_ciphertext, snapshot_nonce, encryption_key_id, encryption_suite, \
         encryption_suite_version, authenticated_metadata_digest, resource_bindings, \
         binding_fingerprint, installation_authority_revision, summary, stage, \
         candidate_revision, candidate_hash, writer_request_digest, harness_contract_revision) \
         SELECT session_id, $1, tenant_id, installation_id, snapshot_schema_version, \
          snapshot_ciphertext, snapshot_nonce, encryption_key_id, encryption_suite, \
          encryption_suite_version, authenticated_metadata_digest, resource_bindings, \
          binding_fingerprint, installation_authority_revision, summary, stage, $2, $3, $4, \
          harness_contract_revision \
         FROM public.authoring_session_generations \
         WHERE session_id = 'authoring' AND generation = $5",
    )
    .bind(generation)
    .bind(i64::try_from(artifact.receipt().candidate_revision).unwrap())
    .bind(&artifact.receipt().candidate_ruleset_hash)
    .bind(format!("{:064x}", generation + 100))
    .bind(previous_generation)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(inserted.rows_affected(), 1);
    let updated = sqlx::query(
        "UPDATE public.authoring_sessions \
         SET current_generation = $1, updated_at = pg_catalog.clock_timestamp() \
         WHERE session_id = 'authoring' AND current_generation = $2",
    )
    .bind(generation)
    .bind(previous_generation)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
    transaction.commit().await.unwrap();
}

pub(super) async fn apply_pending_version(
    pool: &PgPool,
    promotion_id: &str,
    runtime_generation: i64,
) -> (i64, String) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let activation = sqlx::query_as::<_, (String, i64, String)>(
        "SELECT id, target_version, target_content_hash \
         FROM public.activation_requests WHERE promotion_id = $1",
    )
    .bind(promotion_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    let mutation_clock =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.activation_requests DISABLE TRIGGER USER; \
         ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    if runtime_generation > 1 {
        sqlx::query(
            "UPDATE public.runtime_deployments \
             SET phase = 'superseded', superseded_at = $1, updated_at = $1 \
             WHERE guild_id = '3001' AND ruleset_key = 'ruleset' \
               AND phase NOT IN ('live', 'superseded', 'cancelled')",
        )
        .bind(mutation_clock)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    let applied = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applied', applied_at = $2, applied_by = requester_id, \
             completion_kind = 'activated', activation_notices = '[]'::JSONB \
         WHERE id = $1 AND state = 'pending' AND link_state_name = 'linked'",
    )
    .bind(&activation.0)
    .bind(mutation_clock)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(applied.rows_affected(), 1);
    let desired_digest = format!("{:064x}", runtime_generation);
    sqlx::query(
        "INSERT INTO public.runtime_deployments ( \
         deployment_id, tenant_id, installation_id, promotion_id, activation_request_id, \
         installation_authority_revision, guild_id, ruleset_key, target_version, \
         target_content_hash, binding_revision, binding_fingerprint, desired_target_digest, \
         runtime_generation, requested_at, snapshot_format_version, snapshot, revision, phase, \
         policy_revision, created_at, updated_at) \
         SELECT $1, 'tenant', 'installation', $2, $3, 1, '3001', 'ruleset', $4, $5, \
          1, authority.binding_fingerprint, $6, $7, $8, 1, \
          pg_catalog.jsonb_build_object('fixture', pg_catalog.repeat('x', 64)), \
          1, 'requested', authority.policy_revision, $8, $8 \
         FROM public.automation_installation_authority_versions AS authority \
         WHERE authority.tenant_id = 'tenant' AND authority.installation_id = 'installation' \
           AND authority.revision = 1",
    )
    .bind(format!("orchestrator-deployment-{runtime_generation}"))
    .bind(promotion_id)
    .bind(&activation.0)
    .bind(activation.1)
    .bind(&activation.2)
    .bind(desired_digest)
    .bind(runtime_generation)
    .bind(mutation_clock)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER; \
         ALTER TABLE public.activation_requests ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ('3001', 'ruleset', $1) \
         ON CONFLICT (guild_id, ruleset_key) DO UPDATE SET active_version = EXCLUDED.active_version",
    )
    .bind(activation.1)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    (activation.1, activation.2)
}

pub(super) async fn remove_active_pointer_for_test(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_activations \
         DISABLE TRIGGER automation_ruleset_activations_assert_product_slot",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("DELETE FROM public.automation_ruleset_activations")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_activations \
         ENABLE TRIGGER automation_ruleset_activations_assert_product_slot",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}
