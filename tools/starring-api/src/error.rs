use authoring_application::{
    AuthenticationBackendFailureV1, AuthenticationError, AuthoringAdmissionError,
    AuthoringApplicationError, AuthoringConversationError, AuthoringSessionLoadError,
    AuthoringSessionObservationErrorV1, AuthorizedPromotionBackendFailureV1,
    AuthorizedPromotionSubmissionErrorV1, DeploymentStatusPortError, FreshGuildAuthorityError,
    OwnedSessionLoadError, ProductApplicationError, ProductCandidateErrorCodeV1,
    ProductControlPortError, PromotionAuthorityError,
};
use authoring_application_discord::DiscordOAuthError;
use authoring_application_postgres::{
    OAuthFlowError, ProductDatabaseFailureV1, ProductIdentityError,
};
use authoring_promotion::{PendingActivationPortError, PromotionError, PromotionStoreError};
use product_control_http::{FacadeError, FacadeErrorCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApplyInternalErrorCodeV1 {
    AuthenticationBackendTimeout,
    AuthenticationBackendRetryable,
    AuthenticationBackendUnavailable,
    AuthorityStale,
    AuthorityScopeMismatch,
    AuthorityBackend,
    ControlIndeterminate,
    ControlBackend,
    ControlInvalidResult,
    ControlRuntimeDrainConsumeInvalid,
    DeploymentIndeterminate,
    DeploymentBackend,
    InvalidProjection,
}

impl ApplyInternalErrorCodeV1 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationBackendTimeout => "authentication_backend_timeout",
            Self::AuthenticationBackendRetryable => "authentication_backend_retryable",
            Self::AuthenticationBackendUnavailable => "authentication_backend_unavailable",
            Self::AuthorityStale => "authority_stale",
            Self::AuthorityScopeMismatch => "authority_scope_mismatch",
            Self::AuthorityBackend => "authority_backend",
            Self::ControlIndeterminate => "control_indeterminate",
            Self::ControlBackend => "control_backend",
            Self::ControlInvalidResult => "control_invalid_result",
            Self::ControlRuntimeDrainConsumeInvalid => {
                "control_runtime_drain_consume_invalid"
            }
            Self::DeploymentIndeterminate => "deployment_indeterminate",
            Self::DeploymentBackend => "deployment_backend",
            Self::InvalidProjection => "invalid_projection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MappedApplyErrorV1 {
    public: FacadeError,
    internal_code: Option<ApplyInternalErrorCodeV1>,
}

impl MappedApplyErrorV1 {
    pub(crate) fn public(self) -> FacadeError {
        self.public
    }

    pub(crate) fn public_code(self) -> &'static str {
        match self.public.error_code() {
            FacadeErrorCode::AuthenticationRequired => "authentication_required",
            FacadeErrorCode::Forbidden => "forbidden",
            FacadeErrorCode::NotFound => "not_found",
            FacadeErrorCode::StaleGeneration => "stale_generation",
            FacadeErrorCode::StalePayload => "stale_payload",
            FacadeErrorCode::IdempotencyConflict => "idempotency_conflict",
            FacadeErrorCode::InvalidState => "invalid_state",
            FacadeErrorCode::RuntimeDrainRequired => "runtime_drain_required",
            FacadeErrorCode::RuntimeDrainPending => "runtime_drain_pending",
            FacadeErrorCode::Superseded => "superseded",
            FacadeErrorCode::InvalidServerCandidate => "invalid_server_candidate",
            FacadeErrorCode::UpstreamInvalidResponse => "upstream_invalid_response",
            FacadeErrorCode::AuthoringSaturated => "authoring_saturated",
            FacadeErrorCode::DependencyUnavailable => "dependency_unavailable",
            FacadeErrorCode::DependencyTimeout => "dependency_timeout",
            FacadeErrorCode::Internal => "internal_error",
        }
    }

    pub(crate) fn internal_code(self) -> Option<&'static str> {
        self.internal_code.map(ApplyInternalErrorCodeV1::as_str)
    }
}

pub fn map_authentication_error(error: AuthenticationError) -> FacadeError {
    let code = match error {
        AuthenticationError::InvalidCredential
        | AuthenticationError::Expired
        | AuthenticationError::Revoked => FacadeErrorCode::AuthenticationRequired,
        AuthenticationError::InvalidCsrf => FacadeErrorCode::Forbidden,
        AuthenticationError::Backend(failure) => map_authentication_backend(failure),
    };
    facade(code)
}

