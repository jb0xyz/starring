use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    CancelProductLifecycleMutationV1, InstallationSelectorV1, ProductDrainSelectorV1,
    ProductIdempotencyKeyV1, ProductLifecycleCancellationReasonV1,
    ProductPromotionIdempotencyKeyV1, ProductRequestIdV1, ProductRevisionV1, ProductStatusQueryV1,
    PromoteOwnedSessionV1, PromotionSelectorV1, ReadAuthoringSessionV1, RejectProductPromotionV1,
    RejectionReasonV1, RuntimeDeploymentQueryV1, StartOrAdvanceAuthoringTurnV1,
};
use authoring_application_discord::{DiscordAuthorizationCodeV1, DiscordOAuthStateV1};
use authoring_promotion::{
    AuthoringSessionId, AutomationInstallationId, PromotionId, SessionGeneration,
};
use product_control_http::{
    ApplyCommand, AuthoringTurnCommandV1, DecisionCommand, FacadeError, FacadeErrorCode,
    LifecycleCancellationCommand, OAuthCode, OAuthState, PromoteCommand, RejectCommand,
};

pub struct MappedAuthoringTurnCommandV1 {
    request_id: ProductRequestIdV1,
    installation: InstallationSelectorV1,
    command: StartOrAdvanceAuthoringTurnV1,
}

impl MappedAuthoringTurnCommandV1 {
    pub fn request_id(&self) -> &ProductRequestIdV1 {
        &self.request_id
    }

    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn command(&self) -> &StartOrAdvanceAuthoringTurnV1 {
        &self.command
    }

    pub fn into_parts(
        self,
    ) -> (
        ProductRequestIdV1,
        InstallationSelectorV1,
        StartOrAdvanceAuthoringTurnV1,
    ) {
        (self.request_id, self.installation, self.command)
    }
}

pub struct MappedAuthoringSessionQueryV1 {
    installation: InstallationSelectorV1,
    query: ReadAuthoringSessionV1,
}

impl MappedAuthoringSessionQueryV1 {
    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn query(&self) -> &ReadAuthoringSessionV1 {
        &self.query
    }

    pub fn into_parts(self) -> (InstallationSelectorV1, ReadAuthoringSessionV1) {
        (self.installation, self.query)
    }
}

pub struct MappedProductTarget {
    installation: InstallationSelectorV1,
    promotion: PromotionSelectorV1,
}

impl MappedProductTarget {
    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn promotion(&self) -> &PromotionSelectorV1 {
        &self.promotion
    }

    pub fn status_query(&self) -> ProductStatusQueryV1 {
        ProductStatusQueryV1 {
            promotion: self.promotion.clone(),
        }
    }

    pub fn runtime_query(&self) -> RuntimeDeploymentQueryV1 {
        RuntimeDeploymentQueryV1 {
            promotion: self.promotion.clone(),
        }
    }

    pub fn into_parts(self) -> (InstallationSelectorV1, PromotionSelectorV1) {
        (self.installation, self.promotion)
    }
}

pub struct MappedPromoteCommand {
    request_id: ProductRequestIdV1,
    installation: InstallationSelectorV1,
    command: PromoteOwnedSessionV1,
}

impl MappedPromoteCommand {
    pub fn request_id(&self) -> &ProductRequestIdV1 {
        &self.request_id
    }

    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn command(&self) -> &PromoteOwnedSessionV1 {
        &self.command
    }

    pub fn into_parts(
        self,
    ) -> (
        ProductRequestIdV1,
        InstallationSelectorV1,
        PromoteOwnedSessionV1,
    ) {
        (self.request_id, self.installation, self.command)
    }
}

pub struct MappedApproveCommand {
    request_id: ProductRequestIdV1,
    installation: InstallationSelectorV1,
    command: ApproveProductPromotionV1,
}

impl MappedApproveCommand {
    pub fn request_id(&self) -> &ProductRequestIdV1 {
        &self.request_id
    }

    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn command(&self) -> &ApproveProductPromotionV1 {
        &self.command
    }

    pub fn into_parts(
        self,
    ) -> (
        ProductRequestIdV1,
        InstallationSelectorV1,
        ApproveProductPromotionV1,
    ) {
        (self.request_id, self.installation, self.command)
    }
}

