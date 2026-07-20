use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use authoring_application::{
    AuthoringApplication, AuthoringApplicationError, AuthorizedPromotionBackendFailureV1,
    AuthorizedPromotionSubmissionErrorV1, InstallationSelectorV1, ProductRequestIdV1,
    PromotionSubmissionDispositionV1,
};
use authoring_promotion::{ResumePromotionOutcomeV1, SessionGeneration};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::Postgres;

use super::super::*;
use super::support::*;

async fn install_activation_gate_wrapper(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
ALTER FUNCTION public.starring_product_promotion_activation_link_v1(
    TEXT, TEXT, TEXT, BYTEA, TEXT, TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT,
    TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN, TEXT, BIGINT, TEXT, TEXT, JSONB
) RENAME TO starring_product_promotion_activation_link_original_v1;
CREATE FUNCTION public.starring_product_promotion_activation_link_v1(
    expected_tenant_id TEXT,
    expected_installation_id TEXT,
    expected_principal_id TEXT,
    expected_product_session_digest BYTEA,
    expected_acting_user_id TEXT,
    expected_discord_application_id TEXT,
    expected_guild_id TEXT,
    expected_capability TEXT,
    observed_current_authority_revision BIGINT,
    observed_current_authority_payload_digest TEXT,
    authority_observation_digest TEXT,
    authority_observed_at TIMESTAMPTZ,
    authority_expires_at TIMESTAMPTZ,
    effective_permission_bits TEXT,
    guild_owner BOOLEAN,
    expected_promotion_id TEXT,
    expected_promotion_revision BIGINT,
    expected_promotion_request_digest TEXT,
    expected_admission_digest TEXT,
    activation_proposal JSONB
)
RETURNS TABLE(
    outcome_code TEXT,
    promotion_record JSONB,
    admission_evidence JSONB,
    admission_digest TEXT,
    activation_projection JSONB,
    receipt_projection JSONB,
    audit_evidence_projection JSONB,
    database_now TIMESTAMPTZ
)
LANGUAGE plpgsql
VOLATILE
STRICT
SECURITY DEFINER
PARALLEL UNSAFE
ROWS 1
SET search_path = pg_catalog
AS $function$
DECLARE
    baseline_state TEXT;
    baseline_version INTEGER;
BEGIN
    baseline_state := activation_proposal #>> '{proposal,approval_context,baseline,state}';
    IF baseline_state = 'absent' THEN
        baseline_version := 0;
    ELSIF baseline_state = 'exact'
        AND activation_proposal #>> '{proposal,approval_context,baseline,version}'
            ~ '^[1-9][0-9]{0,8}$'
    THEN
        baseline_version := (
            activation_proposal #>> '{proposal,approval_context,baseline,version}'
        )::INTEGER;
    END IF;
    IF baseline_version IS NOT NULL THEN
        PERFORM pg_catalog.pg_advisory_xact_lock(18771, baseline_version);
    END IF;
    RETURN QUERY
    SELECT original.outcome_code,
        original.promotion_record,
        original.admission_evidence,
        original.admission_digest,
        original.activation_projection,
        original.receipt_projection,
        original.audit_evidence_projection,
        original.database_now
    FROM public.starring_product_promotion_activation_link_original_v1(
        expected_tenant_id,
        expected_installation_id,
        expected_principal_id,
        expected_product_session_digest,
        expected_acting_user_id,
        expected_discord_application_id,
        expected_guild_id,
        expected_capability,
        observed_current_authority_revision,
        observed_current_authority_payload_digest,
        authority_observation_digest,
        authority_observed_at,
        authority_expires_at,
        effective_permission_bits,
        guild_owner,
        expected_promotion_id,
        expected_promotion_revision,
        expected_promotion_request_digest,
        expected_admission_digest,
        activation_proposal
    ) AS original;
END;
$function$;
"#,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn lock_activation_gate(
    connection: &mut sqlx::pool::PoolConnection<Postgres>,
    baseline_version: i32,
) {
    sqlx::query("SELECT pg_catalog.pg_advisory_lock($1, $2)")
        .bind(ACTIVATION_GATE_CLASS)
        .bind(baseline_version)
        .execute(&mut **connection)
        .await
        .unwrap();
}

async fn unlock_activation_gate(
    connection: &mut sqlx::pool::PoolConnection<Postgres>,
    baseline_version: i32,
) {
    let unlocked = sqlx::query_scalar::<_, bool>("SELECT pg_catalog.pg_advisory_unlock($1, $2)")
        .bind(ACTIVATION_GATE_CLASS)
        .bind(baseline_version)
        .fetch_one(&mut **connection)
        .await
        .unwrap();
    assert!(unlocked);
}

