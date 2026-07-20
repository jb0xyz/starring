use authoring_application::{
    AuthorizedPromotionAccessV1, AuthorizedPromotionBackendFailureV1,
    AuthorizedPromotionSubmissionErrorV1, AuthorizedPromotionSubmissionPort,
    AuthorizedPromotionSubmissionV1, PromotionSubmissionDispositionV1, PromotionSubmissionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use authoring_promotion::{PromotionStageV1, ResumePromotionOutcomeV1};

use super::row::{
    ProductPromotionActivationStageV1, ProductPromotionAdmittedStageV1,
    ProductPromotionApprovalEnvironmentOutcomeV1, ProductPromotionFinalReplayV1,
    ProductPromotionLegacyRepairStageV1, ProductPromotionPrepareStageV1,
    ProductPromotionPublishStageV1, ProductPromotionReplayStageV1,
};
use super::store::PostgresProductPromotions;

const APPROVAL_ENVIRONMENT_REFRESH_LIMIT: u8 = 1;

#[derive(Debug)]
struct ApprovalEnvironmentRefreshBudgetV1 {
    remaining: u8,
}

impl ApprovalEnvironmentRefreshBudgetV1 {
    fn new() -> Self {
        Self {
            remaining: APPROVAL_ENVIRONMENT_REFRESH_LIMIT,
        }
    }

    fn consume(&mut self) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
        if self.remaining == 0 {
            return Err(AuthorizedPromotionSubmissionErrorV1::Backend(
                AuthorizedPromotionBackendFailureV1::Retryable,
            ));
        }
        self.remaining -= 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromotionJournalProgressV1 {
    Unchanged,
    Advanced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalPromotionStateV1 {
    ActivationPending,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalSubmissionKindV1 {
    Advanced,
    AlreadyActivationPending,
    TerminalExpired,
}

impl AuthorizedPromotionSubmissionPort<FreshDiscordAuthorityEvidenceV1>
    for PostgresProductPromotions
{
    async fn find_or_resume_authorized_promotion(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<Option<PromotionSubmissionV1>, AuthorizedPromotionSubmissionErrorV1> {
        match self.replay_authorized_promotion_stage_v1(access).await? {
            ProductPromotionReplayStageV1::Missing => Ok(None),
            ProductPromotionReplayStageV1::PartialExact(admitted) => self
                .resume_admitted_promotion_v1(
                    access,
                    *admitted,
                    PromotionSubmissionDispositionV1::ExactReplay,
                )
                .await
                .map(Some),
            ProductPromotionReplayStageV1::FinalExact(finalized) => final_submission_v1(
                *finalized,
                PromotionSubmissionDispositionV1::ExactReplay,
                PromotionJournalProgressV1::Unchanged,
            )
            .map(Some),
            ProductPromotionReplayStageV1::LegacyRepairRequired(legacy) => {
                let stage = self
                    .repair_legacy_authorized_promotion_stage_v1(access, *legacy)
                    .await?;
                let submission = match stage {
                    ProductPromotionLegacyRepairStageV1::Finalized(finalized) => {
                        final_submission_v1(
                            *finalized,
                            PromotionSubmissionDispositionV1::ExactReplay,
                            PromotionJournalProgressV1::Unchanged,
                        )?
                    }
                    ProductPromotionLegacyRepairStageV1::FinalReplayRequired(_) => {
                        self.replay_final_submission_v1(
                            access,
                            PromotionSubmissionDispositionV1::ExactReplay,
                            PromotionJournalProgressV1::Unchanged,
                        )
                        .await?
                    }
                };
                Ok(Some(submission))
            }
        }
    }

    async fn submit_authorized_promotion(
        &self,
        request: AuthorizedPromotionSubmissionV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
        let (access, stage) = self.prepare_authorized_promotion_stage_v1(request).await?;
        match stage {
            ProductPromotionPrepareStageV1::Created(admitted) => {
                self.resume_admitted_promotion_v1(
                    &access,
                    *admitted,
                    PromotionSubmissionDispositionV1::Created,
                )
                .await
            }
            ProductPromotionPrepareStageV1::PartialExact(admitted) => {
                self.resume_admitted_promotion_v1(
                    &access,
                    *admitted,
                    PromotionSubmissionDispositionV1::ExactReplay,
                )
                .await
            }
            ProductPromotionPrepareStageV1::FinalExact(finalized) => final_submission_v1(
                *finalized,
                PromotionSubmissionDispositionV1::ExactReplay,
                PromotionJournalProgressV1::Unchanged,
            ),
            ProductPromotionPrepareStageV1::FinalReplayRequired(_) => {
                self.replay_final_submission_v1(
                    &access,
                    PromotionSubmissionDispositionV1::ExactReplay,
                    PromotionJournalProgressV1::Unchanged,
                )
                .await
            }
        }
    }
}

impl PostgresProductPromotions {
    async fn resume_admitted_promotion_v1(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
        admitted: ProductPromotionAdmittedStageV1,
        disposition: PromotionSubmissionDispositionV1,
    ) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
        let (published, progress) = match self
            .publish_authorized_promotion_stage_v1(access, admitted)
            .await?
        {
            ProductPromotionPublishStageV1::Published { admitted, advanced } => (
                *admitted,
                if advanced {
                    PromotionJournalProgressV1::Advanced
                } else {
                    PromotionJournalProgressV1::Unchanged
                },
            ),
            ProductPromotionPublishStageV1::FinalReplayRequired(_) => {
                return self
                    .replay_final_submission_v1(
                        access,
                        disposition,
                        PromotionJournalProgressV1::Unchanged,
                    )
                    .await
            }
        };
        let environment = match self
            .resolve_authorized_promotion_approval_environment_stage_v1(access, published)
            .await?
        {
            ProductPromotionApprovalEnvironmentOutcomeV1::Resolved(environment) => *environment,
            ProductPromotionApprovalEnvironmentOutcomeV1::FinalReplayRequired(_) => {
                return self
                    .replay_final_submission_v1(access, disposition, progress)
                    .await
            }
        };
        let mut environment = environment;
        let mut refresh_budget = ApprovalEnvironmentRefreshBudgetV1::new();
        loop {
            match self
                .link_authorized_promotion_activation_stage_v1(access, &environment)
                .await?
            {
                ProductPromotionActivationStageV1::Finalized(finalized) => {
                    return final_submission_v1(
                        *finalized,
                        disposition,
                        PromotionJournalProgressV1::Advanced,
                    )
                }
                ProductPromotionActivationStageV1::FinalReplayRequired(_) => {
                    return self
                        .replay_final_submission_v1(access, disposition, progress)
                        .await
                }
                ProductPromotionActivationStageV1::ApprovalEnvironmentChanged => {
                    refresh_budget.consume()?;
                    environment = match self
                        .resolve_authorized_promotion_approval_environment_stage_v1(
                            access,
                            environment.admitted,
                        )
                        .await?
                    {
                        ProductPromotionApprovalEnvironmentOutcomeV1::Resolved(environment) => {
                            *environment
                        }
                        ProductPromotionApprovalEnvironmentOutcomeV1::FinalReplayRequired(_) => {
                            return self
                                .replay_final_submission_v1(access, disposition, progress)
                                .await
                        }
                    };
                }
            }
        }
    }

    async fn replay_final_submission_v1(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
        disposition: PromotionSubmissionDispositionV1,
        progress: PromotionJournalProgressV1,
    ) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
        match self.replay_authorized_promotion_stage_v1(access).await? {
            ProductPromotionReplayStageV1::FinalExact(finalized) => {
                final_submission_v1(*finalized, disposition, progress)
            }
            ProductPromotionReplayStageV1::Missing
            | ProductPromotionReplayStageV1::PartialExact(_)
            | ProductPromotionReplayStageV1::LegacyRepairRequired(_) => {
                Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
            }
        }
    }
}

fn final_submission_v1(
    finalized: ProductPromotionFinalReplayV1,
    disposition: PromotionSubmissionDispositionV1,
    progress: PromotionJournalProgressV1,
) -> Result<PromotionSubmissionV1, AuthorizedPromotionSubmissionErrorV1> {
    let ProductPromotionFinalReplayV1 {
        admitted,
        receipt: _,
        audit_evidence: _,
    } = finalized;
    let record = admitted.record;
    let state = match &record.stage {
        PromotionStageV1::ActivationPending { .. } => FinalPromotionStateV1::ActivationPending,
        PromotionStageV1::Expired { .. } => FinalPromotionStateV1::Expired,
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    let advancement = match final_submission_kind_v1(state, progress) {
        FinalSubmissionKindV1::Advanced => ResumePromotionOutcomeV1::Advanced(record),
        FinalSubmissionKindV1::AlreadyActivationPending => {
            ResumePromotionOutcomeV1::AlreadyActivationPending(record)
        }
        FinalSubmissionKindV1::TerminalExpired => ResumePromotionOutcomeV1::TerminalExpired(record),
    };
    Ok(PromotionSubmissionV1 {
        disposition,
        advancement,
    })
}

fn final_submission_kind_v1(
    state: FinalPromotionStateV1,
    progress: PromotionJournalProgressV1,
) -> FinalSubmissionKindV1 {
    match (state, progress) {
        (FinalPromotionStateV1::ActivationPending, PromotionJournalProgressV1::Advanced) => {
            FinalSubmissionKindV1::Advanced
        }
        (FinalPromotionStateV1::ActivationPending, PromotionJournalProgressV1::Unchanged) => {
            FinalSubmissionKindV1::AlreadyActivationPending
        }
        (FinalPromotionStateV1::Expired, _) => FinalSubmissionKindV1::TerminalExpired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_submission_semantics_match_the_domain_service() {
        assert_eq!(
            final_submission_kind_v1(
                FinalPromotionStateV1::ActivationPending,
                PromotionJournalProgressV1::Advanced,
            ),
            FinalSubmissionKindV1::Advanced
        );
        assert_eq!(
            final_submission_kind_v1(
                FinalPromotionStateV1::ActivationPending,
                PromotionJournalProgressV1::Unchanged,
            ),
            FinalSubmissionKindV1::AlreadyActivationPending
        );
        for progress in [
            PromotionJournalProgressV1::Advanced,
            PromotionJournalProgressV1::Unchanged,
        ] {
            assert_eq!(
                final_submission_kind_v1(FinalPromotionStateV1::Expired, progress),
                FinalSubmissionKindV1::TerminalExpired
            );
        }
    }

    #[test]
    fn approval_environment_refresh_is_bounded_to_one_retry() {
        let mut budget = ApprovalEnvironmentRefreshBudgetV1::new();
        assert_eq!(budget.consume(), Ok(()));
        assert_eq!(
            budget.consume(),
            Err(AuthorizedPromotionSubmissionErrorV1::Backend(
                AuthorizedPromotionBackendFailureV1::Retryable,
            ))
        );
        assert_eq!(budget.remaining, 0);
    }
}
