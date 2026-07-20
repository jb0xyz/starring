use std::time::Duration;

use authoring_promotion::{PromotionRecordV1, PromotionStageV1};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::postgres::PgPool;
use sqlx::{Executor, Postgres};

use crate::product_promotions::admission::{
    prepare_legacy_product_promotion_admission_v1, PreparedProductPromotionAdmissionV1,
    ProductPromotionAdmissionContextV1, ProductPromotionAdmissionEvidenceV1,
};
use crate::product_promotions::authorization::ProductPromotionAccessArgsV1;
use crate::product_promotions::digest::ProductPromotionDigestsV1;
use crate::product_promotions::row::{
    ProductPromotionActivationLinkRowV1, ProductPromotionLegacyRepairStageV1,
    ProductPromotionLegacyRepairV1, ProductPromotionReplayStageV1,
};

use super::super::*;

pub(super) struct LegacyFixture {
    pub(super) case: PreparedCase,
    pub(super) secret: String,
    pub(super) record: PromotionRecordV1,
    pub(super) activation_id: String,
    pub(super) activation_expires_at: DateTime<Utc>,
}

pub(super) struct RepairInput {
    pub(super) access: ProductPromotionAccessArgsV1,
    pub(super) context: ProductPromotionAdmissionContextV1,
    pub(super) digests: ProductPromotionDigestsV1,
    pub(super) legacy: ProductPromotionLegacyRepairV1,
    pub(super) admission: PreparedProductPromotionAdmissionV1,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RepairWriteState {
    pub(super) promotion_stage: String,
    pub(super) promotion_revision: i64,
    pub(super) admission_count: i64,
    pub(super) activation_state: String,
    pub(super) link_state: String,
    pub(super) receipt_count: i64,
    pub(super) alias_count: i64,
    pub(super) audit_count: i64,
    pub(super) evidence_count: i64,
}

pub(super) fn legacy_keyring() -> ProductActionDigestKeyringV1 {
    ProductActionDigestKeyringV1::new(digest_key("retired-v1", 113), []).unwrap()
}

pub(super) async fn create_legacy_fixture(
    pool: &PgPool,
    ring: &ProductActionDigestKeyringV1,
    artifact: PreviewReadyArtifactV1,
    secret: &str,
    request_id: &str,
    linked: bool,
    ttl_seconds: u64,
) -> LegacyFixture {
    let now = database_now(pool).await;
    let mut context = promotion_context();
    context.policy.ttl_seconds = NonZeroU64::new(ttl_seconds).unwrap();
    let plan = plan_start_promotion_v1(StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse(secret).unwrap(),
        context,
        artifact,
    })
    .unwrap();
    let case = PreparedCase::new(ring, plan, secret, request_id, now, &SESSION_DIGEST);
    let control_plane_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM public.product_principals)")
            .fetch_one(pool)
            .await
            .unwrap();
    if !control_plane_exists {
        seed_control_plane(pool, &case.plan, &case.access).await;
    }
    let adapter = PostgresProductPromotions::new(pool.clone(), ring.clone()).unwrap();
    let prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &case.access,
                &case.context,
                &case.digests,
                &case.plan,
                &case.admission,
                &case.serialized,
            )
            .await
            .unwrap(),
        &case,
    );
    let publish_access = access_args(database_now(pool).await, &SESSION_DIGEST);
    let publication = direct_publish(pool, &publish_access, &prepared).await;
    let publication = decode_product_promotion_publication_v1(publication, &prepared).unwrap();
    let published = ProductPromotionAdmittedStageV1 {
        record: publication.record,
        admission: prepared.admission,
        admission_digest: prepared.admission_digest,
        database_now: publication.database_now,
    };
    let environment_access = access_args(database_now(pool).await, &SESSION_DIGEST);
    let environment = direct_approval_environment(pool, &environment_access, &published).await;
    let environment =
        decode_product_promotion_approval_environment_v1(environment, &published).unwrap();
    let (resolved, target_artifact, environment_now) = match environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("legacy fixture unexpectedly finalized before activation")
        }
    };
    let proposal = plan_pending_activation_v1(&published.record, resolved.clone()).unwrap();
    let activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: published.record.clone(),
            admission: published.admission.clone(),
            admission_digest: published.admission_digest.clone(),
            database_now: environment_now,
        },
        resolved,
        target_artifact: *target_artifact,
    };
    let activation_access = access_args(database_now(pool).await, &SESSION_DIGEST);
    let row = direct_activation_link(pool, &activation_access, &published, &proposal).await;
    assert!(matches!(row.outcome_code.as_str(), "created" | "reused"));
    let finalized = decode_product_promotion_activation_link_v1(
        row,
        ring,
        &case.context,
        &activation_access,
        &case.digests,
        &activation_environment,
        &proposal,
    )
    .unwrap();
    let record = match finalized {
        ProductPromotionActivationStageV1::Finalized(finalized) => finalized.admitted.record,
        ProductPromotionActivationStageV1::FinalReplayRequired(_)
        | ProductPromotionActivationStageV1::ApprovalEnvironmentChanged => {
            panic!("legacy fixture activation did not finalize")
        }
    };
    let (activation_id, activation_expires_at) = sqlx::query_as::<_, (String, DateTime<Utc>)>(
        "SELECT id, expires_at FROM public.activation_requests WHERE promotion_id = $1",
    )
    .bind(case.plan.promotion_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    erase_product_finalization(pool, &case, linked).await;
    LegacyFixture {
        case,
        secret: secret.to_string(),
        record,
        activation_id,
        activation_expires_at,
    }
}

