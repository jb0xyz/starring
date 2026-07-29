mod approval_query;
mod decision_mutation;
mod lifecycle_cancellation;
mod projection_validation;
mod promotion_flow;
mod status_query;

use crate::authority::validate_authorized_scope;
use crate::{
    AuthenticatedActorV1, AuthenticationPort, AuthorizedInstallationV1, CapabilityV1,
    FreshGuildAuthorityPort, InstallationSelectorV1, MutationAuthenticationPort,
    ProductApplicationError,
};

pub struct AuthoringApplication<'a, A, G, S, P> {
    authentication: &'a A,
    guild_authority: &'a G,
    snapshots: &'a S,
    promotions: &'a P,
}

impl<'a, A, G, S, P> AuthoringApplication<'a, A, G, S, P> {
    pub fn new(
        authentication: &'a A,
        guild_authority: &'a G,
        snapshots: &'a S,
        promotions: &'a P,
    ) -> Self {
        Self {
            authentication,
            guild_authority,
            snapshots,
            promotions,
        }
    }
}

pub struct ProductControlApplication<'a, A, G, D, R> {
    authentication: &'a A,
    guild_authority: &'a G,
    decisions: &'a D,
    deployments: &'a R,
}

impl<'a, A, G, D, R> ProductControlApplication<'a, A, G, D, R> {
    pub fn new(
        authentication: &'a A,
        guild_authority: &'a G,
        decisions: &'a D,
        deployments: &'a R,
    ) -> Self {
        Self {
            authentication,
            guild_authority,
            decisions,
            deployments,
        }
    }
}

impl<A, G, D, R> ProductControlApplication<'_, A, G, D, R>
where
    A: AuthenticationPort,
    G: FreshGuildAuthorityPort,
{
    async fn authenticate_and_authorize(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<
        (AuthenticatedActorV1, AuthorizedInstallationV1<G::Evidence>),
        ProductApplicationError,
    > {
        let claims = self.authentication.authenticate(credential).await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, capability)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        Ok((actor, authorized))
    }

    async fn authenticate_mutation_and_authorize(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<
        (AuthenticatedActorV1, AuthorizedInstallationV1<G::Evidence>),
        ProductApplicationError,
    >
    where
        A: MutationAuthenticationPort,
    {
        let claims = self
            .authentication
            .authenticate_mutation(credential, csrf)
            .await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, capability)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        Ok((actor, authorized))
    }
}