pub fn map_fresh_authority_error(error: FreshGuildAuthorityError) -> FacadeError {
    let code = match error {
        FreshGuildAuthorityError::InstallationNotFound | FreshGuildAuthorityError::Forbidden => {
            FacadeErrorCode::NotFound
        }
        FreshGuildAuthorityError::Stale => FacadeErrorCode::DependencyTimeout,
        FreshGuildAuthorityError::ScopeMismatch => FacadeErrorCode::Internal,
        FreshGuildAuthorityError::Backend(_) => FacadeErrorCode::DependencyUnavailable,
    };
    facade(code)
}

pub fn map_product_control_error(error: ProductControlPortError) -> FacadeError {
    let code = match error {
        ProductControlPortError::NotFound | ProductControlPortError::ScopeMismatch => {
            FacadeErrorCode::NotFound
        }
        ProductControlPortError::RevisionConflict => FacadeErrorCode::InvalidState,
        ProductControlPortError::PayloadMismatch => FacadeErrorCode::StalePayload,
        ProductControlPortError::InvalidState
        | ProductControlPortError::LifecycleCancelled(_)
        | ProductControlPortError::DuplicateDecision
        | ProductControlPortError::Expired => FacadeErrorCode::InvalidState,
        ProductControlPortError::RuntimeDrainRequired => FacadeErrorCode::RuntimeDrainRequired,
        ProductControlPortError::RuntimeDrainPending(_) => FacadeErrorCode::RuntimeDrainPending,
        ProductControlPortError::IdempotencyConflict => FacadeErrorCode::IdempotencyConflict,
        ProductControlPortError::InvalidServerCandidate(candidate) => map_candidate(candidate),
        ProductControlPortError::Superseded => FacadeErrorCode::Superseded,
        ProductControlPortError::Indeterminate(_) | ProductControlPortError::Backend(_) => {
            FacadeErrorCode::DependencyUnavailable
        }
    };
    facade(code)
}

pub fn map_product_application_error(error: ProductApplicationError) -> FacadeError {
    match error {
        ProductApplicationError::Authentication(error) => map_authentication_error(error),
        ProductApplicationError::FreshAuthority(error) => map_fresh_authority_error(error),
        ProductApplicationError::Control(error) => map_product_control_error(error),
        ProductApplicationError::Deployment(error) => map_deployment_error(error),
        ProductApplicationError::InvalidProjection => facade(FacadeErrorCode::Internal),
    }
}

