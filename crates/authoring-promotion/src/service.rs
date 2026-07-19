use automation_ruleset::RuleSetStoreError;
use design_harness::PreviewReadyArtifactV1;

use crate::digest::DigestError;
use crate::id::PromotionIdError;
use crate::{
    plan_activation_link_v1, plan_approval_environment_v1, plan_pending_activation_v1,
    plan_ruleset_publication_v1, plan_start_promotion_v1, AuthenticatedPromotionContext,
    CreatePromotionOutcomeV1, IdempotencyKey, LinkedActivationTransitionV1, PendingActivationPort,
    PendingActivationPortError, PendingActivationTransitionV1, PromotionClock, PromotionId,
    PromotionRecordV1, PromotionStageV1, PromotionStore, PromotionStoreError,
    PublicationTransitionV1, RuleSetPublicationPort,
};

pub struct StartPromotionV1 {
    pub idempotency_key: IdempotencyKey,
    pub context: AuthenticatedPromotionContext,
    pub artifact: PreviewReadyArtifactV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumePromotionOutcomeV1 {
    Advanced(PromotionRecordV1),
    AlreadyActivationPending(PromotionRecordV1),
    TerminalExpired(PromotionRecordV1),
}

pub struct PromotionService<'a, S, P, A, C> {
    store: &'a S,
    publication: &'a P,
    activation: &'a A,
    clock: C,
}

impl<'a, S, P, A, C> PromotionService<'a, S, P, A, C> {
    pub fn new(store: &'a S, publication: &'a P, activation: &'a A, clock: C) -> Self {
        Self {
            store,
            publication,
            activation,
            clock,
        }
    }
}

