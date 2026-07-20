use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use authoring_application::{
    AuthoringApplication, InstallationSelectorV1, ProductRequestIdV1,
    PromotionSubmissionDispositionV1,
};
use authoring_promotion::{ResumePromotionOutcomeV1, SessionGeneration};
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Executor;

use super::super::orchestrator_e2e::{
    authority_adapter, promotion_command, resolved_authority, Authentication, Snapshot,
};
use super::super::*;
use super::support::*;

const EXTERNAL_FUNCTIONS: [&str; 8] = [
    "public.starring_product_promotion_executor_database_identity_v1()",
    "public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])",
    "public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)",
    "public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_keyring_coverage_v1(text[],text[])",
];
const OWNER_ONLY_FUNCTIONS: [&str; 23] = [
    "public.starring_product_promotion_authorize_current_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean)",
    "public.starring_product_promotion_finalize_receipt_v1(jsonb,jsonb,jsonb,jsonb,jsonb)",
    "public.starring_canonical_json_v1(jsonb)",
    "public.starring_ruleset_content_hash_v1(bigint,jsonb)",
    "public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)",
    "public.enforce_authoring_promotion_scope()",
    "public.enforce_authoring_promotion_product_admission()",
    "public.enforce_authoring_promotion_product_transition()",
    "public.reject_ruleset_artifact_mutation()",
    "public.enforce_product_activation_journal_link()",
    "public.enforce_product_activation_scope()",
    "public.guard_legacy_activation_product_slot()",
    "public.guard_product_ruleset_artifact_transition()",
    "public.enforce_activation_approval_payload_binding()",
    "public.enforce_activation_approval_scope()",
    "public.reject_activation_approval_mutation()",
    "public.assert_product_approval_receipt_alias()",
    "public.assert_product_approval_receipt_audit()",
    "public.enforce_product_action_receipt_retention()",
    "public.enforce_product_action_receipt_alias_capacity()",
    "public.enforce_product_action_receipt_alias_retention()",
    "public.capture_product_action_receipt_audit_evidence()",
    "public.reject_immutable_product_approval_row()",
];
const RELATIONS: [&str; 18] = [
    "public.product_control_plane_identity",
    "public.product_principals",
    "public.product_auth_sessions",
    "public.product_tenants",
    "public.automation_installations",
    "public.automation_installation_authority_versions",
    "public.authoring_sessions",
    "public.authoring_session_generations",
    "public.authoring_promotions",
    "public.automation_ruleset_heads",
    "public.automation_ruleset_versions",
    "public.automation_ruleset_activations",
    "public.activation_requests",
    "public.activation_request_approvals",
    "public.product_action_receipts",
    "public.product_action_receipt_idempotency_aliases",
    "public.product_audit_events",
    "public.product_action_receipt_audit_evidence",
];
const AUTHORIZE_DENIAL_QUERY: &str = "SELECT * FROM \
    public.starring_product_promotion_authorize_current_v1(\
        NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::BYTEA, NULL::TEXT, \
        NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::BIGINT, NULL::TEXT, \
        NULL::TEXT, NULL::TIMESTAMPTZ, NULL::TIMESTAMPTZ, NULL::TEXT, \
        NULL::BOOLEAN)";

