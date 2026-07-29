use std::num::NonZeroU32;
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application::{
    AuthoringMutationDispositionV1, AuthoringSessionObservationV1, AuthoringTurnOutcomeV1,
    DeploymentConvergencePhaseV2, DeploymentOperationalObservationV2,
    DeploymentOperatorActionV2 as ApplicationOperatorActionV2, DeploymentRetryObservationV2,
    DeploymentServingFreshnessV2, DeploymentStatusObservationV1, DeploymentStatusProjectionV1,
    DeploymentStatusV1, ProductApplyResultV1, ProductApprovalPreviewObservationV1,
    ProductDecisionPhaseV1, ProductDecisionProjectionV1, ProductDeploymentOperationalStatusV2,
    ProductDeploymentStatusObservationV1, ProductLifecycleCancellationReceiptV1,
    ProductMutationReceiptV1, ProductPromotionObservationV1, ProductPromotionStateV1,
    ProductStatusObservationV1, ProductStatusV1,
};
use authoring_application_discord::DiscordApplicationIdV1;
use authoring_application_postgres::{
    CurrentProductPrincipalV1, IssuedProductSessionV1, OAuthFlowIssueV1,
};
use authoring_promotion::AuthoringSessionId;
use chrono::{DateTime, Utc};
use product_control_http::{
    ApplyView, ApprovalPreviewView, AuthoringSessionViewV1, AuthoringTurnDispositionV1,
    AuthoringTurnViewV1, CsrfSecret, CurrentPrincipal, DecisionView, DeploymentAttestationViewV2,
    DeploymentFailureViewV2, DeploymentOperationalStateV2, DeploymentOperationalViewV2,
    DeploymentOperatorActionV2, DeploymentRetryStateV2, DeploymentRetryViewV2,
    DeploymentRuntimePhaseV2, DeploymentServingFreshnessStateV2, DeploymentServingFreshnessViewV2,
    DeploymentState, DeploymentView, DiscordAuthorizationRequest, FacadeError, FacadeErrorCode,
    LifecycleCancellationView, OAuthCallbackResult, OAuthStartResult, OAuthState, ProductState,
    PromotionView, RuntimeDeploymentOperationalViewV2, SafeApprovalSummary, SessionCredential,
};

pub fn project_authoring_turn(
    expected_session_id: &AuthoringSessionId,
    outcome: &AuthoringTurnOutcomeV1,
) -> Result<AuthoringTurnViewV1, FacadeError> {
    outcome.projection().validate().map_err(|_| internal())?;
    match outcome {
        AuthoringTurnOutcomeV1::Committed(receipt) => {
            if receipt.session_id() != expected_session_id
                || matches!(
                    receipt.projection().state(),
                    authoring_application::SafeAuthoringTurnStateV1::Unsupported
                        | authoring_application::SafeAuthoringTurnStateV1::Rejected
                )
            {
                return Err(internal());
            }
            Ok(AuthoringTurnViewV1 {
                session_id: receipt.session_id().as_str().to_string(),
                generation: Some(receipt.generation().get()),
                disposition: Some(match receipt.disposition() {
                    AuthoringMutationDispositionV1::Created => AuthoringTurnDispositionV1::Created,
                    AuthoringMutationDispositionV1::ExactReplay => {
                        AuthoringTurnDispositionV1::ExactReplay
                    }
                }),
                projection: receipt.projection().clone(),
            })
        }
        AuthoringTurnOutcomeV1::NotCommitted(projection) => {
            if !matches!(
                projection.state(),
                authoring_application::SafeAuthoringTurnStateV1::Unsupported
                    | authoring_application::SafeAuthoringTurnStateV1::Rejected
            ) {
                return Err(internal());
            }
            Ok(AuthoringTurnViewV1 {
                session_id: expected_session_id.as_str().to_string(),
                generation: None,
                disposition: None,
                projection: projection.clone(),
            })
        }
    }
}

pub fn project_authoring_session(
    observation: &AuthoringSessionObservationV1,
) -> Result<AuthoringSessionViewV1, FacadeError> {
    observation
        .projection()
        .validate()
        .map_err(|_| internal())?;
    if matches!(
        observation.projection().state(),
        authoring_application::SafeAuthoringTurnStateV1::Unsupported
            | authoring_application::SafeAuthoringTurnStateV1::Rejected
    ) {
        return Err(internal());
    }
    Ok(AuthoringSessionViewV1 {
        session_id: observation.session_id().as_str().to_string(),
        observed_generation: observation.generation().get(),
        projection: observation.projection().clone(),
    })
}

pub fn project_oauth_start(
    issue: &OAuthFlowIssueV1,
    application_id: DiscordApplicationIdV1,
) -> Result<OAuthStartResult, FacadeError> {
    project_oauth_start_parts(
        issue.state().expose_secret(),
        issue.browser_nonce().expose_secret(),
        issue.redirect_uri(),
        issue.max_age_seconds(),
        application_id,
    )
}