async fn erase_product_finalization(pool: &PgPool, case: &PreparedCase, linked: bool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.authoring_promotions DISABLE TRIGGER USER; \
         ALTER TABLE public.activation_requests DISABLE TRIGGER USER; \
         ALTER TABLE public.product_action_receipts DISABLE TRIGGER ALL; \
         ALTER TABLE public.product_action_receipt_idempotency_aliases DISABLE TRIGGER ALL; \
         ALTER TABLE public.product_audit_events DISABLE TRIGGER ALL; \
         ALTER TABLE public.product_action_receipt_audit_evidence DISABLE TRIGGER ALL",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::raw_sql(
        "TRUNCATE public.product_action_receipt_audit_evidence, \
         public.product_audit_events, \
         public.product_action_receipt_idempotency_aliases, \
         public.product_action_receipts",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET product_admission_format_version = NULL, product_admission_digest = NULL, \
             product_admission = NULL \
         WHERE id = $1",
    )
    .bind(case.plan.promotion_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    if !linked {
        sqlx::query(
            "UPDATE public.activation_requests \
             SET link_state_name = 'unlinked', link_state = $2, linked_at = NULL \
             WHERE promotion_id = $1",
        )
        .bind(case.plan.promotion_id.as_str())
        .bind(sqlx::types::Json(json!({"state": "unlinked"})))
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    sqlx::raw_sql(
        "ALTER TABLE public.product_action_receipt_audit_evidence ENABLE TRIGGER ALL; \
         ALTER TABLE public.product_audit_events ENABLE TRIGGER ALL; \
         ALTER TABLE public.product_action_receipt_idempotency_aliases ENABLE TRIGGER ALL; \
         ALTER TABLE public.product_action_receipts ENABLE TRIGGER ALL; \
         ALTER TABLE public.activation_requests ENABLE TRIGGER USER; \
         ALTER TABLE public.authoring_promotions ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

pub(super) async fn repair_input(
    pool: &PgPool,
    ring: &ProductActionDigestKeyringV1,
    fixture: &LegacyFixture,
    recovery_request_id: &str,
) -> RepairInput {
    let access = access_args(database_now(pool).await, &SESSION_DIGEST);
    let context = admission_context(
        recovery_request_id,
        fixture.case.plan.intent.authority.session_generation,
    );
    let digests = promotion_digests(ring, &fixture.case.plan, &fixture.secret, &SESSION_DIGEST);
    let adapter = PostgresProductPromotions::new(pool.clone(), ring.clone()).unwrap();
    let legacy = match adapter
        .execute_replay_stage_v1(&access, &context, &digests)
        .await
        .unwrap()
    {
        ProductPromotionReplayStageV1::LegacyRepairRequired(legacy) => *legacy,
        _ => panic!("legacy fixture was not detected as repairable"),
    };
    assert_eq!(legacy.record, fixture.record);
    let admission = prepare_legacy_product_promotion_admission_v1(
        ring,
        &context,
        &access,
        &legacy.record,
        &digests,
    )
    .unwrap();
    RepairInput {
        access,
        context,
        digests,
        legacy,
        admission,
    }
}

pub(super) async fn progress_legacy_activation_to_approved(pool: &PgPool, fixture: &LegacyFixture) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.activation_requests DISABLE TRIGGER USER; \
         ALTER TABLE public.activation_request_approvals DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_request_approvals \
         (request_id, approver_id, approved_at, approval_payload_digest, \
          tenant_id, installation_id) \
         SELECT id, '1002', created_at + INTERVAL '1 millisecond', \
             approval_payload_digest, tenant_id, installation_id \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("UPDATE public.activation_requests SET state = 'approved' WHERE id = $1")
        .bind(&fixture.activation_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.activation_request_approvals ENABLE TRIGGER USER; \
         ALTER TABLE public.activation_requests ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

pub(super) async fn progress_legacy_activation_to_terminal(
    pool: &PgPool,
    fixture: &LegacyFixture,
    state: &str,
) {
    if state == "superseded" {
        progress_legacy_activation_to_approved(pool, fixture).await;
    }
    let at = database_now(pool).await;
    let termination = match state {
        "superseded" => {
            let fingerprint = sqlx::query_scalar::<_, String>(
                "SELECT approval_context #>> '{context,binding,fingerprint}' \
                 FROM public.activation_requests WHERE id = $1",
            )
            .bind(&fixture.activation_id)
            .fetch_one(pool)
            .await
            .unwrap();
            json!({
                "kind": "superseded",
                "at": at,
                "reason": {
                    "reason": "binding_drift",
                    "expected_revision": 1,
                    "observed_revision": 2,
                    "expected_fingerprint": fingerprint,
                    "observed_fingerprint": null
                }
            })
        }
        "withdrawn" => json!({
            "kind": "withdrawn",
            "at": at,
            "by": "1002",
            "reason": "cancelled before activation"
        }),
        _ => panic!("unsupported terminal activation state"),
    };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.activation_requests DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("UPDATE public.activation_requests SET state = $2, termination = $3 WHERE id = $1")
        .bind(&fixture.activation_id)
        .bind(state)
        .bind(sqlx::types::Json(termination))
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.activation_requests ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

pub(super) async fn expire_repaired_promotion(pool: &PgPool, fixture: &LegacyFixture) {
    let now = database_now(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.activation_requests DISABLE TRIGGER USER; \
         ALTER TABLE public.authoring_promotions DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("UPDATE public.activation_requests SET state = 'expired' WHERE id = $1")
        .bind(&fixture.activation_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 4, stage = 'expired', record = record || pg_catalog.jsonb_build_object( \
             'revision', 4, \
             'stage', pg_catalog.jsonb_build_object( \
                 'state', 'expired', \
                 'publication', record #> '{stage,publication}', \
                 'activation', (record #> '{stage,activation}') || \
                     pg_catalog.jsonb_build_object( \
                         'disposition', 'reused', \
                         'request_state_at_journal', 'expired' \
                     ) \
             ), \
             'updated_at', $2::TIMESTAMPTZ \
         ) WHERE id = $1",
    )
    .bind(fixture.case.plan.promotion_id.as_str())
    .bind(now)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.authoring_promotions ENABLE TRIGGER USER; \
         ALTER TABLE public.activation_requests ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

pub(super) async fn rewrite_legacy_promotion_stage(
    pool: &PgPool,
    fixture: &LegacyFixture,
    stage: &str,
) {
    let mut record = serde_json::to_value(&fixture.record).unwrap();
    match stage {
        "prepared" => {
            record["revision"] = json!(1);
            record["stage"] = json!({"state": "prepared"});
            record["updated_at"] = record["created_at"].clone();
        }
        "published" => {
            let publication = record["stage"]["publication"].clone();
            record["revision"] = json!(2);
            record["stage"] = json!({
                "state": "published",
                "publication": publication
            });
        }
        _ => panic!("unsupported legacy promotion stage"),
    }
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.authoring_promotions DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions SET revision = $2, stage = $3, record = $4 \
         WHERE id = $1",
    )
    .bind(fixture.case.plan.promotion_id.as_str())
    .bind(if stage == "prepared" { 1_i64 } else { 2_i64 })
    .bind(stage)
    .bind(sqlx::types::Json(record))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.authoring_promotions ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

pub(super) async fn direct_repair<'executor, ExecutorType>(
    executor: ExecutorType,
    input: &RepairInput,
) -> Result<ProductPromotionActivationLinkRowV1, sqlx::Error>
where
    ExecutorType: Executor<'executor, Database = Postgres>,
{
    sqlx::query_as::<_, ProductPromotionActivationLinkRowV1>(
        "SELECT outcome_code, promotion_record, admission_evidence, admission_digest, \
         activation_projection, receipt_projection, audit_evidence_projection, database_now \
         FROM public.starring_product_promotion_repair_link_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
         $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29)",
    )
    .bind(&input.access.expected_tenant_id)
    .bind(&input.access.expected_installation_id)
    .bind(&input.access.expected_principal_id)
    .bind(&input.access.expected_product_session_digest)
    .bind(&input.access.expected_acting_user_id)
    .bind(&input.access.expected_discord_application_id)
    .bind(&input.access.expected_guild_id)
    .bind(&input.access.expected_capability)
    .bind(input.access.observed_current_authority_revision)
    .bind(&input.access.observed_current_authority_payload_digest)
    .bind(&input.access.authority_observation_digest)
    .bind(input.access.authority_observed_at)
    .bind(input.access.authority_expires_at)
    .bind(&input.access.effective_permission_bits)
    .bind(input.access.guild_owner)
    .bind(input.legacy.record.id.as_str())
    .bind(input.legacy.record.request_digest.as_str())
    .bind(&input.context.product_request_id)
    .bind(&input.digests.session_subject)
    .bind(sqlx::types::Json(&input.admission.payload))
    .bind(&input.admission.digest)
    .bind(&input.digests.active_idempotency)
    .bind(&input.digests.idempotency_candidates)
    .bind(&input.digests.idempotency_candidate_key_ids)
    .bind(&input.digests.idempotency_candidate_key_fingerprints)
    .bind(&input.digests.active_key_id)
    .bind(&input.digests.semantic_request)
    .bind(&input.digests.receipt_id)
    .bind(&input.digests.audit_event_id)
    .fetch_one(executor)
    .await
}

pub(super) fn decode_repair(
    row: ProductPromotionActivationLinkRowV1,
    ring: &ProductActionDigestKeyringV1,
    input: &RepairInput,
) -> ProductPromotionLegacyRepairStageV1 {
    crate::product_promotions::row::decode_product_promotion_repair_link_v1(
        row,
        ring,
        &input.context,
        &input.access,
        &input.digests,
        &input.legacy,
        &input.admission,
    )
    .unwrap()
}

pub(super) async fn repair_write_state(
    pool: &PgPool,
    promotion_id: &str,
    receipt_id: &str,
) -> RepairWriteState {
    let state = sqlx::query_as::<_, (String, i64, i64, String, String, i64, i64, i64, i64)>(
        "SELECT promotion.stage, promotion.revision, \
         CASE WHEN promotion.product_admission IS NOT NULL THEN 1::BIGINT ELSE 0::BIGINT END, \
         activation.state, activation.link_state_name, \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE receipt_id = $2) \
         FROM public.authoring_promotions AS promotion \
         INNER JOIN public.activation_requests AS activation \
           ON activation.promotion_id = promotion.id \
         WHERE promotion.id = $1",
    )
    .bind(promotion_id)
    .bind(receipt_id)
    .fetch_one(pool)
    .await
    .unwrap();
    RepairWriteState {
        promotion_stage: state.0,
        promotion_revision: state.1,
        admission_count: state.2,
        activation_state: state.3,
        link_state: state.4,
        receipt_count: state.5,
        alias_count: state.6,
        audit_count: state.7,
        evidence_count: state.8,
    }
}

pub(super) async fn wait_until_expired(fixtures: &[&LegacyFixture]) {
    let latest = fixtures
        .iter()
        .map(|fixture| fixture.activation_expires_at)
        .max()
        .unwrap();
    let now = Utc::now();
    if latest >= now {
        let delay = (latest - now).to_std().unwrap_or_default() + Duration::from_millis(75);
        tokio::time::sleep(delay).await;
    }
}

pub(super) async fn database_now(pool: &PgPool) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap()
}

pub(super) fn assert_recovered(
    stage: ProductPromotionLegacyRepairStageV1,
    expected_state: &str,
    expected_revision: u64,
) {
    let ProductPromotionLegacyRepairStageV1::Finalized(finalized) = stage else {
        panic!("repair unexpectedly returned a replay signal")
    };
    assert_eq!(finalized.admitted.record.revision.get(), expected_revision);
    match expected_state {
        "activation_pending" => assert!(matches!(
            finalized.admitted.record.stage,
            PromotionStageV1::ActivationPending { .. }
        )),
        "expired" => assert!(matches!(
            finalized.admitted.record.stage,
            PromotionStageV1::Expired { .. }
        )),
        _ => panic!("unsupported expected promotion state"),
    }
}

pub(super) fn admission_evidence(fixture: &LegacyFixture) -> ProductPromotionAdmissionEvidenceV1 {
    ProductPromotionAdmissionEvidenceV1 {
        format_version: 1,
        payload: fixture.case.admission.payload.clone(),
        admitted_at: fixture.record.created_at,
    }
}