pub(crate) fn map_apply_error(error: ProductApplicationError) -> MappedApplyErrorV1 {
    let internal_code = match &error {
        ProductApplicationError::Authentication(error) => match error {
            AuthenticationError::Backend(error) => match error {
                AuthenticationBackendFailureV1::Timeout => {
                    Some(ApplyInternalErrorCodeV1::AuthenticationBackendTimeout)
                }
                AuthenticationBackendFailureV1::Retryable => {
                    Some(ApplyInternalErrorCodeV1::AuthenticationBackendRetryable)
                }
                AuthenticationBackendFailureV1::Unavailable => {
                    Some(ApplyInternalErrorCodeV1::AuthenticationBackendUnavailable)
                }
            },
            AuthenticationError::InvalidCredential
            | AuthenticationError::InvalidCsrf
            | AuthenticationError::Expired
            | AuthenticationError::Revoked => None,
        },
        ProductApplicationError::FreshAuthority(error) => match error {
            FreshGuildAuthorityError::Stale => Some(ApplyInternalErrorCodeV1::AuthorityStale),
            FreshGuildAuthorityError::ScopeMismatch => {
                Some(ApplyInternalErrorCodeV1::AuthorityScopeMismatch)
            }
            FreshGuildAuthorityError::Backend(_) => {
                Some(ApplyInternalErrorCodeV1::AuthorityBackend)
            }
            FreshGuildAuthorityError::InstallationNotFound
            | FreshGuildAuthorityError::Forbidden => None,
        },
        ProductApplicationError::Control(error) => match error {
            ProductControlPortError::Indeterminate(_) => {
                Some(ApplyInternalErrorCodeV1::ControlIndeterminate)
            }
            ProductControlPortError::Backend(detail) => match detail.as_str() {
                "product apply function returned an invalid result" => {
                    Some(ApplyInternalErrorCodeV1::ControlInvalidResult)
                }
                "product apply runtime drain consume returned an invalid result" => {
                    Some(ApplyInternalErrorCodeV1::ControlRuntimeDrainConsumeInvalid)
                }
                _ => Some(ApplyInternalErrorCodeV1::ControlBackend),
            },
            ProductControlPortError::NotFound
            | ProductControlPortError::ScopeMismatch
            | ProductControlPortError::RevisionConflict
            | ProductControlPortError::PayloadMismatch
            | ProductControlPortError::InvalidState
            | ProductControlPortError::RuntimeDrainRequired
            | ProductControlPortError::RuntimeDrainPending(_)
            | ProductControlPortError::LifecycleCancelled(_)
            | ProductControlPortError::DuplicateDecision
            | ProductControlPortError::Expired
            | ProductControlPortError::IdempotencyConflict
            | ProductControlPortError::InvalidServerCandidate(_)
            | ProductControlPortError::Superseded => None,
        },
        ProductApplicationError::Deployment(error) => match error {
            DeploymentStatusPortError::Indeterminate(_) => {
                Some(ApplyInternalErrorCodeV1::DeploymentIndeterminate)
            }
            DeploymentStatusPortError::Backend(_) => {
                Some(ApplyInternalErrorCodeV1::DeploymentBackend)
            }
            DeploymentStatusPortError::NotFound => None,
        },
        ProductApplicationError::InvalidProjection => {
            Some(ApplyInternalErrorCodeV1::InvalidProjection)
        }
    };
    MappedApplyErrorV1 {
        public: map_product_application_error(error),
        internal_code,
    }
}

pub fn map_authoring_application_error(error: AuthoringApplicationError) -> FacadeError {
    match error {
        AuthoringApplicationError::Authentication(error) => map_authentication_error(error),
        AuthoringApplicationError::FreshAuthority(error) => map_fresh_authority_error(error),
        AuthoringApplicationError::Session(error) => map_session_error(error),
        AuthoringApplicationError::Authority(error) => map_promotion_authority_error(error),
        AuthoringApplicationError::Promotion(error) => map_promotion_error(error),
        AuthoringApplicationError::AuthorizedPromotion(error) => {
            map_authorized_promotion_error(error)
        }
    }
}

pub fn map_authoring_conversation_error(error: AuthoringConversationError) -> FacadeError {
    match error {
        AuthoringConversationError::Authentication(error) => map_authentication_error(error),
        AuthoringConversationError::Authority(error) => map_fresh_authority_error(error),
        AuthoringConversationError::Admission(error) => facade(match error {
            AuthoringAdmissionError::Saturated => FacadeErrorCode::AuthoringSaturated,
            AuthoringAdmissionError::Unavailable => FacadeErrorCode::DependencyUnavailable,
        }),
        AuthoringConversationError::Store(error) => facade(match error {
            AuthoringSessionLoadError::Timeout => FacadeErrorCode::DependencyTimeout,
            AuthoringSessionLoadError::Unavailable | AuthoringSessionLoadError::Retryable => {
                FacadeErrorCode::DependencyUnavailable
            }
            AuthoringSessionLoadError::InvalidState => FacadeErrorCode::Internal,
        }),
        AuthoringConversationError::Observation(error) => facade(match error {
            AuthoringSessionObservationErrorV1::NotFound
            | AuthoringSessionObservationErrorV1::InvalidState => FacadeErrorCode::NotFound,
            AuthoringSessionObservationErrorV1::Timeout => FacadeErrorCode::DependencyTimeout,
            AuthoringSessionObservationErrorV1::Retryable
            | AuthoringSessionObservationErrorV1::Unavailable => {
                FacadeErrorCode::DependencyUnavailable
            }
        }),
        AuthoringConversationError::IdempotencyConflict => {
            facade(FacadeErrorCode::IdempotencyConflict)
        }
        AuthoringConversationError::GenerationConflict { .. } => {
            facade(FacadeErrorCode::StaleGeneration)
        }
        AuthoringConversationError::AuthorityDrift | AuthoringConversationError::BindingDrift => {
            facade(FacadeErrorCode::InvalidState)
        }
        AuthoringConversationError::TurnHalted { .. } => {
            facade(FacadeErrorCode::DependencyUnavailable)
        }
        AuthoringConversationError::CancelledBeforeCommit => {
            facade(FacadeErrorCode::DependencyTimeout)
        }
        AuthoringConversationError::Projection(_)
        | AuthoringConversationError::ExpectedGeneration(_)
        | AuthoringConversationError::InvalidSession
        | AuthoringConversationError::InvalidModelCallCount
        | AuthoringConversationError::InvalidCommit => facade(FacadeErrorCode::Internal),
    }
}

