use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

use crate::{
    NewPromotionV1, PendingActivationLinkV1, PromotionId, PromotionRecordV1,
    PromotionRecordValidationError, PromotionRevision, PromotionStageV1, PublicationRecordV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatePromotionOutcomeV1 {
    Created(PromotionRecordV1),
    ExactReplay(PromotionRecordV1),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionStoreError {
    #[error("idempotency key is already bound to a different promotion request")]
    IdempotencyConflict,
    #[error("promotion was not found")]
    NotFound,
    #[error("promotion revision conflict; current revision is {current}")]
    RevisionConflict { current: PromotionRevision },
    #[error("promotion is not in the required stage")]
    InvalidTransition,
    #[error("promotion revision overflow")]
    RevisionOverflow,
    #[error("promotion record is invalid: {0}")]
    InvalidRecord(PromotionRecordValidationError),
    #[error("promotion store backend failed: {0}")]
    Backend(String),
}

pub trait PromotionClock: Clone + Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UtcPromotionClock;

impl PromotionClock for UtcPromotionClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone)]
pub struct ManualPromotionClock {
    now: Arc<Mutex<DateTime<Utc>>>,
}

impl ManualPromotionClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(Mutex::new(now)),
        }
    }

    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().unwrap() = now;
    }

    pub fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().unwrap();
        *now = now.checked_add_signed(duration).unwrap();
    }
}

impl PromotionClock for ManualPromotionClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }
}

#[allow(async_fn_in_trait)]
pub trait PromotionStore {
    async fn create_prepared(
        &self,
        promotion: NewPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionStoreError>;

    async fn get(
        &self,
        promotion_id: &PromotionId,
    ) -> Result<Option<PromotionRecordV1>, PromotionStoreError>;

    async fn mark_published(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        publication: PublicationRecordV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError>;

    async fn mark_activation_pending(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError>;

    async fn mark_expired(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError>;
}

#[derive(Default)]
pub struct InMemoryPromotionStore {
    state: Mutex<InMemoryPromotionState>,
}

#[derive(Default)]
struct InMemoryPromotionState {
    records: BTreeMap<PromotionId, PromotionRecordV1>,
}

impl PromotionRecordV1 {
    pub fn prepared(promotion: NewPromotionV1) -> Result<Self, PromotionStoreError> {
        let record = Self {
            id: promotion.id,
            revision: PromotionRevision::FIRST,
            request_digest: promotion.request_digest,
            intent: promotion.intent,
            stage: PromotionStageV1::Prepared,
            created_at: promotion.created_at,
            updated_at: promotion.created_at,
        };
        record
            .validate()
            .map_err(PromotionStoreError::InvalidRecord)?;
        Ok(record)
    }

    pub fn transition_to_published(
        &self,
        expected_revision: PromotionRevision,
        publication: PublicationRecordV1,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, PromotionStoreError> {
        self.ensure_transition(expected_revision, updated_at)?;
        if self.stage != PromotionStageV1::Prepared {
            return Err(PromotionStoreError::InvalidTransition);
        }
        self.with_next_stage(PromotionStageV1::Published { publication }, updated_at)
    }

    pub fn transition_to_activation_pending(
        &self,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, PromotionStoreError> {
        self.ensure_transition(expected_revision, updated_at)?;
        let PromotionStageV1::Published { publication } = &self.stage else {
            return Err(PromotionStoreError::InvalidTransition);
        };
        self.with_next_stage(
            PromotionStageV1::ActivationPending {
                publication: publication.clone(),
                activation,
            },
            updated_at,
        )
    }

    pub fn transition_to_expired(
        &self,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, PromotionStoreError> {
        self.ensure_transition(expected_revision, updated_at)?;
        let PromotionStageV1::Published { publication } = &self.stage else {
            return Err(PromotionStoreError::InvalidTransition);
        };
        self.with_next_stage(
            PromotionStageV1::Expired {
                publication: publication.clone(),
                activation,
            },
            updated_at,
        )
    }

    fn ensure_transition(
        &self,
        expected_revision: PromotionRevision,
        updated_at: DateTime<Utc>,
    ) -> Result<(), PromotionStoreError> {
        self.validate()
            .map_err(PromotionStoreError::InvalidRecord)?;
        if self.revision != expected_revision {
            return Err(PromotionStoreError::RevisionConflict {
                current: self.revision,
            });
        }
        if updated_at < self.updated_at {
            return Err(PromotionStoreError::InvalidRecord(
                PromotionRecordValidationError::Timestamp,
            ));
        }
        Ok(())
    }

    fn with_next_stage(
        &self,
        stage: PromotionStageV1,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, PromotionStoreError> {
        let revision = self
            .revision
            .next()
            .map_err(|_| PromotionStoreError::RevisionOverflow)?;
        let next = Self {
            stage,
            revision,
            updated_at,
            ..self.clone()
        };
        next.validate()
            .map_err(PromotionStoreError::InvalidRecord)?;
        Ok(next)
    }
}

impl PromotionStore for InMemoryPromotionStore {
    async fn create_prepared(
        &self,
        promotion: NewPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionStoreError> {
        let record = PromotionRecordV1::prepared(promotion)?;
        let mut state = self.state.lock().unwrap();
        if let Some(existing) = state.records.get(&record.id) {
            existing
                .validate()
                .map_err(PromotionStoreError::InvalidRecord)?;
            return if existing.request_digest == record.request_digest {
                Ok(CreatePromotionOutcomeV1::ExactReplay(existing.clone()))
            } else {
                Err(PromotionStoreError::IdempotencyConflict)
            };
        }
        state.records.insert(record.id.clone(), record.clone());
        Ok(CreatePromotionOutcomeV1::Created(record))
    }

    async fn get(
        &self,
        promotion_id: &PromotionId,
    ) -> Result<Option<PromotionRecordV1>, PromotionStoreError> {
        let record = self
            .state
            .lock()
            .unwrap()
            .records
            .get(promotion_id)
            .cloned();
        if let Some(record) = &record {
            record
                .validate()
                .map_err(PromotionStoreError::InvalidRecord)?;
        }
        Ok(record)
    }

    async fn mark_published(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        publication: PublicationRecordV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        let mut state = self.state.lock().unwrap();
        let current = state
            .records
            .get(promotion_id)
            .cloned()
            .ok_or(PromotionStoreError::NotFound)?;
        let next = current.transition_to_published(expected_revision, publication, updated_at)?;
        state.records.insert(next.id.clone(), next.clone());
        Ok(next)
    }

    async fn mark_activation_pending(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        let mut state = self.state.lock().unwrap();
        let current = state
            .records
            .get(promotion_id)
            .cloned()
            .ok_or(PromotionStoreError::NotFound)?;
        let next =
            current.transition_to_activation_pending(expected_revision, activation, updated_at)?;
        state.records.insert(next.id.clone(), next.clone());
        Ok(next)
    }

    async fn mark_expired(
        &self,
        promotion_id: &PromotionId,
        expected_revision: PromotionRevision,
        activation: PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        let mut state = self.state.lock().unwrap();
        let current = state
            .records
            .get(promotion_id)
            .cloned()
            .ok_or(PromotionStoreError::NotFound)?;
        let next = current.transition_to_expired(expected_revision, activation, updated_at)?;
        state.records.insert(next.id.clone(), next.clone());
        Ok(next)
    }
}