async fn await_activation_gate_wait(pool: &PgPool, baseline_version: i32) {
    for _ in 0..200 {
        let waiting = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
             SELECT 1 FROM pg_catalog.pg_locks \
             WHERE locktype = 'advisory' AND NOT granted \
               AND database = (SELECT oid FROM pg_catalog.pg_database \
                   WHERE datname = pg_catalog.current_database()) \
               AND classid::BIGINT = $1 AND objid::BIGINT = $2 \
             )",
        )
        .bind(i64::from(ACTIVATION_GATE_CLASS))
        .bind(i64::from(baseline_version))
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("activation gate wait was not observed")
}

async fn install_pointer(pool: &PgPool, version: i64) {
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ('3001', 'ruleset', $1) \
         ON CONFLICT (guild_id, ruleset_key) DO UPDATE SET active_version = EXCLUDED.active_version",
    )
    .bind(version)
    .execute(pool)
    .await
    .unwrap();
}

async fn assert_exact_activation_context(pool: &PgPool, promotion_id: &str, version: i64) {
    let baseline = sqlx::query_as::<_, (String, i64)>(
        "SELECT approval_context #>> '{context,baseline,state}', \
         (approval_context #>> '{context,baseline,version}')::BIGINT \
         FROM public.activation_requests WHERE promotion_id = $1",
    )
    .bind(promotion_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(baseline, ("exact".to_string(), version));
}

async fn current_promotion_write_state(
    pool: &PgPool,
    promotion_id: &str,
    receipt_id: &str,
) -> (String, i64, i64, i64, i64) {
    sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT promotion.stage, \
         (SELECT pg_catalog.count(*) FROM public.activation_requests \
          WHERE promotion_id = promotion.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE receipt_id = $2) \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(promotion_id)
    .bind(receipt_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

struct ChurnFixture {
    administrator: PgConnection,
    pool: PgPool,
    ring: ProductActionDigestKeyringV1,
    artifact: PreviewReadyArtifactV1,
    current: PreparedCase,
    v1_version: i64,
    v2_promotion_id: String,
    idempotency_key: &'static str,
}

async fn churn_fixture(name: &str, idempotency_key: &'static str) -> ChurnFixture {
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let v1_artifact = preview_ready_artifact().await;
    let v2_artifact = preview_ready_artifact_variant(
        "Start alpha room",
        "Create private rooms in community_hub and prepare a validated preview. Set the launcher create-button label to 'Start alpha room'.",
    )
    .await;
    let artifact = preview_ready_artifact_variant(
        "Start beta room",
        "Create private rooms in community_hub and prepare a validated preview. Set the launcher create-button label to 'Start beta room'.",
    )
    .await;
    let hashes = [
        v1_artifact.receipt().candidate_ruleset_hash.as_str(),
        v2_artifact.receipt().candidate_ruleset_hash.as_str(),
        artifact.receipt().candidate_ruleset_hash.as_str(),
    ];
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
    assert_ne!(hashes[1], hashes[2]);
    let ring = keyring();
    let seed_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let seed_plan = promotion_plan("orchestrator-lineage-v1", v1_artifact.clone());
    let seed = PreparedCase::new(
        &ring,
        seed_plan,
        "orchestrator-lineage-v1",
        "orchestrator-lineage-seed",
        seed_now,
        &SESSION_DIGEST,
    );
    seed_control_plane(&pool, &seed.plan, &seed.access).await;
    let promotions = PostgresProductPromotions::new(pool.clone(), ring.clone()).unwrap();
    let v1 = create_pending_version(
        &pool,
        &promotions,
        &ring,
        v1_artifact,
        "orchestrator-lineage-v1",
        1,
    )
    .await;
    let (v1_version, _) = apply_pending_version(&pool, v1.plan.promotion_id.as_str(), 1).await;
    assert_eq!(v1_version, 1);
    advance_authoring_generation(&pool, 2, &v2_artifact).await;
    let v2 = create_pending_version(
        &pool,
        &promotions,
        &ring,
        v2_artifact,
        "orchestrator-lineage-v2",
        2,
    )
    .await;
    let v2_version = sqlx::query_scalar::<_, i64>(
        "SELECT target_version FROM public.activation_requests WHERE promotion_id = $1",
    )
    .bind(v2.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(v2_version, 2);
    remove_active_pointer_for_test(&pool).await;
    install_activation_gate_wrapper(&pool).await;
    advance_authoring_generation(&pool, 3, &artifact).await;
    let current_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let current = PreparedCase::new(
        &ring,
        promotion_plan_at_generation(idempotency_key, artifact.clone(), 3),
        idempotency_key,
        "orchestrator-current-seed",
        current_now,
        &SESSION_DIGEST,
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT next_version FROM public.automation_ruleset_heads \
             WHERE guild_id = '3001' AND ruleset_key = 'ruleset'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        3
    );
    ChurnFixture {
        administrator,
        pool,
        ring,
        artifact,
        current,
        v1_version,
        v2_promotion_id: v2.plan.promotion_id.to_string(),
        idempotency_key,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn application_submission_finalizes_and_exact_replay_skips_snapshot() {
    let name = "starring_product_promotion_orchestrator_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let artifact = preview_ready_artifact().await;
    let idempotency_key = "orchestrator-happy-replay";
    let plan = promotion_plan(idempotency_key, artifact.clone());
    let ring = keyring();
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let prepared = PreparedCase::new(
        &ring,
        plan,
        idempotency_key,
        "orchestrator-seed",
        database_now,
        &SESSION_DIGEST,
    );
    seed_control_plane(&pool, &prepared.plan, &prepared.access).await;
    let authentication = Authentication;
    let authority = authority_adapter(&prepared.plan);
    let snapshot_calls = Arc::new(AtomicUsize::new(0));
    let snapshots = Snapshot {
        artifact,
        authority: resolved_authority(&prepared.plan),
        expected_generation: SessionGeneration::new(1).unwrap(),
        calls: snapshot_calls.clone(),
    };
    let promotions = PostgresProductPromotions::new(pool.clone(), ring).unwrap();
    let application =
        AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions);
    let installation =
        InstallationSelectorV1::new(prepared.plan.intent.authority.installation_id.clone());

    let first = application
        .promote_owned_session(
            "valid-credential",
            "valid-csrf",
            &ProductRequestIdV1::parse("orchestrator.first").unwrap(),
            &installation,
            promotion_command(idempotency_key, SessionGeneration::new(1).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(first.disposition, PromotionSubmissionDispositionV1::Created);
    let first_record = match first.advancement {
        ResumePromotionOutcomeV1::Advanced(record) => record,
        ResumePromotionOutcomeV1::AlreadyActivationPending(_)
        | ResumePromotionOutcomeV1::TerminalExpired(_) => panic!("first submission must advance"),
    };
    assert!(matches!(
        first_record.stage,
        authoring_promotion::PromotionStageV1::ActivationPending { .. }
    ));
    assert_eq!(snapshot_calls.load(Ordering::SeqCst), 1);

    let replay = application
        .promote_owned_session(
            "valid-credential",
            "valid-csrf",
            &ProductRequestIdV1::parse("orchestrator.replay").unwrap(),
            &installation,
            promotion_command(idempotency_key, SessionGeneration::new(1).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(
        replay.disposition,
        PromotionSubmissionDispositionV1::ExactReplay
    );
    let replay_record = match replay.advancement {
        ResumePromotionOutcomeV1::AlreadyActivationPending(record) => record,
        ResumePromotionOutcomeV1::Advanced(_) | ResumePromotionOutcomeV1::TerminalExpired(_) => {
            panic!("exact replay must report already activation pending")
        }
    };
    assert_eq!(replay_record.id, first_record.id);
    assert_eq!(replay_record.revision, first_record.revision);
    assert_eq!(snapshot_calls.load(Ordering::SeqCst), 1);

    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn application_submission_refreshes_one_changed_approval_environment() {
    let fixture = churn_fixture(
        "starring_product_promotion_refresh_test",
        "orchestrator-one-refresh",
    )
    .await;
    let ChurnFixture {
        administrator,
        pool,
        ring,
        artifact,
        current,
        v1_version,
        v2_promotion_id: _,
        idempotency_key,
    } = fixture;
    let mut controller = pool.acquire().await.unwrap();
    lock_activation_gate(&mut controller, 0).await;
    let authentication = Authentication;
    let authority = authority_adapter(&current.plan);
    let snapshot_calls = Arc::new(AtomicUsize::new(0));
    let snapshots = Snapshot {
        artifact,
        authority: resolved_authority(&current.plan),
        expected_generation: SessionGeneration::new(3).unwrap(),
        calls: snapshot_calls.clone(),
    };
    let promotions = PostgresProductPromotions::new(pool.clone(), ring).unwrap();
    let application =
        AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions);
    let installation =
        InstallationSelectorV1::new(current.plan.intent.authority.installation_id.clone());
    let request_id = ProductRequestIdV1::parse("orchestrator.refresh.first").unwrap();
    let submission = application.promote_owned_session(
        "valid-credential",
        "valid-csrf",
        &request_id,
        &installation,
        promotion_command(idempotency_key, SessionGeneration::new(3).unwrap()),
    );
    let control = async {
        await_activation_gate_wait(&pool, 0).await;
        install_pointer(&pool, v1_version).await;
        unlock_activation_gate(&mut controller, 0).await;
    };
    let (submission, ()) = tokio::join!(submission, control);
    let submission = submission.unwrap();
    assert_eq!(
        submission.disposition,
        PromotionSubmissionDispositionV1::Created
    );
    assert!(matches!(
        submission.advancement,
        ResumePromotionOutcomeV1::Advanced(_)
    ));
    assert_eq!(snapshot_calls.load(Ordering::SeqCst), 1);
    assert_exact_activation_context(&pool, current.plan.promotion_id.as_str(), v1_version).await;
    assert_eq!(
        current_promotion_write_state(
            &pool,
            current.plan.promotion_id.as_str(),
            &current.admission.payload.receipt_id,
        )
        .await,
        ("activation_pending".to_string(), 1, 1, 1, 1)
    );
    drop(promotions);
    drop(controller);
    drop_temporary_database(
        administrator,
        pool,
        "starring_product_promotion_refresh_test",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn application_submission_bounds_repeated_approval_environment_churn() {
    let name = "starring_product_promotion_budget_test";
    let fixture = churn_fixture(name, "orchestrator-two-refreshes").await;
    let ChurnFixture {
        administrator,
        pool,
        ring,
        artifact,
        current,
        v1_version,
        v2_promotion_id,
        idempotency_key,
    } = fixture;
    let mut controller = pool.acquire().await.unwrap();
    lock_activation_gate(&mut controller, 0).await;
    lock_activation_gate(&mut controller, i32::try_from(v1_version).unwrap()).await;
    let authentication = Authentication;
    let authority = authority_adapter(&current.plan);
    let snapshot_calls = Arc::new(AtomicUsize::new(0));
    let snapshots = Snapshot {
        artifact,
        authority: resolved_authority(&current.plan),
        expected_generation: SessionGeneration::new(3).unwrap(),
        calls: snapshot_calls.clone(),
    };
    let promotions = PostgresProductPromotions::new(pool.clone(), ring).unwrap();
    let application =
        AuthoringApplication::new(&authentication, &authority, &snapshots, &promotions);
    let installation =
        InstallationSelectorV1::new(current.plan.intent.authority.installation_id.clone());
    let request_id = ProductRequestIdV1::parse("orchestrator.budget.first").unwrap();
    let submission = application.promote_owned_session(
        "valid-credential",
        "valid-csrf",
        &request_id,
        &installation,
        promotion_command(idempotency_key, SessionGeneration::new(3).unwrap()),
    );
    let control = async {
        await_activation_gate_wait(&pool, 0).await;
        install_pointer(&pool, v1_version).await;
        unlock_activation_gate(&mut controller, 0).await;
        let v1_gate = i32::try_from(v1_version).unwrap();
        await_activation_gate_wait(&pool, v1_gate).await;
        let (v2_version, _) = apply_pending_version(&pool, &v2_promotion_id, 2).await;
        assert_eq!(v2_version, 2);
        unlock_activation_gate(&mut controller, v1_gate).await;
    };
    let (submission, ()) = tokio::join!(submission, control);
    assert_eq!(
        submission.unwrap_err(),
        AuthoringApplicationError::AuthorizedPromotion(
            AuthorizedPromotionSubmissionErrorV1::Backend(
                AuthorizedPromotionBackendFailureV1::Retryable
            )
        )
    );
    assert_eq!(snapshot_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        current_promotion_write_state(
            &pool,
            current.plan.promotion_id.as_str(),
            &current.admission.payload.receipt_id,
        )
        .await,
        ("published".to_string(), 0, 0, 0, 0)
    );

    let replay = application
        .promote_owned_session(
            "valid-credential",
            "valid-csrf",
            &ProductRequestIdV1::parse("orchestrator.budget.replay").unwrap(),
            &installation,
            promotion_command(idempotency_key, SessionGeneration::new(3).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(
        replay.disposition,
        PromotionSubmissionDispositionV1::ExactReplay
    );
    assert!(matches!(
        replay.advancement,
        ResumePromotionOutcomeV1::Advanced(_)
    ));
    assert_eq!(snapshot_calls.load(Ordering::SeqCst), 1);
    assert_exact_activation_context(&pool, current.plan.promotion_id.as_str(), 2).await;
    assert_eq!(
        current_promotion_write_state(
            &pool,
            current.plan.promotion_id.as_str(),
            &current.admission.payload.receipt_id,
        )
        .await,
        ("activation_pending".to_string(), 1, 1, 1, 1)
    );
    drop(promotions);
    drop(controller);
    drop_temporary_database(administrator, pool, name).await;
}