pub fn map_discord_oauth_error(error: DiscordOAuthError) -> FacadeError {
    let code = match error {
        DiscordOAuthError::ExchangeRejected => FacadeErrorCode::AuthenticationRequired,
        DiscordOAuthError::Unavailable | DiscordOAuthError::RevocationFailed => {
            FacadeErrorCode::DependencyUnavailable
        }
        DiscordOAuthError::Timeout => FacadeErrorCode::DependencyTimeout,
        DiscordOAuthError::InvalidResponse | DiscordOAuthError::ResponseTooLarge => {
            FacadeErrorCode::UpstreamInvalidResponse
        }
    };
    facade(code)
}

pub fn map_oauth_flow_error(error: OAuthFlowError) -> FacadeError {
    match error {
        OAuthFlowError::InvalidRequest => facade(FacadeErrorCode::Internal),
        OAuthFlowError::InvalidOrConsumed => facade(FacadeErrorCode::AuthenticationRequired),
        OAuthFlowError::SecretGeneration => facade(FacadeErrorCode::DependencyUnavailable),
        OAuthFlowError::Database(error) => map_database_failure(error),
        OAuthFlowError::Invariant => facade(FacadeErrorCode::Internal),
        OAuthFlowError::CommitIndeterminate => facade(FacadeErrorCode::DependencyUnavailable),
    }
}

pub fn map_product_identity_error(error: ProductIdentityError) -> FacadeError {
    match error {
        ProductIdentityError::FlowInvalidOrConsumed
        | ProductIdentityError::InvalidCredential
        | ProductIdentityError::Expired
        | ProductIdentityError::Revoked
        | ProductIdentityError::PrincipalDisabled => {
            facade(FacadeErrorCode::AuthenticationRequired)
        }
        ProductIdentityError::InvalidCsrf => facade(FacadeErrorCode::Forbidden),
        ProductIdentityError::SecretGeneration => facade(FacadeErrorCode::DependencyUnavailable),
        ProductIdentityError::Database(error) => map_database_failure(error),
        ProductIdentityError::Invariant => facade(FacadeErrorCode::Internal),
        ProductIdentityError::CommitIndeterminate => facade(FacadeErrorCode::DependencyUnavailable),
    }
}

pub fn map_database_failure(error: ProductDatabaseFailureV1) -> FacadeError {
    facade(match error {
        ProductDatabaseFailureV1::Timeout => FacadeErrorCode::DependencyTimeout,
        ProductDatabaseFailureV1::Retryable | ProductDatabaseFailureV1::Unavailable => {
            FacadeErrorCode::DependencyUnavailable
        }
    })
}

fn map_authentication_backend(error: AuthenticationBackendFailureV1) -> FacadeErrorCode {
    match error {
        AuthenticationBackendFailureV1::Timeout => FacadeErrorCode::DependencyTimeout,
        AuthenticationBackendFailureV1::Retryable | AuthenticationBackendFailureV1::Unavailable => {
            FacadeErrorCode::DependencyUnavailable
        }
    }
}

fn map_candidate(error: ProductCandidateErrorCodeV1) -> FacadeErrorCode {
    match error {
        ProductCandidateErrorCodeV1::TargetCorrupt
        | ProductCandidateErrorCodeV1::BindingRevisionUnavailable
        | ProductCandidateErrorCodeV1::UnsupportedSchema
        | ProductCandidateErrorCodeV1::StructurallyInvalid
        | ProductCandidateErrorCodeV1::HashComputationFailed
        | ProductCandidateErrorCodeV1::HashMismatch
        | ProductCandidateErrorCodeV1::BindingInvalid
        | ProductCandidateErrorCodeV1::BlockingPolicy
        | ProductCandidateErrorCodeV1::MissingCapabilities
        | ProductCandidateErrorCodeV1::RoleHierarchyUnavailable
        | ProductCandidateErrorCodeV1::RoleHierarchyIncomplete
        | ProductCandidateErrorCodeV1::RoleUnmanageable => FacadeErrorCode::InvalidServerCandidate,
    }
}