pub struct MappedRejectCommand {
    request_id: ProductRequestIdV1,
    installation: InstallationSelectorV1,
    command: RejectProductPromotionV1,
}

impl MappedRejectCommand {
    pub fn request_id(&self) -> &ProductRequestIdV1 {
        &self.request_id
    }

    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn command(&self) -> &RejectProductPromotionV1 {
        &self.command
    }

    pub fn into_parts(
        self,
    ) -> (
        ProductRequestIdV1,
        InstallationSelectorV1,
        RejectProductPromotionV1,
    ) {
        (self.request_id, self.installation, self.command)
    }
}

pub struct MappedApplyCommand {
    request_id: ProductRequestIdV1,
    installation: InstallationSelectorV1,
    command: ApplyProductPromotionV1,
}

pub struct MappedLifecycleCancellationCommand {
    request_id: ProductRequestIdV1,
    installation: InstallationSelectorV1,
    command: CancelProductLifecycleMutationV1,
}

impl MappedLifecycleCancellationCommand {
    pub fn request_id(&self) -> &ProductRequestIdV1 {
        &self.request_id
    }

    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn command(&self) -> &CancelProductLifecycleMutationV1 {
        &self.command
    }

    pub fn into_parts(
        self,
    ) -> (
        ProductRequestIdV1,
        InstallationSelectorV1,
        CancelProductLifecycleMutationV1,
    ) {
        (self.request_id, self.installation, self.command)
    }
}

impl MappedApplyCommand {
    pub fn request_id(&self) -> &ProductRequestIdV1 {
        &self.request_id
    }

    pub fn installation(&self) -> &InstallationSelectorV1 {
        &self.installation
    }

    pub fn command(&self) -> &ApplyProductPromotionV1 {
        &self.command
    }

    pub fn into_parts(
        self,
    ) -> (
        ProductRequestIdV1,
        InstallationSelectorV1,
        ApplyProductPromotionV1,
    ) {
        (self.request_id, self.installation, self.command)
    }
}

pub fn map_product_target(
    installation_id: &str,
    promotion_id: &str,
) -> Result<MappedProductTarget, FacadeError> {
    Ok(MappedProductTarget {
        installation: parse_installation(installation_id)?,
        promotion: parse_promotion(promotion_id)?,
    })
}

pub fn map_authoring_turn_command(
    command: AuthoringTurnCommandV1,
) -> Result<MappedAuthoringTurnCommandV1, FacadeError> {
    let request_id = parse_request_id(&command.request_id)?;
    let installation = parse_installation(&command.installation_id)?;
    let session_id = AuthoringSessionId::parse(&command.session_id).map_err(internal)?;
    let idempotency_key = ProductIdempotencyKeyV1::parse(command.idempotency_key.expose_secret())
        .map_err(internal)?;
    Ok(MappedAuthoringTurnCommandV1 {
        request_id,
        installation,
        command: StartOrAdvanceAuthoringTurnV1::new_with_commit_boundary(
            session_id,
            command.expected_generation,
            idempotency_key,
            command.message,
            command.commit_boundary,
        ),
    })
}

pub fn map_authoring_session_query(
    installation_id: &str,
    session_id: &str,
) -> Result<MappedAuthoringSessionQueryV1, FacadeError> {
    Ok(MappedAuthoringSessionQueryV1 {
        installation: parse_installation(installation_id)?,
        query: ReadAuthoringSessionV1::new(
            AuthoringSessionId::parse(session_id).map_err(internal)?,
        ),
    })
}

pub fn map_promote_command(command: PromoteCommand) -> Result<MappedPromoteCommand, FacadeError> {
    let request_id = parse_request_id(&command.request_id)?;
    let installation = parse_installation(&command.installation_id)?;
    let session_id = AuthoringSessionId::parse(&command.session_id).map_err(internal)?;
    let expected_generation =
        SessionGeneration::new(command.expected_generation).map_err(internal)?;
    let idempotency_key =
        ProductPromotionIdempotencyKeyV1::parse(command.idempotency_key.expose_secret())
            .map_err(internal)?;
    Ok(MappedPromoteCommand {
        request_id,
        installation,
        command: PromoteOwnedSessionV1 {
            idempotency_key,
            session_id,
            expected_generation,
        },
    })
}

