use authoring_promotion::{
    CreatePromotionOutcomeV1, NewPromotionV1, PendingActivationLinkV1, PromotionId,
    PromotionRecordV1, PromotionRevision, PromotionStore, PromotionStoreError, PublicationRecordV1,
};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::types::Json;

use crate::row::{backend, decode_record, stage_name, PromotionRow, PROMOTION_COLUMNS};

#[derive(Clone)]
pub struct PostgresPromotionStore {
    pool: PgPool,
}

impl PostgresPromotionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn transition(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        updated_at: DateTime<Utc>,
        transition: Transition,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        let current = fetch_record(&self.pool, promotion_id)
            .await?
            .ok_or(PromotionStoreError::NotFound)?;
        let next =
            match transition {
                Transition::Published(publication) => {
                    current.transition_to_published(expected_revision, publication, updated_at)?
                }
                Transition::ActivationPending(activation) => current
                    .transition_to_activation_pending(expected_revision, activation, updated_at)?,
                Transition::Expired(activation) => {
                    current.transition_to_expired(expected_revision, activation, updated_at)?
                }
            };
        match persist_transition(&self.pool, &current, &next).await? {
            Some(persisted) => Ok(persisted),
            None => match fetch_record(&self.pool, promotion_id).await? {
                None => Err(PromotionStoreError::NotFound),
                Some(winner) if winner.revision != expected_revision => {
                    Err(PromotionStoreError::RevisionConflict {
                        current: winner.revision,
                    })
                }
                Some(_) => Err(backend("promotion transition CAS failed")),
            },
        }
    }
}

enum Transition {
    Published(PublicationRecordV1),
    ActivationPending(PendingActivationLinkV1),
    Expired(PendingActivationLinkV1),
}

async fn fetch_record(
    pool: &PgPool,
    promotion_id: &PromotionId,
) -> Result<Option<PromotionRecordV1>, PromotionStoreError> {
    let row = sqlx::query_as::<_, PromotionRow>(&format!(
        "SELECT {PROMOTION_COLUMNS} FROM authoring_promotions WHERE id = $1"
    ))
    .bind(promotion_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(backend)?;
    row.map(decode_record).transpose()
}

async fn persist_transition(
    pool: &PgPool,
    current: &PromotionRecordV1,
    next: &PromotionRecordV1,
) -> Result<Option<PromotionRecordV1>, PromotionStoreError> {
    let current_revision = database_revision(current.revision)?;
    let next_revision = database_revision(next.revision)?;
    let row = sqlx::query_as::<_, PromotionRow>(&format!(
        "UPDATE authoring_promotions SET revision = $2, stage = $3, request_digest = $4, \
         tenant_id = $5, installation_id = $6, principal_id = $7, record = $8 \
         WHERE id = $1 AND revision = $9 RETURNING {PROMOTION_COLUMNS}"
    ))
    .bind(next.id.as_str())
    .bind(next_revision)
    .bind(stage_name(&next.stage))
    .bind(next.request_digest.as_str())
    .bind(next.intent.authority.tenant_id.as_str())
    .bind(next.intent.authority.installation_id.as_str())
    .bind(next.intent.authority.principal_id.as_str())
    .bind(Json(next))
    .bind(current_revision)
    .fetch_optional(pool)
    .await
    .map_err(backend)?;
    row.map(decode_record).transpose()
}

fn database_revision(revision: PromotionRevision) -> Result<i64, PromotionStoreError> {
    i64::try_from(revision.get()).map_err(|_| PromotionStoreError::RevisionOverflow)
}

impl PromotionStore for PostgresPromotionStore {
    async fn create_prepared(
        &self,
        promotion: NewPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionStoreError> {
        let record = PromotionRecordV1::prepared(promotion)?;
        let revision = database_revision(record.revision)?;
        let result = sqlx::query(
            "INSERT INTO authoring_promotions \
             (id, record_format_version, revision, stage, request_digest, tenant_id, installation_id, principal_id, record) \
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT DO NOTHING",
        )
        .bind(record.id.as_str())
        .bind(revision)
        .bind(stage_name(&record.stage))
        .bind(record.request_digest.as_str())
        .bind(record.intent.authority.tenant_id.as_str())
        .bind(record.intent.authority.installation_id.as_str())
        .bind(record.intent.authority.principal_id.as_str())
        .bind(Json(&record))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 1 {
            return Ok(CreatePromotionOutcomeV1::Created(record));
        }
        let existing = fetch_record(&self.pool, &record.id)
            .await?
            .ok_or_else(|| backend("conflicting promotion disappeared"))?;
        if existing.request_digest == record.request_digest {
            Ok(CreatePromotionOutcomeV1::ExactReplay(existing))
        } else {
            Err(PromotionStoreError::IdempotencyConflict)
        }
    }

    async fn get(
        &self,
        promotion_id: &PromotionId,
    ) -> Result<Option<PromotionRecordV1>, PromotionStoreError> {
        fetch_record(&self.pool, promotion_id).await
    }

    async fn mark_published(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        publication: PublicationRecordV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        self.transition(
            promotion_id,
            expected_revision,
            updated_at,
            Transition::Published(publication),
        )
        .await
    }

    async fn mark_activation_pending(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        self.transition(
            promotion_id,
            expected_revision,
            updated_at,
            Transition::ActivationPending(activation),
        )
        .await
    }

    async fn mark_expired(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        self.transition(
            promotion_id,
            expected_revision,
            updated_at,
            Transition::Expired(activation),
        )
        .await
    }
}