fn map_deployment_error(error: DeploymentStatusPortError) -> FacadeError {
    facade(match error {
        DeploymentStatusPortError::NotFound => FacadeErrorCode::NotFound,
        DeploymentStatusPortError::Indeterminate(_) | DeploymentStatusPortError::Backend(_) => {
            FacadeErrorCode::DependencyUnavailable
        }
    })
}

fn map_session_error(error: OwnedSessionLoadError) -> FacadeError {
    facade(match error {
        OwnedSessionLoadError::NotFound | OwnedSessionLoadError::NotOwned => {
            FacadeErrorCode::NotFound
        }
        OwnedSessionLoadError::GenerationMismatch => FacadeErrorCode::StaleGeneration,
        OwnedSessionLoadError::NotPreviewReady => FacadeErrorCode::InvalidState,
        OwnedSessionLoadError::Backend(_) => FacadeErrorCode::DependencyUnavailable,
    })
}

fn map_promotion_authority_error(error: PromotionAuthorityError) -> FacadeError {
    facade(match error {
        PromotionAuthorityError::NotFound => FacadeErrorCode::NotFound,
        PromotionAuthorityError::Forbidden => FacadeErrorCode::Forbidden,
        PromotionAuthorityError::GenerationMismatch => FacadeErrorCode::StaleGeneration,
        PromotionAuthorityError::ScopeMismatch | PromotionAuthorityError::InvalidIdempotencyKey => {
            FacadeErrorCode::Internal
        }
        PromotionAuthorityError::Backend(_) => FacadeErrorCode::DependencyUnavailable,
    })
}

fn map_authorized_promotion_error(error: AuthorizedPromotionSubmissionErrorV1) -> FacadeError {
    facade(match error {
        AuthorizedPromotionSubmissionErrorV1::NotFound
        | AuthorizedPromotionSubmissionErrorV1::ScopeMismatch => FacadeErrorCode::NotFound,
        AuthorizedPromotionSubmissionErrorV1::GenerationMismatch => {
            FacadeErrorCode::StaleGeneration
        }
        AuthorizedPromotionSubmissionErrorV1::Forbidden => FacadeErrorCode::Forbidden,
        AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict => {
            FacadeErrorCode::IdempotencyConflict
        }
        AuthorizedPromotionSubmissionErrorV1::InvalidCandidate => {
            FacadeErrorCode::InvalidServerCandidate
        }
        AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt => FacadeErrorCode::Internal,
        AuthorizedPromotionSubmissionErrorV1::Indeterminate => {
            FacadeErrorCode::DependencyUnavailable
        }
        AuthorizedPromotionSubmissionErrorV1::Backend(error) => match error {
            AuthorizedPromotionBackendFailureV1::Timeout => FacadeErrorCode::DependencyTimeout,
            AuthorizedPromotionBackendFailureV1::Retryable
            | AuthorizedPromotionBackendFailureV1::Unavailable => {
                FacadeErrorCode::DependencyUnavailable
            }
        },
    })
}

fn map_promotion_error(error: PromotionError) -> FacadeError {
    facade(match error {
        PromotionError::ValidatedPreviewRequired
        | PromotionError::InvalidArtifactHash { .. }
        | PromotionError::ArtifactCountOverflow { .. }
        | PromotionError::InvalidPolicy
        | PromotionError::Hash(_)
        | PromotionError::Digest(_) => FacadeErrorCode::InvalidServerCandidate,
        PromotionError::SessionOwnerMismatch
        | PromotionError::PublicationMismatch
        | PromotionError::PendingActivationMismatch
        | PromotionError::ActivationIdentity => FacadeErrorCode::Internal,
        PromotionError::NotFound => FacadeErrorCode::NotFound,
        PromotionError::ConcurrentTransitionLimit => FacadeErrorCode::DependencyUnavailable,
        PromotionError::RuleSet(error) => map_ruleset_store_error(error),
        PromotionError::PendingActivation(error) => match error {
            PendingActivationPortError::Conflict(_)
            | PendingActivationPortError::Indeterminate(_)
            | PendingActivationPortError::Backend(_) => FacadeErrorCode::DependencyUnavailable,
        },
        PromotionError::Store(error) => match error {
            PromotionStoreError::IdempotencyConflict => FacadeErrorCode::IdempotencyConflict,
            PromotionStoreError::NotFound => FacadeErrorCode::NotFound,
            PromotionStoreError::RevisionConflict { .. } => FacadeErrorCode::DependencyUnavailable,
            PromotionStoreError::InvalidTransition
            | PromotionStoreError::RevisionOverflow
            | PromotionStoreError::InvalidRecord(_) => FacadeErrorCode::Internal,
            PromotionStoreError::Backend(_) => FacadeErrorCode::DependencyUnavailable,
        },
    })
}