pub fn map_approve_command(command: DecisionCommand) -> Result<MappedApproveCommand, FacadeError> {
    let (
        request_id,
        installation,
        promotion,
        expected_payload_digest,
        expected_revision,
        idempotency_key,
    ) = map_decision(command)?;
    Ok(MappedApproveCommand {
        request_id,
        installation,
        command: ApproveProductPromotionV1 {
            promotion,
            expected_payload_digest,
            expected_revision,
            idempotency_key,
        },
    })
}

pub fn map_reject_command(command: RejectCommand) -> Result<MappedRejectCommand, FacadeError> {
    let reason = RejectionReasonV1::parse(&command.reason).map_err(internal)?;
    let (
        request_id,
        installation,
        promotion,
        expected_payload_digest,
        expected_revision,
        idempotency_key,
    ) = map_decision(command.decision)?;
    Ok(MappedRejectCommand {
        request_id,
        installation,
        command: RejectProductPromotionV1 {
            promotion,
            expected_payload_digest,
            expected_revision,
            idempotency_key,
            reason,
        },
    })
}

pub fn map_apply_command(command: ApplyCommand) -> Result<MappedApplyCommand, FacadeError> {
    let (
        request_id,
        installation,
        promotion,
        expected_payload_digest,
        expected_revision,
        idempotency_key,
    ) = map_decision(command.decision)?;
    Ok(MappedApplyCommand {
        request_id,
        installation,
        command: ApplyProductPromotionV1 {
            promotion,
            expected_payload_digest,
            expected_revision,
            idempotency_key,
        },
    })
}

pub fn map_lifecycle_cancellation_command(
    command: LifecycleCancellationCommand,
) -> Result<MappedLifecycleCancellationCommand, FacadeError> {
    let reason = ProductLifecycleCancellationReasonV1::parse(&command.reason).map_err(internal)?;
    let drain_selector = ProductDrainSelectorV1::from_server_projection(
        command.drain_intent_id,
        command.acknowledged_intent_revision,
        command.acknowledged_state_digest,
        command.product_operation_id,
        command.expected_runtime_deployment_revision,
    )
    .map_err(internal)?;
    let (
        request_id,
        installation,
        promotion,
        expected_payload_digest,
        expected_revision,
        idempotency_key,
    ) = map_decision(command.decision)?;
    Ok(MappedLifecycleCancellationCommand {
        request_id,
        installation,
        command: CancelProductLifecycleMutationV1 {
            promotion,
            expected_payload_digest,
            expected_revision,
            drain_selector,
            idempotency_key,
            reason,
        },
    })
}

pub fn map_discord_oauth_state(value: &OAuthState) -> Result<DiscordOAuthStateV1, FacadeError> {
    DiscordOAuthStateV1::from_owned(value.expose_secret().to_string()).map_err(internal)
}

pub fn map_discord_authorization_code(
    value: &OAuthCode,
) -> Result<DiscordAuthorizationCodeV1, FacadeError> {
    DiscordAuthorizationCodeV1::from_owned(value.expose_secret().to_string()).map_err(internal)
}

type MappedDecisionParts = (
    ProductRequestIdV1,
    InstallationSelectorV1,
    PromotionSelectorV1,
    ApprovalPayloadDigestV1,
    ProductRevisionV1,
    ProductIdempotencyKeyV1,
);

fn map_decision(command: DecisionCommand) -> Result<MappedDecisionParts, FacadeError> {
    Ok((
        parse_request_id(&command.request_id)?,
        parse_installation(&command.installation_id)?,
        parse_promotion(&command.promotion_id)?,
        ApprovalPayloadDigestV1::parse(&command.expected_payload_digest).map_err(internal)?,
        ProductRevisionV1::new(command.expected_revision).map_err(internal)?,
        ProductIdempotencyKeyV1::parse(command.idempotency_key.expose_secret())
            .map_err(internal)?,
    ))
}

fn parse_request_id(
    value: &product_control_http::ProductRequestId,
) -> Result<ProductRequestIdV1, FacadeError> {
    ProductRequestIdV1::parse(value.as_str()).map_err(internal)
}

pub(crate) fn parse_installation(value: &str) -> Result<InstallationSelectorV1, FacadeError> {
    AutomationInstallationId::parse(value)
        .map(InstallationSelectorV1::new)
        .map_err(internal)
}