static ROLE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct RestrictedRoles {
    owner_role: String,
    executor_role: String,
    executor_pool: PgPool,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn restricted_executor_completes_new_and_legacy_promotion_verticals() {
    let database_suffix = ROLE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        "starring_promotion_restricted_test_{}_{}",
        std::process::id(),
        database_suffix
    );
    let (mut administrator, owner_pool) = temporary_database(&name).await;
    MIGRATOR.run(&owner_pool).await.unwrap();
    let ring = keyring();
    let artifact = preview_ready_artifact().await;
    let idempotency_key = "restricted-executor-new";
    let normal_plan = promotion_plan(idempotency_key, artifact.clone());
    let seed = PreparedCase::new(
        &ring,
        normal_plan.clone(),
        idempotency_key,
        "restricted-executor-seed",
        database_now(&owner_pool).await,
        &SESSION_DIGEST,
    );
    seed_control_plane(&owner_pool, &seed.plan, &seed.access).await;
    let roles = install_restricted_roles(
        &mut administrator,
        &owner_pool,
        &name,
        database_url().parse::<PgConnectOptions>().unwrap(),
    )
    .await;
    let outcome = AssertUnwindSafe(async {
        let promotions =
            PostgresProductPromotions::new(roles.executor_pool.clone(), ring.clone()).unwrap();
        promotions.verify_readiness().await.unwrap();
        assert_executor_denied(&roles.executor_pool).await;
        let authentication = Authentication;
        let authority = authority_adapter(&normal_plan);
        let normal_snapshot_calls = Arc::new(AtomicUsize::new(0));
        let normal_snapshots = Snapshot {
            artifact: artifact.clone(),
            authority: resolved_authority(&normal_plan),
            expected_generation: SessionGeneration::new(1).unwrap(),
            calls: normal_snapshot_calls.clone(),
        };
        let normal_application =
            AuthoringApplication::new(&authentication, &authority, &normal_snapshots, &promotions);
        let installation =
            InstallationSelectorV1::new(normal_plan.intent.authority.installation_id.clone());
        let normal = normal_application
            .promote_owned_session(
                "valid-credential",
                "valid-csrf",
                &ProductRequestIdV1::parse("restricted-executor-new-request").unwrap(),
                &installation,
                promotion_command(idempotency_key, SessionGeneration::new(1).unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(
            normal.disposition,
            PromotionSubmissionDispositionV1::Created
        );
        let normal_record = match normal.advancement {
            ResumePromotionOutcomeV1::Advanced(record) => record,
            ResumePromotionOutcomeV1::AlreadyActivationPending(_)
            | ResumePromotionOutcomeV1::TerminalExpired(_) => {
                panic!("new restricted promotion did not advance")
            }
        };
        let normal_digests =
            promotion_digests(&ring, &normal_plan, idempotency_key, &SESSION_DIGEST);
        assert_atomic_result(
            &owner_pool,
            normal_record.id.as_str(),
            &normal_digests.receipt_id,
            "activation_pending",
            3,
        )
        .await;
        let replay = normal_application
            .promote_owned_session(
                "valid-credential",
                "valid-csrf",
                &ProductRequestIdV1::parse("restricted-executor-new-replay").unwrap(),
                &installation,
                promotion_command(idempotency_key, SessionGeneration::new(1).unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(
            replay.disposition,
            PromotionSubmissionDispositionV1::ExactReplay
        );
        assert!(matches!(
            replay.advancement,
            ResumePromotionOutcomeV1::AlreadyActivationPending(_)
        ));
        assert_eq!(normal_snapshot_calls.load(Ordering::SeqCst), 1);
        let (activation_id, activation_expires_at) =
            sqlx::query_as::<_, (String, chrono::DateTime<chrono::Utc>)>(
                "SELECT id, expires_at FROM public.activation_requests WHERE promotion_id = $1",
            )
            .bind(normal_record.id.as_str())
            .fetch_one(&owner_pool)
            .await
            .unwrap();
        let legacy = LegacyFixture {
            case: seed,
            secret: idempotency_key.to_string(),
            record: normal_record,
            activation_id,
            activation_expires_at,
        };
        erase_product_finalization(&owner_pool, &legacy.case, true).await;
        progress_legacy_activation_to_approved(&owner_pool, &legacy).await;
        let repaired = normal_application
            .promote_owned_session(
                "valid-credential",
                "valid-csrf",
                &ProductRequestIdV1::parse("restricted-executor-legacy-repair").unwrap(),
                &installation,
                promotion_command(idempotency_key, SessionGeneration::new(1).unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(
            repaired.disposition,
            PromotionSubmissionDispositionV1::ExactReplay
        );
        assert!(matches!(
            repaired.advancement,
            ResumePromotionOutcomeV1::AlreadyActivationPending(_)
        ));
        assert_eq!(normal_snapshot_calls.load(Ordering::SeqCst), 1);
        assert_atomic_result(
            &owner_pool,
            legacy.case.plan.promotion_id.as_str(),
            &normal_digests.receipt_id,
            "activation_pending",
            3,
        )
        .await;
        assert_approved_activation(&owner_pool, legacy.case.plan.promotion_id.as_str()).await;
        promotions.verify_readiness().await.unwrap();
    })
    .catch_unwind()
    .await;
    roles.executor_pool.close().await;
    owner_pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
        .unwrap();
    for role in [&roles.executor_role, &roles.owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

async fn assert_atomic_result(
    owner: &PgPool,
    promotion_id: &str,
    receipt_id: &str,
    stage: &str,
    revision: i64,
) {
    let state = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        "SELECT promotion.stage, promotion.revision, \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases AS alias \
          WHERE alias.receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence AS evidence \
          WHERE evidence.receipt_id = $2) \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(promotion_id)
    .bind(receipt_id)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(state, (stage.to_string(), revision, 1, 1, 1, 1));
}

async fn assert_approved_activation(owner: &PgPool, promotion_id: &str) {
    let state = sqlx::query_as::<_, (String, i64, bool)>(
        "SELECT activation.state, pg_catalog.count(approval.approver_id), \
         pg_catalog.bool_and(approval.approval_payload_digest \
             = activation.approval_payload_digest) \
         FROM public.activation_requests AS activation \
         INNER JOIN public.activation_request_approvals AS approval \
             ON approval.request_id = activation.id \
         WHERE activation.promotion_id = $1 \
         GROUP BY activation.id",
    )
    .bind(promotion_id)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(state, ("approved".to_string(), 1, true));
}

async fn assert_executor_denied(executor: &PgPool) {
    for query in [
        "SELECT 1 FROM public.authoring_promotions LIMIT 1",
        "SELECT 1 FROM public.activation_request_approvals LIMIT 1",
        AUTHORIZE_DENIAL_QUERY,
    ] {
        let error = sqlx::query(query).execute(executor).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
    }
}

async fn install_restricted_roles(
    administrator: &mut PgConnection,
    owner_pool: &PgPool,
    database_name: &str,
    connection_options: PgConnectOptions,
) -> RestrictedRoles {
    let suffix = format!(
        "{}_{}",
        std::process::id(),
        ROLE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let owner_role = format!("promotion_owner_{suffix}");
    let executor_role = format!("promotion_executor_{suffix}");
    let password = database_role_password();
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(&password)
        .fetch_one(&mut *administrator)
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&mut *administrator)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {executor_role} LOGIN PASSWORD {password_literal} \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
         NOBYPASSRLS CONNECTION LIMIT 4"
    ))
    .execute(&mut *administrator)
    .await
    .unwrap();
    for relation in RELATIONS {
        owner_pool
            .execute(format!("ALTER TABLE {relation} OWNER TO {owner_role}").as_str())
            .await
            .unwrap();
    }
    for function in EXTERNAL_FUNCTIONS.into_iter().chain(OWNER_ONLY_FUNCTIONS) {
        owner_pool
            .execute(format!("ALTER FUNCTION {function} OWNER TO {owner_role}").as_str())
            .await
            .unwrap();
        owner_pool
            .execute(format!("REVOKE ALL ON FUNCTION {function} FROM PUBLIC").as_str())
            .await
            .unwrap();
    }
    owner_pool
        .execute(format!("REVOKE ALL ON DATABASE {database_name} FROM PUBLIC").as_str())
        .await
        .unwrap();
    owner_pool
        .execute("REVOKE ALL ON SCHEMA public FROM PUBLIC")
        .await
        .unwrap();
    owner_pool
        .execute(format!("GRANT CONNECT ON DATABASE {database_name} TO {executor_role}").as_str())
        .await
        .unwrap();
    owner_pool
        .execute(format!("GRANT USAGE ON SCHEMA public TO {owner_role}, {executor_role}").as_str())
        .await
        .unwrap();
    owner_pool
        .execute(
            format!(
                "GRANT EXECUTE ON FUNCTION {} TO {executor_role}",
                EXTERNAL_FUNCTIONS.join(", ")
            )
            .as_str(),
        )
        .await
        .unwrap();
    let executor_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(
            connection_options
                .database(database_name)
                .username(&executor_role)
                .password(&password),
        )
        .await
        .unwrap();
    RestrictedRoles {
        owner_role,
        executor_role,
        executor_pool,
    }
}

fn database_role_password() -> String {
    let mut material = [0_u8; 32];
    getrandom::fill(&mut material).unwrap();
    material.iter().map(|byte| format!("{byte:02x}")).collect()
}