pub fn project_oauth_callback(
    session: &IssuedProductSessionV1,
) -> Result<OAuthCallbackResult, FacadeError> {
    project_oauth_callback_parts(
        session.session().expose_secret(),
        session.csrf().expose_secret(),
        session.return_path(),
        session.max_age_seconds(),
    )
}

pub fn project_current_principal(principal: &CurrentProductPrincipalV1) -> CurrentPrincipal {
    CurrentPrincipal {
        principal_id: principal.principal_id().as_str().to_string(),
        display_name: principal.display_name().to_string(),
    }
}

pub fn project_promotion(
    promotion: &ProductPromotionObservationV1,
    current: &ProductStatusObservationV1,
) -> Result<PromotionView, FacadeError> {
    let decision = current.decision();
    if promotion.promotion_id() != decision.promotion_id() || promotion.revision() == 0 {
        return Err(internal());
    }
    match promotion.state() {
        ProductPromotionStateV1::ActivationLinked => {}
        ProductPromotionStateV1::Expired if current.status() == ProductStatusV1::Expired => {}
        ProductPromotionStateV1::Expired => return Err(internal()),
    }
    ensure_product_status_matches_decision(current.status(), decision.phase())?;
    if !promotion.exact_replay() && current.status() != ProductStatusV1::PendingApproval {
        return Err(internal());
    }
    Ok(PromotionView {
        installation_id: decision.installation_id().as_str().to_string(),
        promotion_id: promotion.promotion_id().as_str().to_string(),
        revision: decision.revision().get(),
        state: product_state(current.status()),
        payload_digest: promotion.approval_payload_digest().as_str().to_string(),
        replayed: promotion.exact_replay(),
    })
}

pub fn project_product_status(
    observation: &ProductStatusObservationV1,
) -> Result<DecisionView, FacadeError> {
    ensure_product_status_matches_decision(observation.status(), observation.decision().phase())?;
    Ok(decision_view(
        observation.decision(),
        product_state(observation.status()),
        false,
    ))
}

pub fn project_decision_mutation(receipt: &ProductMutationReceiptV1) -> DecisionView {
    decision_view(
        receipt.projection(),
        product_state_from_phase(receipt.projection().phase()),
        receipt.exact_replay(),
    )
}

pub fn project_approval_preview(
    observation: &ProductApprovalPreviewObservationV1,
) -> Result<ApprovalPreviewView, FacadeError> {
    let preview = observation.preview();
    let payload = preview.payload();
    let summary = &payload.preview.summary;
    let state = match preview.phase() {
        ProductDecisionPhaseV1::PendingApproval => ProductState::PendingApproval,
        ProductDecisionPhaseV1::Approved => ProductState::Approved,
        ProductDecisionPhaseV1::Applying
        | ProductDecisionPhaseV1::Applied { .. }
        | ProductDecisionPhaseV1::Rejected
        | ProductDecisionPhaseV1::Expired
        | ProductDecisionPhaseV1::Superseded
        | ProductDecisionPhaseV1::Withdrawn => return Err(internal()),
    };
    Ok(ApprovalPreviewView {
        installation_id: preview.installation_id().as_str().to_string(),
        promotion_id: payload.promotion_id.as_str().to_string(),
        revision: preview.revision().get(),
        state,
        payload_digest: preview.payload_digest().as_str().to_string(),
        summary: SafeApprovalSummary {
            panels: summary.panels,
            modals: summary.modals,
            rules: summary.rules,
            actions: summary.actions,
            target_version: payload.target.version.get(),
            target_content_hash: payload.target.content_hash.to_hex(),
            binding_fingerprint: payload.binding.fingerprint.as_str().to_string(),
            required_approvals: payload.policy.required_approvals.get(),
            expires_at: system_time_to_utc(observation.activation_expires_at())?,
        },
    })
}

pub fn project_apply(result: &ProductApplyResultV1) -> ApplyView {
    ApplyView {
        installation_id: result
            .exact_deployment()
            .installation_id()
            .as_str()
            .to_string(),
        promotion_id: result
            .exact_deployment()
            .promotion_id()
            .as_str()
            .to_string(),
        state: product_state(result.status()),
        replayed: result.exact_replay(),
    }
}

pub fn project_lifecycle_cancellation(
    receipt: &ProductLifecycleCancellationReceiptV1,
) -> Result<LifecycleCancellationView, FacadeError> {
    let decision = receipt.decision();
    let source = receipt.source_drain_selector();
    if decision.phase() != &ProductDecisionPhaseV1::Approved {
        return Err(internal());
    }
    Ok(LifecycleCancellationView {
        installation_id: decision.installation_id().as_str().to_string(),
        promotion_id: decision.promotion_id().as_str().to_string(),
        revision: decision.revision().get(),
        state: ProductState::Approved,
        drain_intent_id: source.drain_intent_id().to_string(),
        source_intent_revision: source.acknowledged_intent_revision().get(),
        terminal_intent_revision: receipt.terminal_intent_revision().get(),
        terminal_state_digest: receipt.terminal_state_digest().to_string(),
        product_operation_id: source.product_operation_id().to_string(),
        source_runtime_deployment_revision: source.expected_runtime_deployment_revision().get(),
        resulting_runtime_deployment_revision: receipt
            .resulting_runtime_deployment_revision()
            .get(),
        source_slot_writer_epoch: receipt.source_slot_writer_epoch().get(),
        successor_slot_writer_epoch: receipt.successor_slot_writer_epoch().get(),
        cancelled_at: system_time_to_utc(receipt.cancelled_at())?,
        replayed: receipt.exact_replay(),
    })
}

