use std::fmt::{Debug, Formatter};

use automation_runtime_interaction::{
    DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionExpectedRouteV1,
    InteractionGatewayShardIdentityV1, InteractionProductScopeV1,
    InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1,
    InteractionRouteIncarnationV1, InteractionRuntimeBuildRevisionV1,
};
use zeroize::Zeroizing;

use crate::shared_gateway_admission::SharedGatewayAdmittedInteractionV3;
use crate::shared_gateway_dispatcher::SharedGatewayInteractionEnvelopeV3;
use crate::shared_gateway_router::{parse_shared_gateway_route_v1, SharedGatewayRouteHintV1};

pub struct SharedGatewayDurableReceiptClaimInputV1 {
    candidate: InteractionReceiptClaimCandidateV1,
    route_hint: SharedGatewayRouteHintV1,
    interaction_token: Zeroizing<String>,
}

impl SharedGatewayDurableReceiptClaimInputV1 {
    pub fn candidate(&self) -> &InteractionReceiptClaimCandidateV1 {
        &self.candidate
    }

    pub fn route_hint(&self) -> &SharedGatewayRouteHintV1 {
        &self.route_hint
    }

    pub fn expose_interaction_token(&self) -> &str {
        self.interaction_token.as_str()
    }
}

impl Debug for SharedGatewayDurableReceiptClaimInputV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayDurableReceiptClaimInputV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SharedGatewayDurableReceiptClaimInputErrorV1 {
    #[error("shared gateway durable receipt Discord identity is invalid")]
    Identity,
    #[error("shared gateway durable receipt route hint is invalid")]
    RouteHint,
    #[error("shared gateway durable receipt route does not match admission")]
    RouteMismatch,
    #[error("shared gateway durable receipt expected route is invalid")]
    ExpectedRoute,
    #[error("shared gateway durable receipt request digest is invalid")]
    RequestDigest,
}

impl SharedGatewayDurableReceiptClaimInputErrorV1 {
    pub fn code(self) -> &'static str {
        match self {
            Self::Identity => "shared_gateway_durable_receipt_identity_invalid",
            Self::RouteHint => "shared_gateway_durable_receipt_route_hint_invalid",
            Self::RouteMismatch => "shared_gateway_durable_receipt_route_mismatch",
            Self::ExpectedRoute => "shared_gateway_durable_receipt_expected_route_invalid",
            Self::RequestDigest => "shared_gateway_durable_receipt_request_digest_invalid",
        }
    }
}

pub fn build_shared_gateway_durable_receipt_claim_input_v1(
    envelope: &SharedGatewayInteractionEnvelopeV3,
    admitted: &SharedGatewayAdmittedInteractionV3,
    gateway_shard_identity: InteractionGatewayShardIdentityV1,
    runtime_build_revision: InteractionRuntimeBuildRevisionV1,
) -> Result<SharedGatewayDurableReceiptClaimInputV1, SharedGatewayDurableReceiptClaimInputErrorV1> {
    let envelope_identity = envelope.identity_v3();
    let receipt_identity = InteractionReceiptIdentityV1::new(
        DiscordApplicationIdV1::new(envelope_identity.application_id().get())
            .map_err(|_| SharedGatewayDurableReceiptClaimInputErrorV1::Identity)?,
        DiscordInteractionIdV1::new(envelope_identity.interaction_id().get())
            .map_err(|_| SharedGatewayDurableReceiptClaimInputErrorV1::Identity)?,
    );
    let route_hint =
        parse_shared_gateway_route_v1(envelope_identity.guild_id(), envelope.custom_id_v3())
            .map_err(|_| SharedGatewayDurableReceiptClaimInputErrorV1::RouteHint)?
            .ok_or(SharedGatewayDurableReceiptClaimInputErrorV1::RouteHint)?;
    let route = admitted.route();
    let token = admitted.token();
    let route_key = route.slot_key();
    if route.process_identity() != token.identity()
        || token.key() != &route_key
        || route.process_identity().target.guild_id != envelope_identity.guild_id()
        || matches!(&route_hint, SharedGatewayRouteHintV1::Static(key) if key != &route_key)
    {
        return Err(SharedGatewayDurableReceiptClaimInputErrorV1::RouteMismatch);
    }
    let route_incarnation = InteractionRouteIncarnationV1::new(token.route_incarnation().get())
        .map_err(|_| SharedGatewayDurableReceiptClaimInputErrorV1::ExpectedRoute)?;
    let expected_route = InteractionExpectedRouteV1::new(
        InteractionProductScopeV1::from_deployment_identity(route.deployment_identity()),
        route.process_identity().clone(),
        gateway_shard_identity,
        runtime_build_revision,
        token.fencing_token(),
        route_incarnation,
    )
    .map_err(|_| SharedGatewayDurableReceiptClaimInputErrorV1::ExpectedRoute)?;
    let request_digest = envelope
        .receipt_request_digest_v1(receipt_identity)
        .map_err(|_| SharedGatewayDurableReceiptClaimInputErrorV1::RequestDigest)?;
    Ok(SharedGatewayDurableReceiptClaimInputV1 {
        candidate: InteractionReceiptClaimCandidateV1::new(
            receipt_identity,
            expected_route,
            request_digest,
        ),
        route_hint,
        interaction_token: envelope.receipt_interaction_token_copy_v1(),
    })
}

#[cfg(test)]
mod tests;