fn map_ruleset_store_error(error: automation_ruleset::RuleSetStoreError) -> FacadeErrorCode {
    match error {
        automation_ruleset::RuleSetStoreError::InvalidDefinition(_)
        | automation_ruleset::RuleSetStoreError::TargetHashMismatch
        | automation_ruleset::RuleSetStoreError::GuardedActivationUnsupported
        | automation_ruleset::RuleSetStoreError::Canonicalization(_) => {
            FacadeErrorCode::InvalidServerCandidate
        }
        automation_ruleset::RuleSetStoreError::VersionNotFound
        | automation_ruleset::RuleSetStoreError::VersionOverflow
        | automation_ruleset::RuleSetStoreError::HashCollision => FacadeErrorCode::Internal,
        automation_ruleset::RuleSetStoreError::Backend(_) => FacadeErrorCode::DependencyUnavailable,
    }
}

fn facade(code: FacadeErrorCode) -> FacadeError {
    FacadeError::new(code)
}

#[cfg(test)]
mod tests {
    use authoring_application::ProductDrainSelectorV1;

    use super::*;

    fn code(error: FacadeError) -> FacadeErrorCode {
        error.error_code()
    }

    #[test]
    fn authentication_and_installation_authority_errors_keep_closed_security_semantics() {
        for error in [
            AuthenticationError::InvalidCredential,
            AuthenticationError::Expired,
            AuthenticationError::Revoked,
        ] {
            assert_eq!(
                code(map_authentication_error(error)),
                FacadeErrorCode::AuthenticationRequired
            );
        }
        assert_eq!(
            code(map_authentication_error(AuthenticationError::InvalidCsrf)),
            FacadeErrorCode::Forbidden
        );
        assert_eq!(
            code(map_authentication_error(AuthenticationError::Backend(
                AuthenticationBackendFailureV1::Timeout,
            ))),
            FacadeErrorCode::DependencyTimeout
        );
        for error in [
            FreshGuildAuthorityError::InstallationNotFound,
            FreshGuildAuthorityError::Forbidden,
        ] {
            assert_eq!(
                code(map_fresh_authority_error(error)),
                FacadeErrorCode::NotFound
            );
        }
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::FreshAuthority(FreshGuildAuthorityError::Forbidden),
            )),
            FacadeErrorCode::NotFound
        );
        assert_eq!(
            code(map_product_application_error(
                ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Forbidden),
            )),
            FacadeErrorCode::NotFound
        );
        assert_eq!(
            code(map_fresh_authority_error(
                FreshGuildAuthorityError::ScopeMismatch,
            )),
            FacadeErrorCode::Internal
        );
        assert_eq!(
            code(map_fresh_authority_error(
                FreshGuildAuthorityError::Backend("sensitive".to_string(),)
            )),
            FacadeErrorCode::DependencyUnavailable
        );
    }

    #[test]
    fn product_control_errors_are_non_enumerating_and_preserve_conflict_kinds() {
        for error in [
            ProductControlPortError::NotFound,
            ProductControlPortError::ScopeMismatch,
        ] {
            assert_eq!(
                code(map_product_control_error(error)),
                FacadeErrorCode::NotFound
            );
        }
        assert_eq!(
            code(map_product_control_error(
                ProductControlPortError::RevisionConflict,
            )),
            FacadeErrorCode::InvalidState
        );
        assert_eq!(
            code(map_product_control_error(
                ProductControlPortError::PayloadMismatch,
            )),
            FacadeErrorCode::StalePayload
        );
        assert_eq!(
            code(map_product_control_error(
                ProductControlPortError::IdempotencyConflict,
            )),
            FacadeErrorCode::IdempotencyConflict
        );
        assert_eq!(
            code(map_product_control_error(
                ProductControlPortError::Superseded,
            )),
            FacadeErrorCode::Superseded
        );
        let drain_required =
            map_product_control_error(ProductControlPortError::RuntimeDrainRequired);
        assert_eq!(
            drain_required.error_code(),
            FacadeErrorCode::RuntimeDrainRequired
        );
        assert!(drain_required.retryable());
        let selector = ProductDrainSelectorV1::from_server_projection(
            "1".repeat(32),
            7,
            "b".repeat(64),
            "2".repeat(32),
            10,
        )
        .unwrap();
        let drain_pending =
            map_product_control_error(ProductControlPortError::RuntimeDrainPending(selector));
        assert_eq!(
            drain_pending.error_code(),
            FacadeErrorCode::RuntimeDrainPending
        );
        assert!(drain_pending.retryable());
    }

    #[test]
    fn every_server_candidate_code_stays_a_422_classification() {
        let candidates = [
            ProductCandidateErrorCodeV1::TargetCorrupt,
            ProductCandidateErrorCodeV1::BindingRevisionUnavailable,
            ProductCandidateErrorCodeV1::UnsupportedSchema,
            ProductCandidateErrorCodeV1::StructurallyInvalid,
            ProductCandidateErrorCodeV1::HashComputationFailed,
            ProductCandidateErrorCodeV1::HashMismatch,
            ProductCandidateErrorCodeV1::BindingInvalid,
            ProductCandidateErrorCodeV1::BlockingPolicy,
            ProductCandidateErrorCodeV1::MissingCapabilities,
            ProductCandidateErrorCodeV1::RoleHierarchyUnavailable,
            ProductCandidateErrorCodeV1::RoleHierarchyIncomplete,
            ProductCandidateErrorCodeV1::RoleUnmanageable,
        ];
        for candidate in candidates {
            assert_eq!(
                code(map_product_control_error(
                    ProductControlPortError::InvalidServerCandidate(candidate),
                )),
                FacadeErrorCode::InvalidServerCandidate
            );
        }
    }

    #[test]
    fn authoring_errors_keep_generation_candidate_and_backend_boundaries() {
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::Session(OwnedSessionLoadError::GenerationMismatch),
            )),
            FacadeErrorCode::StaleGeneration
        );
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::Session(OwnedSessionLoadError::NotPreviewReady),
            )),
            FacadeErrorCode::InvalidState
        );
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::AuthorizedPromotion(
                    AuthorizedPromotionSubmissionErrorV1::Backend(
                        AuthorizedPromotionBackendFailureV1::Timeout,
                    ),
                ),
            )),
            FacadeErrorCode::DependencyTimeout
        );
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::AuthorizedPromotion(
                    AuthorizedPromotionSubmissionErrorV1::ScopeMismatch,
                ),
            )),
            FacadeErrorCode::NotFound
        );
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::Promotion(PromotionError::RuleSet(
                    automation_ruleset::RuleSetStoreError::TargetHashMismatch,
                )),
            )),
            FacadeErrorCode::InvalidServerCandidate
        );
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::Promotion(PromotionError::RuleSet(
                    automation_ruleset::RuleSetStoreError::HashCollision,
                )),
            )),
            FacadeErrorCode::Internal
        );
        assert_eq!(
            code(map_authoring_application_error(
                AuthoringApplicationError::Promotion(PromotionError::RuleSet(
                    automation_ruleset::RuleSetStoreError::Backend("sensitive".to_string()),
                )),
            )),
            FacadeErrorCode::DependencyUnavailable
        );
    }

    #[test]
    fn conversation_errors_preserve_capacity_conflicts_and_timeouts() {
        let saturated = map_authoring_conversation_error(AuthoringConversationError::Admission(
            AuthoringAdmissionError::Saturated,
        ));
        assert_eq!(code(saturated), FacadeErrorCode::AuthoringSaturated);
        assert!(saturated.retryable());
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::IdempotencyConflict,
            )),
            FacadeErrorCode::IdempotencyConflict
        );
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::GenerationConflict {
                    current_generation: None,
                },
            )),
            FacadeErrorCode::StaleGeneration
        );
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::Store(AuthoringSessionLoadError::Timeout),
            )),
            FacadeErrorCode::DependencyTimeout
        );
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::CancelledBeforeCommit,
            )),
            FacadeErrorCode::DependencyTimeout
        );
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::TurnHalted {
                    code: "sensitive-upstream-detail".to_string(),
                },
            )),
            FacadeErrorCode::DependencyUnavailable
        );
    }

    #[test]
    fn conversation_observation_failures_do_not_reveal_session_existence() {
        for error in [
            AuthoringSessionObservationErrorV1::NotFound,
            AuthoringSessionObservationErrorV1::InvalidState,
        ] {
            assert_eq!(
                code(map_authoring_conversation_error(
                    AuthoringConversationError::Observation(error),
                )),
                FacadeErrorCode::NotFound
            );
        }
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::Observation(
                    AuthoringSessionObservationErrorV1::Timeout,
                ),
            )),
            FacadeErrorCode::DependencyTimeout
        );
        assert_eq!(
            code(map_authoring_conversation_error(
                AuthoringConversationError::Authority(FreshGuildAuthorityError::Forbidden),
            )),
            FacadeErrorCode::NotFound
        );
    }

    #[test]
    fn oauth_and_identity_errors_never_expose_backend_detail() {
        assert_eq!(
            code(map_discord_oauth_error(DiscordOAuthError::InvalidResponse)),
            FacadeErrorCode::UpstreamInvalidResponse
        );
        assert_eq!(
            code(map_discord_oauth_error(DiscordOAuthError::Timeout)),
            FacadeErrorCode::DependencyTimeout
        );
        assert_eq!(
            code(map_oauth_flow_error(OAuthFlowError::InvalidOrConsumed)),
            FacadeErrorCode::AuthenticationRequired
        );
        assert_eq!(
            code(map_product_identity_error(
                ProductIdentityError::PrincipalDisabled,
            )),
            FacadeErrorCode::AuthenticationRequired
        );
        assert_eq!(
            code(map_product_identity_error(ProductIdentityError::Database(
                ProductDatabaseFailureV1::Retryable,
            ))),
            FacadeErrorCode::DependencyUnavailable
        );
    }

    #[test]
    fn invalid_projection_and_string_bearing_failures_fail_closed() {
        assert_eq!(
            code(map_product_application_error(
                ProductApplicationError::InvalidProjection,
            )),
            FacadeErrorCode::Internal
        );
        assert_eq!(
            code(map_product_application_error(
                ProductApplicationError::Deployment(DeploymentStatusPortError::Backend(
                    "credential".to_string(),
                )),
            )),
            FacadeErrorCode::DependencyUnavailable
        );
        assert_eq!(
            code(map_product_control_error(
                ProductControlPortError::Indeterminate("payload".to_string(),)
            )),
            FacadeErrorCode::DependencyUnavailable
        );
    }

    #[test]
    fn apply_error_mapping_has_stable_internal_codes_without_changing_public_errors() {
        let invalid_projection = map_apply_error(ProductApplicationError::InvalidProjection);
        assert_eq!(
            invalid_projection.internal_code(),
            Some("invalid_projection")
        );
        assert_eq!(
            invalid_projection.public().error_code(),
            FacadeErrorCode::Internal
        );

        let known = map_apply_error(ProductApplicationError::Control(
            ProductControlPortError::Backend(
                "product apply function returned an invalid result".to_string(),
            ),
        ));
        assert_eq!(known.internal_code(), Some("control_invalid_result"));

        let consume = map_apply_error(ProductApplicationError::Control(
            ProductControlPortError::Backend(
                "product apply runtime drain consume returned an invalid result".to_string(),
            ),
        ));
        assert_eq!(
            consume.internal_code(),
            Some("control_runtime_drain_consume_invalid")
        );

        let secret = "postgres://user:secret@private-host/database";
        let backend = map_apply_error(ProductApplicationError::Control(
            ProductControlPortError::Backend(secret.to_string()),
        ));
        assert_eq!(backend.internal_code(), Some("control_backend"));
        assert_eq!(backend.public_code(), "dependency_unavailable");
        assert_eq!(
            backend.public().error_code(),
            FacadeErrorCode::DependencyUnavailable
        );
        assert!(backend.public().retryable());
        assert!(!backend.internal_code().unwrap().contains(secret));
    }
}