pub fn project_deployment(
    observation: &ProductDeploymentStatusObservationV1,
) -> Result<DeploymentView, FacadeError> {
    project_deployment_parts(
        observation.decision(),
        observation.decision_observed_at(),
        observation.status(),
        observation.deployment(),
    )
}

pub fn project_deployment_operational_v2(
    observation: &ProductDeploymentOperationalStatusV2,
) -> Result<DeploymentOperationalViewV2, FacadeError> {
    let decision = observation.decision();
    let runtime = match (observation.status(), observation.deployment()) {
        (DeploymentStatusV1::NotApplicable, None) => None,
        (DeploymentStatusV1::Pending, Some(runtime))
        | (DeploymentStatusV1::Failed { .. }, Some(runtime))
        | (DeploymentStatusV1::Live { .. }, Some(runtime)) => {
            ensure_operational_status_matches(observation.status(), runtime)?;
            Some(project_operational_runtime(runtime)?)
        }
        (DeploymentStatusV1::NotRequested, _)
        | (_, None)
        | (DeploymentStatusV1::NotApplicable, Some(_)) => return Err(internal()),
    };
    let state = match observation.status() {
        DeploymentStatusV1::NotApplicable => DeploymentOperationalStateV2::NotApplicable,
        DeploymentStatusV1::Pending => DeploymentOperationalStateV2::Pending,
        DeploymentStatusV1::Failed { .. } => DeploymentOperationalStateV2::Failed,
        DeploymentStatusV1::Live { .. } => DeploymentOperationalStateV2::Live,
        DeploymentStatusV1::NotRequested => return Err(internal()),
    };
    Ok(DeploymentOperationalViewV2 {
        installation_id: decision.installation_id().as_str().to_string(),
        promotion_id: decision.promotion_id().as_str().to_string(),
        decision_observed_at: system_time_to_utc(observation.decision_observed_at())?,
        state,
        runtime,
    })
}

fn project_oauth_start_parts(
    state: &str,
    browser_nonce: &str,
    callback_url: &str,
    max_age_seconds: u32,
    application_id: DiscordApplicationIdV1,
) -> Result<OAuthStartResult, FacadeError> {
    let authorization_state = OAuthState::parse(state).map_err(|_| internal())?;
    let browser_nonce = OAuthState::parse(browser_nonce).map_err(|_| internal())?;
    if authorization_state == browser_nonce || max_age_seconds == 0 || max_age_seconds > 600 {
        return Err(internal());
    }
    Ok(OAuthStartResult {
        authorization_request: DiscordAuthorizationRequest {
            client_id: application_id.to_string(),
            callback_url: callback_url.to_string(),
        },
        authorization_state,
        browser_nonce,
        max_age_seconds,
    })
}

fn project_oauth_callback_parts(
    session: &str,
    csrf: &str,
    return_to: &str,
    max_age_seconds: u32,
) -> Result<OAuthCallbackResult, FacadeError> {
    let session = SessionCredential::parse(session).map_err(|_| internal())?;
    let csrf = CsrfSecret::parse(csrf).map_err(|_| internal())?;
    if session.expose_secret() == csrf.expose_secret()
        || max_age_seconds == 0
        || max_age_seconds > 43_200
    {
        return Err(internal());
    }
    Ok(OAuthCallbackResult {
        session,
        csrf,
        return_to: return_to.to_string(),
        max_age_seconds,
    })
}

fn decision_view(
    projection: &ProductDecisionProjectionV1,
    state: ProductState,
    replayed: bool,
) -> DecisionView {
    DecisionView {
        installation_id: projection.installation_id().as_str().to_string(),
        promotion_id: projection.promotion_id().as_str().to_string(),
        revision: projection.revision().get(),
        state,
        replayed,
    }
}

fn product_state(value: ProductStatusV1) -> ProductState {
    match value {
        ProductStatusV1::PendingApproval => ProductState::PendingApproval,
        ProductStatusV1::Approved => ProductState::Approved,
        ProductStatusV1::Applying => ProductState::Applying,
        ProductStatusV1::RuntimePending => ProductState::RuntimePending,
        ProductStatusV1::Live => ProductState::Live,
        ProductStatusV1::Rejected => ProductState::Rejected,
        ProductStatusV1::Expired => ProductState::Expired,
        ProductStatusV1::Superseded => ProductState::Superseded,
        ProductStatusV1::Withdrawn => ProductState::Withdrawn,
    }
}