fn parse_promotion(value: &str) -> Result<PromotionSelectorV1, FacadeError> {
    PromotionId::parse(value)
        .map(PromotionSelectorV1::new)
        .map_err(internal)
}

fn internal<T>(_error: T) -> FacadeError {
    FacadeError::new(FacadeErrorCode::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use authoring_application::{AuthoringExpectedGenerationV1, AuthoringHumanMessageV1};
    use product_control_http::{IdempotencyKey, ProductRequestId};

    fn digest(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn identifier(character: char) -> String {
        character.to_string().repeat(32)
    }

    fn request_id() -> ProductRequestId {
        ProductRequestId::parse("request-1").unwrap()
    }

    fn idempotency() -> product_control_http::IdempotencyKey {
        IdempotencyKey::parse("idempotency-1").unwrap()
    }

    fn decision() -> DecisionCommand {
        DecisionCommand {
            request_id: request_id(),
            installation_id: "installation-1".to_string(),
            promotion_id: digest('a'),
            expected_payload_digest: digest('b'),
            expected_revision: 7,
            idempotency_key: idempotency(),
        }
    }

    #[test]
    fn promote_mapping_builds_only_validated_domain_types() {
        let mapped = map_promote_command(PromoteCommand {
            request_id: request_id(),
            installation_id: "installation-1".to_string(),
            session_id: "session-1".to_string(),
            expected_generation: 3,
            idempotency_key: idempotency(),
        })
        .unwrap();
        assert_eq!(mapped.request_id().as_str(), "request-1");
        assert_eq!(
            mapped.installation().installation_id().as_str(),
            "installation-1"
        );
        assert_eq!(mapped.command().session_id.as_str(), "session-1");
        assert_eq!(mapped.command().expected_generation.get(), 3);
    }

    #[test]
    fn authoring_mapping_preserves_only_the_validated_public_command_fields() {
        let commit_boundary = authoring_application::AuthoringCommitBoundaryV1::new();
        let mapped = map_authoring_turn_command(AuthoringTurnCommandV1 {
            request_id: request_id(),
            installation_id: "installation-1".to_string(),
            session_id: "session-1".to_string(),
            expected_generation: AuthoringExpectedGenerationV1::new(3).unwrap(),
            idempotency_key: idempotency(),
            message: AuthoringHumanMessageV1::parse("스터디룸을 만들어줘").unwrap(),
            commit_boundary: commit_boundary.clone(),
        })
        .unwrap();
        assert_eq!(mapped.request_id().as_str(), "request-1");
        assert_eq!(
            mapped.installation().installation_id().as_str(),
            "installation-1"
        );
        assert_eq!(mapped.command().session_id().as_str(), "session-1");
        assert_eq!(mapped.command().expected_generation().get(), 3);
        assert_eq!(mapped.command().idempotency_key().as_str(), "idempotency-1");
        assert_eq!(
            mapped.command().human_message().as_str(),
            "스터디룸을 만들어줘"
        );
        assert!(mapped.command().commit_boundary().enter_commit_phase());
        assert!(commit_boundary.commit_phase_started());

        let query = map_authoring_session_query("installation-1", "session-1").unwrap();
        assert_eq!(
            query.installation().installation_id().as_str(),
            "installation-1"
        );
        assert_eq!(query.query().session_id().as_str(), "session-1");
    }

    #[test]
    fn authoring_mapping_rejects_any_domain_identity_mismatch_as_internal() {
        let invalid = map_authoring_turn_command(AuthoringTurnCommandV1 {
            request_id: request_id(),
            installation_id: "invalid/path".to_string(),
            session_id: "session-1".to_string(),
            expected_generation: AuthoringExpectedGenerationV1::new(0).unwrap(),
            idempotency_key: idempotency(),
            message: AuthoringHumanMessageV1::parse("계속해").unwrap(),
            commit_boundary: authoring_application::AuthoringCommitBoundaryV1::new(),
        })
        .err()
        .unwrap();
        assert_eq!(invalid.error_code(), FacadeErrorCode::Internal);
        assert_eq!(
            map_authoring_session_query("installation-1", "invalid/path")
                .err()
                .unwrap()
                .error_code(),
            FacadeErrorCode::Internal
        );
    }

    #[test]
    fn product_target_and_decisions_preserve_exact_scope_and_concurrency_guards() {
        let target = map_product_target("installation-1", &digest('a')).unwrap();
        assert_eq!(
            target.installation().installation_id().as_str(),
            "installation-1"
        );
        assert_eq!(target.promotion().promotion_id().as_str(), digest('a'));
        assert_eq!(
            target.status_query().promotion.promotion_id().as_str(),
            digest('a')
        );
        assert_eq!(
            target.runtime_query().promotion.promotion_id().as_str(),
            digest('a')
        );

        let approve = map_approve_command(decision()).unwrap();
        assert_eq!(approve.command().expected_revision.get(), 7);
        assert_eq!(
            approve.command().expected_payload_digest.as_str(),
            digest('b')
        );

        let reject = map_reject_command(RejectCommand {
            decision: decision(),
            reason: "  durable reason  ".to_string(),
        })
        .unwrap();
        assert_eq!(reject.command().reason.as_str(), "durable reason");

        let apply = map_apply_command(ApplyCommand {
            decision: decision(),
        })
        .unwrap();
        assert_eq!(apply.command().expected_revision.get(), 7);

        let cancellation = map_lifecycle_cancellation_command(LifecycleCancellationCommand {
            decision: decision(),
            drain_intent_id: identifier('c'),
            acknowledged_intent_revision: 11,
            acknowledged_state_digest: digest('d'),
            product_operation_id: identifier('e'),
            expected_runtime_deployment_revision: 17,
            reason: "  retain current automation  ".to_string(),
        })
        .unwrap();
        assert_eq!(
            cancellation.command().drain_selector.drain_intent_id(),
            identifier('c')
        );
        assert_eq!(
            cancellation
                .command()
                .drain_selector
                .acknowledged_intent_revision()
                .get(),
            11
        );
        assert_eq!(
            cancellation
                .command()
                .drain_selector
                .expected_runtime_deployment_revision()
                .get(),
            17
        );
        assert_eq!(
            cancellation.command().reason.as_str(),
            "retain current automation"
        );
    }

    #[test]
    fn domain_mismatch_is_never_reclassified_as_client_input() {
        let mut invalid = decision();
        invalid.expected_revision = 0;
        assert_eq!(
            map_approve_command(invalid).err().unwrap().error_code(),
            FacadeErrorCode::Internal
        );
        assert_eq!(
            map_product_target("invalid/path", &digest('a'))
                .err()
                .unwrap()
                .error_code(),
            FacadeErrorCode::Internal
        );
        let mut cancellation = LifecycleCancellationCommand {
            decision: decision(),
            drain_intent_id: identifier('c'),
            acknowledged_intent_revision: 0,
            acknowledged_state_digest: digest('d'),
            product_operation_id: identifier('e'),
            expected_runtime_deployment_revision: 17,
            reason: "retain current automation".to_string(),
        };
        assert_eq!(
            map_lifecycle_cancellation_command(cancellation)
                .err()
                .unwrap()
                .error_code(),
            FacadeErrorCode::Internal
        );
        cancellation = LifecycleCancellationCommand {
            decision: decision(),
            drain_intent_id: identifier('c'),
            acknowledged_intent_revision: 11,
            acknowledged_state_digest: digest('d'),
            product_operation_id: identifier('e'),
            expected_runtime_deployment_revision: 17,
            reason: "\n".to_string(),
        };
        assert_eq!(
            map_lifecycle_cancellation_command(cancellation)
                .err()
                .unwrap()
                .error_code(),
            FacadeErrorCode::Internal
        );
    }

    #[test]
    fn oauth_secrets_cross_the_boundary_without_debug_exposure() {
        let encoded = "A".repeat(43);
        let state = OAuthState::parse(&encoded).unwrap();
        let mapped_state = map_discord_oauth_state(&state).unwrap();
        assert_eq!(mapped_state.expose_secret(), encoded);
        assert!(!format!("{mapped_state:?}").contains(&encoded));

        let code = OAuthCode::parse("authorization-code").unwrap();
        let mapped_code = map_discord_authorization_code(&code).unwrap();
        assert!(!format!("{mapped_code:?}").contains("authorization-code"));
    }
}