impl<S, P, A, C> PromotionService<'_, S, P, A, C>
where
    S: PromotionStore,
    P: RuleSetPublicationPort,
    A: PendingActivationPort,
    C: PromotionClock,
{
    pub async fn start(
        &self,
        input: StartPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionError> {
        let promotion = plan_start_promotion_v1(input)?.materialize(self.clock.now())?;
        self.store
            .create_prepared(promotion)
            .await
            .map_err(PromotionError::Store)
    }

    pub async fn resume_to_activation_pending(
        &self,
        promotion_id: &PromotionId,
    ) -> Result<ResumePromotionOutcomeV1, PromotionError> {
        let mut advanced = false;
        for _ in 0..8 {
            let record = self
                .store
                .get(promotion_id)
                .await?
                .ok_or(PromotionError::NotFound)?;
            record
                .validate()
                .map_err(PromotionStoreError::InvalidRecord)?;
            match &record.stage {
                PromotionStageV1::Prepared => {
                    let publication = self.publish(&record).await?;
                    match self
                        .store
                        .mark_published(
                            &record.id,
                            record.revision,
                            publication.publication,
                            publication.expected_record.updated_at,
                        )
                        .await
                    {
                        Ok(_) => {
                            advanced = true;
                            continue;
                        }
                        Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                        Err(error) => return Err(PromotionError::Store(error)),
                    }
                }
                PromotionStageV1::Published { .. } => {
                    let request = self.request_pending(&record).await?;
                    let transition = match request {
                        PendingActivationTransitionV1::RefreshJournal => continue,
                        PendingActivationTransitionV1::ActivationPending {
                            activation,
                            expected_record,
                        } => {
                            self.store
                                .mark_activation_pending(
                                    &record.id,
                                    record.revision,
                                    activation,
                                    expected_record.updated_at,
                                )
                                .await
                        }
                        PendingActivationTransitionV1::Expired {
                            activation,
                            expected_record,
                        } => {
                            self.store
                                .mark_expired(
                                    &record.id,
                                    record.revision,
                                    activation,
                                    expected_record.updated_at,
                                )
                                .await
                        }
                    };
                    match transition {
                        Ok(record) => {
                            advanced = true;
                            if matches!(record.stage, PromotionStageV1::ActivationPending { .. }) {
                                continue;
                            }
                            return Ok(match &record.stage {
                                PromotionStageV1::Expired { .. } => {
                                    ResumePromotionOutcomeV1::TerminalExpired(record)
                                }
                                _ => ResumePromotionOutcomeV1::Advanced(record),
                            });
                        }
                        Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                        Err(error) => return Err(PromotionError::Store(error)),
                    }
                }
                PromotionStageV1::ActivationPending { .. } => {
                    let linked = self.link_pending(&record).await?;
                    if let LinkedActivationTransitionV1::Expired {
                        activation,
                        expected_record,
                    } = linked
                    {
                        match self
                            .store
                            .mark_expired(
                                &record.id,
                                record.revision,
                                *activation,
                                expected_record.updated_at,
                            )
                            .await
                        {
                            Ok(record) => {
                                return Ok(ResumePromotionOutcomeV1::TerminalExpired(record));
                            }
                            Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                            Err(error) => return Err(PromotionError::Store(error)),
                        }
                    }
                    return Ok(if advanced {
                        ResumePromotionOutcomeV1::Advanced(record)
                    } else {
                        ResumePromotionOutcomeV1::AlreadyActivationPending(record)
                    });
                }
                PromotionStageV1::Expired { .. } => {
                    return Ok(ResumePromotionOutcomeV1::TerminalExpired(record));
                }
            }
        }
        Err(PromotionError::ConcurrentTransitionLimit)
    }

    async fn publish(
        &self,
        record: &PromotionRecordV1,
    ) -> Result<PublicationTransitionV1, PromotionError> {
        let proposal = plan_ruleset_publication_v1(record)?;
        let outcome = self
            .publication
            .publish_ruleset(proposal.request())
            .await
            .map_err(PromotionError::RuleSet)?;
        proposal.complete(record, outcome, self.clock.now())
    }

    async fn request_pending(
        &self,
        record: &PromotionRecordV1,
    ) -> Result<PendingActivationTransitionV1, PromotionError> {
        let environment = plan_approval_environment_v1(record)?;
        let resolved = self
            .activation
            .resolve_product_approval_context(environment.request())
            .await?;
        let proposal = plan_pending_activation_v1(record, resolved)?;
        let receipt = self
            .activation
            .ensure_pending_activation(proposal.request())
            .await?;
        proposal.complete(record, &receipt, self.clock.now())
    }

    async fn link_pending(
        &self,
        record: &PromotionRecordV1,
    ) -> Result<LinkedActivationTransitionV1, PromotionError> {
        let proposal = plan_activation_link_v1(record)?;
        let linked = self
            .activation
            .link_pending_activation(proposal.request())
            .await?;
        proposal.complete(record, &linked, self.clock.now())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionError {
    #[error("only a validated-preview artifact can be promoted")]
    ValidatedPreviewRequired,
    #[error("authenticated principal does not own the authoring session")]
    SessionOwnerMismatch,
    #[error("authoring artifact hash {field} is invalid: {source}")]
    InvalidArtifactHash {
        field: &'static str,
        source: PromotionIdError,
    },
    #[error("authoring artifact count {field} exceeds the durable u64 range")]
    ArtifactCountOverflow { field: &'static str },
    #[error("approval policy is invalid")]
    InvalidPolicy,
    #[error("promotion was not found")]
    NotFound,
    #[error("published RuleSet does not match the exact prepared artifact")]
    PublicationMismatch,
    #[error("pending activation request does not match the exact publication and policy")]
    PendingActivationMismatch,
    #[error("activation request identity could not be constructed")]
    ActivationIdentity,
    #[error("concurrent promotion transition retry limit exceeded")]
    ConcurrentTransitionLimit,
    #[error("RuleSet hashing failed: {0:?}")]
    Hash(automation_ruleset::RuleSetHashError),
    #[error("RuleSet publication failed: {0:?}")]
    RuleSet(RuleSetStoreError),
    #[error(transparent)]
    PendingActivation(#[from] PendingActivationPortError),
    #[error(transparent)]
    Store(#[from] PromotionStoreError),
    #[error("promotion digest failed: {0}")]
    Digest(String),
}

impl From<DigestError> for PromotionError {
    fn from(error: DigestError) -> Self {
        Self::Digest(error.to_string())
    }
}