fn product_state_from_phase(value: &ProductDecisionPhaseV1) -> ProductState {
    match value {
        ProductDecisionPhaseV1::PendingApproval => ProductState::PendingApproval,
        ProductDecisionPhaseV1::Approved => ProductState::Approved,
        ProductDecisionPhaseV1::Applying => ProductState::Applying,
        ProductDecisionPhaseV1::Applied { .. } => ProductState::RuntimePending,
        ProductDecisionPhaseV1::Rejected => ProductState::Rejected,
        ProductDecisionPhaseV1::Expired => ProductState::Expired,
        ProductDecisionPhaseV1::Superseded => ProductState::Superseded,
        ProductDecisionPhaseV1::Withdrawn => ProductState::Withdrawn,
    }
}

fn ensure_product_status_matches_decision(
    status: ProductStatusV1,
    phase: &ProductDecisionPhaseV1,
) -> Result<(), FacadeError> {
    let matches = matches!(
        (status, phase),
        (
            ProductStatusV1::PendingApproval,
            ProductDecisionPhaseV1::PendingApproval
        ) | (ProductStatusV1::Approved, ProductDecisionPhaseV1::Approved)
            | (ProductStatusV1::Applying, ProductDecisionPhaseV1::Applying)
            | (
                ProductStatusV1::RuntimePending,
                ProductDecisionPhaseV1::Applied { .. }
            )
            | (
                ProductStatusV1::Live,
                ProductDecisionPhaseV1::Applied { .. }
            )
            | (ProductStatusV1::Rejected, ProductDecisionPhaseV1::Rejected)
            | (ProductStatusV1::Expired, ProductDecisionPhaseV1::Expired)
            | (
                ProductStatusV1::Superseded,
                ProductDecisionPhaseV1::Superseded
            )
            | (
                ProductStatusV1::Withdrawn,
                ProductDecisionPhaseV1::Withdrawn
            )
    );
    if matches {
        Ok(())
    } else {
        Err(internal())
    }
}

fn project_deployment_parts(
    decision: &ProductDecisionProjectionV1,
    decision_observed_at: SystemTime,
    status: &DeploymentStatusV1,
    deployment: Option<&DeploymentStatusObservationV1>,
) -> Result<DeploymentView, FacadeError> {
    let observed_at = deployment
        .map(DeploymentStatusObservationV1::observed_at)
        .unwrap_or(decision_observed_at);
    let (
        state,
        retryable,
        failure_code,
        attestation_revision,
        last_serving_heartbeat,
        serving_lease_expires_at,
    ) = match (status, deployment) {
        (DeploymentStatusV1::NotApplicable, None) => (
            DeploymentState::NotApplicable,
            false,
            None,
            None,
            None,
            None,
        ),
        (DeploymentStatusV1::NotRequested, Some(observation))
            if matches!(
                observation.projection(),
                DeploymentStatusProjectionV1::NotRequested
            ) =>
        {
            (DeploymentState::NotRequested, false, None, None, None, None)
        }
        (DeploymentStatusV1::Pending, Some(observation))
            if matches!(
                observation.projection(),
                DeploymentStatusProjectionV1::Pending
            ) =>
        {
            (DeploymentState::Pending, false, None, None, None, None)
        }
        (
            DeploymentStatusV1::Failed {
                retryable,
                failure_code,
            },
            Some(observation),
        ) => {
            let failure = observation.failure().ok_or_else(internal)?;
            let safe_code = public_failure_code(failure_code)?;
            if failure.retryable() != *retryable || failure.failure_code().as_str() != safe_code {
                return Err(internal());
            }
            (
                DeploymentState::Failed,
                *retryable,
                Some(safe_code.to_string()),
                None,
                None,
                None,
            )
        }
        (
            DeploymentStatusV1::Live {
                attestation_revision,
            },
            Some(observation),
        ) => {
            let DeploymentStatusProjectionV1::ExactLive(live) = observation.projection() else {
                return Err(internal());
            };
            if live.attestation_revision() != *attestation_revision {
                return Err(internal());
            }
            let heartbeat = observation
                .last_heartbeat_at()
                .map(system_time_to_utc)
                .transpose()?
                .ok_or_else(internal)?;
            let lease = observation
                .lease_expires_at()
                .map(system_time_to_utc)
                .transpose()?
                .ok_or_else(internal)?;
            (
                DeploymentState::Live,
                false,
                None,
                Some(attestation_revision.get()),
                Some(heartbeat),
                Some(lease),
            )
        }
        _ => return Err(internal()),
    };
    Ok(DeploymentView {
        installation_id: decision.installation_id().as_str().to_string(),
        promotion_id: decision.promotion_id().as_str().to_string(),
        observed_at: system_time_to_utc(observed_at)?,
        state,
        retryable,
        failure_code,
        attestation_revision,
        last_serving_heartbeat,
        serving_lease_expires_at,
    })
}

