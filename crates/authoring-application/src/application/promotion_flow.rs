use authoring_promotion::plan_start_promotion_ref_v1;

use super::AuthoringApplication;
use crate::authority::validate_authorized_scope;
use crate::promotion::{
    build_start_promotion, product_promotion_observation, validate_authorized_replay,
    validate_authorized_submission, ExpectedPromotionSubmissionV1,
};
use crate::{
    AuthenticatedActorV1, AuthoringApplicationError, AuthorizedPromotionAccessV1,
    AuthorizedPromotionSnapshotPort, AuthorizedPromotionSubmissionPort,
    AuthorizedPromotionSubmissionV1, CapabilityV1, FreshGuildAuthorityPort, InstallationSelectorV1,
    MutationAuthenticationPort, ProductPromotionObservationV1, ProductRequestIdV1,
    PromoteOwnedSessionV1, PromotionSubmissionV1,
};

impl<A, G, S, P> AuthoringApplication<'_, A, G, S, P>
where
    A: MutationAuthenticationPort,
    G: FreshGuildAuthorityPort,
    S: AuthorizedPromotionSnapshotPort<G::Evidence>,
    P: AuthorizedPromotionSubmissionPort<G::Evidence>,
{
    pub async fn promote_owned_session(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
    ) -> Result<PromotionSubmissionV1, AuthoringApplicationError> {
        Ok(self
            .promote_owned_session_inner(credential, csrf, request_id, installation, command, false)
            .await?
            .0)
    }

    pub async fn promote_owned_session_observation(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
    ) -> Result<ProductPromotionObservationV1, AuthoringApplicationError> {
        let (_, observation) = self
            .promote_owned_session_inner(credential, csrf, request_id, installation, command, true)
            .await?;
        observation.ok_or({
            AuthoringApplicationError::AuthorizedPromotion(
                crate::AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt,
            )
        })
    }

    async fn promote_owned_session_inner(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
        include_observation: bool,
    ) -> Result<
        (PromotionSubmissionV1, Option<ProductPromotionObservationV1>),
        AuthoringApplicationError,
    > {
        let claims = self
            .authentication
            .authenticate_mutation(credential, csrf)
            .await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, CapabilityV1::Promote)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        let access = AuthorizedPromotionAccessV1::new(
            request_id,
            &actor,
            authorized.scope(),
            authorized.evidence(),
            command,
        );
        if let Some(submission) = self
            .promotions
            .find_or_resume_authorized_promotion(&access)
            .await?
        {
            let record = validate_authorized_replay(&access, &submission)?;
            let observation = include_observation
                .then(|| product_promotion_observation(record, submission.disposition))
                .transpose()?;
            return Ok((submission, observation));
        }
        let snapshot = self
            .snapshots
            .load_atomic_authorized_snapshot(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                access.session_id(),
                access.expected_generation(),
            )
            .await?;
        let input = build_start_promotion(&actor, authorized.scope(), &access, snapshot)?;
        let plan = plan_start_promotion_ref_v1(&input)?;
        let expected = ExpectedPromotionSubmissionV1::from_plan(&plan);
        let submission = self
            .promotions
            .submit_authorized_promotion(AuthorizedPromotionSubmissionV1::new(access, input, plan))
            .await?;
        let record = validate_authorized_submission(&expected, &submission)?;
        let observation = include_observation
            .then(|| product_promotion_observation(record, submission.disposition))
            .transpose()?;
        Ok((submission, observation))
    }
}