fn project_operational_runtime(
    observation: &DeploymentOperationalObservationV2,
) -> Result<RuntimeDeploymentOperationalViewV2, FacadeError> {
    let failure = observation
        .base()
        .failure()
        .map(|failure| DeploymentFailureViewV2 {
            retryable: failure.retryable(),
            code: failure.failure_code().as_str().to_string(),
        });
    let retry = observation.retry().map(project_retry).transpose()?;
    let attestation = observation.attestation().map(project_attestation);
    Ok(RuntimeDeploymentOperationalViewV2 {
        observed_at: system_time_to_utc(observation.observed_at())?,
        phase: project_phase(observation.phase()),
        current_attempt: observation.current_attempt(),
        last_failure_attempt: observation.last_failure_attempt().map(NonZeroU32::get),
        failure,
        retry,
        operator_action: observation.operator_action().map(project_operator_action),
        attestation,
        serving: project_serving(observation.serving())?,
    })
}

fn ensure_operational_status_matches(
    status: &DeploymentStatusV1,
    runtime: &DeploymentOperationalObservationV2,
) -> Result<(), FacadeError> {
    let matches = match (status, runtime.base().projection()) {
        (DeploymentStatusV1::Pending, DeploymentStatusProjectionV1::Pending) => true,
        (
            DeploymentStatusV1::Failed {
                retryable,
                failure_code,
            },
            DeploymentStatusProjectionV1::Failed {
                retryable: runtime_retryable,
                failure_code: runtime_code,
            },
        ) => {
            *retryable == *runtime_retryable
                && public_failure_code(failure_code)? == public_failure_code(runtime_code)?
        }
        (
            DeploymentStatusV1::Live {
                attestation_revision,
            },
            DeploymentStatusProjectionV1::ExactLive(live),
        ) => *attestation_revision == live.attestation_revision(),
        (DeploymentStatusV1::NotApplicable, _)
        | (DeploymentStatusV1::NotRequested, _)
        | (DeploymentStatusV1::Pending, _)
        | (DeploymentStatusV1::Failed { .. }, _)
        | (DeploymentStatusV1::Live { .. }, _) => false,
    };
    if matches {
        Ok(())
    } else {
        Err(internal())
    }
}

fn project_phase(value: DeploymentConvergencePhaseV2) -> DeploymentRuntimePhaseV2 {
    match value {
        DeploymentConvergencePhaseV2::Requested => DeploymentRuntimePhaseV2::Requested,
        DeploymentConvergencePhaseV2::PreflightReady => DeploymentRuntimePhaseV2::PreflightReady,
        DeploymentConvergencePhaseV2::DrainRequested => DeploymentRuntimePhaseV2::DrainRequested,
        DeploymentConvergencePhaseV2::Drained => DeploymentRuntimePhaseV2::Drained,
        DeploymentConvergencePhaseV2::ActivationApplying => {
            DeploymentRuntimePhaseV2::ActivationApplying
        }
        DeploymentConvergencePhaseV2::RuntimeReady => DeploymentRuntimePhaseV2::RuntimeReady,
        DeploymentConvergencePhaseV2::RetryWaiting => DeploymentRuntimePhaseV2::RetryWaiting,
        DeploymentConvergencePhaseV2::RetryDue => DeploymentRuntimePhaseV2::RetryDue,
        DeploymentConvergencePhaseV2::OperatorBlocked => DeploymentRuntimePhaseV2::OperatorBlocked,
        DeploymentConvergencePhaseV2::AuthorityBlocked => {
            DeploymentRuntimePhaseV2::AuthorityBlocked
        }
        DeploymentConvergencePhaseV2::ReconcilingPanels => {
            DeploymentRuntimePhaseV2::ReconcilingPanels
        }
        DeploymentConvergencePhaseV2::AwaitingGatewayReady => {
            DeploymentRuntimePhaseV2::AwaitingGatewayReady
        }
        DeploymentConvergencePhaseV2::Live => DeploymentRuntimePhaseV2::Live,
        DeploymentConvergencePhaseV2::Superseded => DeploymentRuntimePhaseV2::Superseded,
        DeploymentConvergencePhaseV2::Cancelled => DeploymentRuntimePhaseV2::Cancelled,
    }
}

fn project_operator_action(value: ApplicationOperatorActionV2) -> DeploymentOperatorActionV2 {
    match value {
        ApplicationOperatorActionV2::RecoverBlockedDeployment => {
            DeploymentOperatorActionV2::RecoverBlockedDeployment
        }
        ApplicationOperatorActionV2::RestoreProductAuthority => {
            DeploymentOperatorActionV2::RestoreProductAuthority
        }
    }
}

fn project_retry(
    value: DeploymentRetryObservationV2,
) -> Result<DeploymentRetryViewV2, FacadeError> {
    let state = match value {
        DeploymentRetryObservationV2::Waiting { .. } => DeploymentRetryStateV2::Waiting,
        DeploymentRetryObservationV2::Due { .. } => DeploymentRetryStateV2::Due,
    };
    Ok(DeploymentRetryViewV2 {
        state,
        failure_attempt: value.failure_attempt().get(),
        retry_not_before: system_time_to_utc(value.retry_not_before())?,
    })
}

fn project_attestation(
    value: authoring_application::DeploymentAttestationObservationV2,
) -> DeploymentAttestationViewV2 {
    DeploymentAttestationViewV2 {
        deployment_revision: value.deployment_revision().get(),
        convergence_attempt: value.convergence_attempt().get(),
    }
}

fn project_serving(
    value: DeploymentServingFreshnessV2,
) -> Result<DeploymentServingFreshnessViewV2, FacadeError> {
    let state = match value {
        DeploymentServingFreshnessV2::NotExpected => DeploymentServingFreshnessStateV2::NotExpected,
        DeploymentServingFreshnessV2::AttestationMissing => {
            DeploymentServingFreshnessStateV2::AttestationMissing
        }
        DeploymentServingFreshnessV2::LeaseMissing => {
            DeploymentServingFreshnessStateV2::LeaseMissing
        }
        DeploymentServingFreshnessV2::IdentityMismatch => {
            DeploymentServingFreshnessStateV2::IdentityMismatch
        }
        DeploymentServingFreshnessV2::Disconnected { .. } => {
            DeploymentServingFreshnessStateV2::Disconnected
        }
        DeploymentServingFreshnessV2::Expired { .. } => DeploymentServingFreshnessStateV2::Expired,
        DeploymentServingFreshnessV2::Fresh { .. } => DeploymentServingFreshnessStateV2::Fresh,
    };
    Ok(DeploymentServingFreshnessViewV2 {
        state,
        last_heartbeat_at: value
            .last_heartbeat_at()
            .map(system_time_to_utc)
            .transpose()?,
        lease_expires_at: value
            .lease_expires_at()
            .map(system_time_to_utc)
            .transpose()?,
    })
}

fn public_failure_code(value: &str) -> Result<&'static str, FacadeError> {
    authoring_application::DeploymentFailureCodeV1::parse(value)
        .map(authoring_application::DeploymentFailureCodeV1::as_str)
        .map_err(|_| internal())
}

fn system_time_to_utc(value: SystemTime) -> Result<DateTime<Utc>, FacadeError> {
    let (seconds, nanoseconds) = match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => (i128::from(duration.as_secs()), duration.subsec_nanos()),
        Err(error) => {
            let duration = error.duration();
            if duration.subsec_nanos() == 0 {
                (-i128::from(duration.as_secs()), 0)
            } else {
                (
                    -i128::from(duration.as_secs()) - 1,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    };
    i64::try_from(seconds)
        .ok()
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, nanoseconds))
        .ok_or_else(internal)
}

fn internal() -> FacadeError {
    FacadeError::new(FacadeErrorCode::Internal)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::time::Duration;

    use authoring_application::{
        DeploymentOperationalProjectionV2, ExactDeploymentSelectorV1, ExactLiveProjectionV1,
        ProductDecisionProjectionV1, ProductDrainSelectorV1,
        ProductLifecycleCancellationDeploymentProjectionV1,
        ProductLifecycleCancellationDrainProjectionV1,
        ProductLifecycleCancellationSlotProjectionV1, ProductRevisionV1,
        SafeAuthoringTurnProjectionV1,
    };
    use authoring_promotion::{
        AuthoringSessionId, AutomationInstallationId, PromotionId, SessionGeneration, TenantId,
    };
    use discord_model::GuildId;

    use super::*;

    fn digest(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn identifier(character: char) -> String {
        character.to_string().repeat(32)
    }

    fn exact_deployment() -> authoring_application::ExactDeploymentSelectorV1 {
        ExactDeploymentSelectorV1::from_server_projection(
            AutomationInstallationId::parse("installation-1").unwrap(),
            PromotionId::parse(&digest('a')).unwrap(),
            "deployment-1",
            digest('b'),
        )
        .unwrap()
    }

    fn safe_authoring_projection(state: &str) -> SafeAuthoringTurnProjectionV1 {
        let bytes = format!(
            "{{\"schema_version\":1,\"state\":\"{state}\",\"assistant_message\":\"Safe response\",\"capabilities\":[],\"draft\":{{\"panels\":0,\"modals\":0,\"rules\":0,\"actions\":0,\"unresolved_references\":[]}},\"preview\":null}}"
        );
        SafeAuthoringTurnProjectionV1::from_canonical_json(bytes.as_bytes()).unwrap()
    }

    fn decision(phase: ProductDecisionPhaseV1) -> ProductDecisionProjectionV1 {
        ProductDecisionProjectionV1::from_server_projection(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-1").unwrap(),
            GuildId(1),
            PromotionId::parse(&digest('a')).unwrap(),
            ProductRevisionV1::new(4).unwrap(),
            phase,
        )
    }

    #[test]
    fn oauth_projection_rejects_equal_or_noncanonical_secrets() {
        let first = "A".repeat(43);
        let second = format!("{}E", "A".repeat(42));
        let application = DiscordApplicationIdV1::new(1).unwrap();
        assert_eq!(
            project_oauth_start_parts(&first, &first, "callback", 60, application)
                .unwrap_err()
                .error_code(),
            FacadeErrorCode::Internal
        );
        let projected =
            project_oauth_start_parts(&first, &second, "callback", 60, application).unwrap();
        assert_eq!(projected.authorization_request.client_id, "1");
        assert!(!format!("{projected:?}").contains(&first));

        assert_eq!(
            project_oauth_callback_parts(&first, &first, "/", 60)
                .unwrap_err()
                .error_code(),
            FacadeErrorCode::Internal
        );
    }

    #[test]
    fn authoring_projection_exposes_only_the_canonical_safe_projection() {
        let session_id = AuthoringSessionId::parse("session-1").unwrap();
        let outcome = AuthoringTurnOutcomeV1::NotCommitted(safe_authoring_projection("rejected"));
        let view = project_authoring_turn(&session_id, &outcome).unwrap();
        assert_eq!(view.session_id, "session-1");
        assert_eq!(view.generation, None);
        assert_eq!(view.disposition, None);
        assert_eq!(
            view.projection.state(),
            authoring_application::SafeAuthoringTurnStateV1::Rejected
        );

        let observation = AuthoringSessionObservationV1::from_storage(
            session_id,
            SessionGeneration::new(7).unwrap(),
            safe_authoring_projection("discussion"),
            None,
        )
        .unwrap();
        let view = project_authoring_session(&observation).unwrap();
        assert_eq!(view.session_id, "session-1");
        assert_eq!(view.observed_generation, 7);
        assert_eq!(
            view.projection.state(),
            authoring_application::SafeAuthoringTurnStateV1::Discussion
        );
    }

    #[test]
    fn authoring_projection_rejects_non_durable_read_and_commit_shapes() {
        let session_id = AuthoringSessionId::parse("session-1").unwrap();
        let invalid_turn =
            AuthoringTurnOutcomeV1::NotCommitted(safe_authoring_projection("discussion"));
        assert_eq!(
            project_authoring_turn(&session_id, &invalid_turn)
                .unwrap_err()
                .error_code(),
            FacadeErrorCode::Internal
        );

        let invalid_read = AuthoringSessionObservationV1::from_storage(
            session_id,
            SessionGeneration::new(1).unwrap(),
            safe_authoring_projection("unsupported"),
            None,
        )
        .unwrap();
        assert_eq!(
            project_authoring_session(&invalid_read)
                .unwrap_err()
                .error_code(),
            FacadeErrorCode::Internal
        );
    }

    #[test]
    fn mutation_projection_preserves_revision_state_and_replay() {
        let projection = decision(ProductDecisionPhaseV1::Approved);
        let receipt = ProductMutationReceiptV1::from_server_projection(projection, true);
        let view = project_decision_mutation(&receipt);
        assert_eq!(view.revision, 4);
        assert_eq!(view.state, ProductState::Approved);
        assert!(view.replayed);
    }

    #[test]
    fn lifecycle_cancellation_projection_preserves_all_concurrency_boundaries() {
        let source = ProductDrainSelectorV1::from_server_projection(
            identifier('b'),
            11,
            digest('c'),
            identifier('d'),
            17,
        )
        .unwrap();
        let receipt = ProductLifecycleCancellationReceiptV1::from_server_projection(
            decision(ProductDecisionPhaseV1::Approved),
            ProductLifecycleCancellationDeploymentProjectionV1::from_server_projection(18).unwrap(),
            ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
                source,
                12,
                digest('e'),
            )
            .unwrap(),
            ProductLifecycleCancellationSlotProjectionV1::from_server_projection(23, 24).unwrap(),
            UNIX_EPOCH + Duration::from_secs(100),
            true,
        )
        .unwrap();
        let view = project_lifecycle_cancellation(&receipt).unwrap();
        assert_eq!(view.revision, 4);
        assert_eq!(view.state, ProductState::Approved);
        assert_eq!(view.drain_intent_id, identifier('b'));
        assert_eq!(view.source_intent_revision, 11);
        assert_eq!(view.terminal_intent_revision, 12);
        assert_eq!(view.terminal_state_digest, digest('e'));
        assert_eq!(view.product_operation_id, identifier('d'));
        assert_eq!(view.source_runtime_deployment_revision, 17);
        assert_eq!(view.resulting_runtime_deployment_revision, 18);
        assert_eq!(view.source_slot_writer_epoch, 23);
        assert_eq!(view.successor_slot_writer_epoch, 24);
        assert_eq!(view.cancelled_at.timestamp(), 100);
        assert!(view.replayed);

        let invalid = ProductLifecycleCancellationReceiptV1::from_server_projection(
            decision(ProductDecisionPhaseV1::Applying),
            ProductLifecycleCancellationDeploymentProjectionV1::from_server_projection(18).unwrap(),
            ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
                ProductDrainSelectorV1::from_server_projection(
                    identifier('b'),
                    11,
                    digest('c'),
                    identifier('d'),
                    17,
                )
                .unwrap(),
                12,
                digest('e'),
            )
            .unwrap(),
            ProductLifecycleCancellationSlotProjectionV1::from_server_projection(23, 24).unwrap(),
            UNIX_EPOCH + Duration::from_secs(100),
            false,
        )
        .unwrap();
        assert_eq!(
            project_lifecycle_cancellation(&invalid)
                .unwrap_err()
                .error_code(),
            FacadeErrorCode::Internal
        );
    }

    #[test]
    fn v1_live_projection_preserves_the_validated_serving_window() {
        let exact = exact_deployment();
        let attestation_revision = NonZeroU64::new(7).unwrap();
        let observed_at = UNIX_EPOCH + Duration::from_secs(100);
        let heartbeat = UNIX_EPOCH + Duration::from_secs(90);
        let lease = UNIX_EPOCH + Duration::from_secs(110);
        let runtime = DeploymentStatusObservationV1::from_server_projection(
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                exact.clone(),
                attestation_revision,
            )),
            observed_at,
            Some(heartbeat),
            Some(lease),
        )
        .unwrap();
        let decision = decision(ProductDecisionPhaseV1::Applied {
            exact_deployment: exact,
        });
        let view = project_deployment_parts(
            &decision,
            UNIX_EPOCH + Duration::from_secs(80),
            &DeploymentStatusV1::Live {
                attestation_revision,
            },
            Some(&runtime),
        )
        .unwrap();
        assert_eq!(view.state, DeploymentState::Live);
        assert_eq!(view.last_serving_heartbeat.unwrap().timestamp(), 90);
        assert_eq!(view.serving_lease_expires_at.unwrap().timestamp(), 110);
    }

    #[test]
    fn v1_failure_projection_rejects_unknown_or_mismatched_metadata() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(100);
        let runtime = DeploymentStatusObservationV1::from_server_projection(
            DeploymentStatusProjectionV1::Failed {
                retryable: true,
                failure_code: "gateway_ready_timeout".to_string(),
            },
            observed_at,
            None,
            None,
        )
        .unwrap();
        let decision = decision(ProductDecisionPhaseV1::Applied {
            exact_deployment: exact_deployment(),
        });
        let status = DeploymentStatusV1::Failed {
            retryable: true,
            failure_code: "gateway_ready_timeout".to_string(),
        };
        let view =
            project_deployment_parts(&decision, observed_at, &status, Some(&runtime)).unwrap();
        assert_eq!(view.failure_code.as_deref(), Some("gateway_ready_timeout"));

        let mismatch = DeploymentStatusV1::Failed {
            retryable: false,
            failure_code: "gateway_ready_timeout".to_string(),
        };
        assert_eq!(
            project_deployment_parts(&decision, observed_at, &mismatch, Some(&runtime))
                .unwrap_err()
                .error_code(),
            FacadeErrorCode::Internal
        );
    }

    #[test]
    fn operational_runtime_projection_keeps_closed_v2_semantics() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(100);
        let base = DeploymentStatusObservationV1::from_server_projection(
            DeploymentStatusProjectionV1::Pending,
            observed_at,
            None,
            None,
        )
        .unwrap();
        let runtime = DeploymentOperationalObservationV2::from_server_projection(
            base,
            DeploymentOperationalProjectionV2 {
                phase: DeploymentConvergencePhaseV2::Requested,
                current_attempt: 0,
                last_failure_attempt: None,
                retry: None,
                operator_action: None,
                attestation: None,
                serving: DeploymentServingFreshnessV2::NotExpected,
            },
        )
        .unwrap();
        let projected = project_operational_runtime(&runtime).unwrap();
        assert_eq!(projected.phase, DeploymentRuntimePhaseV2::Requested);
        assert_eq!(
            projected.serving.state,
            DeploymentServingFreshnessStateV2::NotExpected
        );
        assert_eq!(projected.observed_at.timestamp(), 100);
    }

    #[test]
    fn system_time_conversion_supports_pre_epoch_and_rejects_chrono_overflow() {
        let before = UNIX_EPOCH - Duration::from_millis(1_500);
        let converted = system_time_to_utc(before).unwrap();
        assert_eq!(converted.timestamp(), -2);
        assert_eq!(converted.timestamp_subsec_millis(), 500);

        let overflow = UNIX_EPOCH
            .checked_add(Duration::from_secs(10_000_000_000_000))
            .unwrap();
        assert_eq!(
            system_time_to_utc(overflow).unwrap_err().error_code(),
            FacadeErrorCode::Internal
        );
    }
}
