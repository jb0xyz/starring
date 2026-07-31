use automation_instance::InstanceId;
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, FencingToken, InstallationId, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    DiscordApplicationIdV1, DiscordInteractionIdV1, EncryptedInteractionTokenV1,
    InteractionExpectedRouteV1, InteractionGatewayOwnerIdentityV1,
    InteractionGatewayOwnerLeaseEpochV1, InteractionGatewayOwnerRevisionV1,
    InteractionGatewayShardIdentityV1, InteractionInstanceManifestDigestV1,
    InteractionProductScopeV1, InteractionReceiptClaimCandidateV1, InteractionReceiptClaimRootV1,
    InteractionReceiptIdentityV1, InteractionReceiptStateV1, InteractionRequestDigestV1,
    InteractionRouteAttestationDigestV1, InteractionRouteBindingV1, InteractionRouteIncarnationV1,
    InteractionRuntimeBuildRevisionV1, InteractionServingLeaseEpochV1,
    InteractionServingLeaseRevisionV1, InteractionServingRouteIdentityV1,
    InteractionTokenAuthenticatedDataDigestV1, InteractionTokenEnvelopeTimeV1,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use crate::receipt::{
    unix_milliseconds, validate_database_time, validate_envelope_authenticated_data,
    RuntimeInteractionReceiptAuthorityV1, RuntimeInteractionReceiptClaimDuplicateV1,
    RuntimeInteractionReceiptClaimOutcomeV1, RuntimeInteractionReceiptClaimRequestV1,
    RuntimeInteractionReceiptExclusiveClaimV1, RuntimeInteractionReceiptRecoveredClaimV1,
    RuntimeInteractionReceiptRecoveryCandidateV1,
    RuntimeInteractionReceiptRecoveryDeferredReasonV1, RuntimeInteractionReceiptRecoveryOutcomeV1,
    RuntimeInteractionReceiptRecoveryRequestV1, RuntimeInteractionReceiptRecoveryRequiredReasonV1,
    RuntimeInteractionReceiptRecoveryScanKeyV1, RuntimeInteractionReceiptRouteV1,
    RuntimeInteractionReceiptTerminalizeExpiredDispositionV1,
    RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1,
    RuntimeInteractionReceiptTokenExpiryDispositionV1,
    RuntimeInteractionReceiptTokenExpiryOutcomeV1,
};
use crate::RuntimeInteractionPersistenceErrorV1;

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptAuthorityRowV1 {
    pub(crate) tenant_id: String,
    pub(crate) installation_id: String,
    pub(crate) deployment_id: String,
    pub(crate) attestation_id: String,
    pub(crate) attestation_digest: String,
    pub(crate) serving_lease_epoch: i64,
    pub(crate) serving_revision: i64,
    pub(crate) gateway_owner_lease_epoch: i64,
    pub(crate) gateway_owner_revision: i64,
    pub(crate) route_controller_fencing_token: i64,
    pub(crate) route_incarnation: i64,
    pub(crate) runtime_build_revision: String,
    pub(crate) execution_ruleset_version: i64,
    pub(crate) execution_ruleset_content_hash: String,
    pub(crate) instance_manifest_digest: Option<String>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptClaimRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) derived_tenant_id: String,
    pub(crate) derived_installation_id: String,
    pub(crate) derived_deployment_id: String,
    pub(crate) derived_attestation_id: String,
    pub(crate) derived_attestation_digest: String,
    pub(crate) derived_serving_lease_epoch: i64,
    pub(crate) derived_serving_revision: i64,
    pub(crate) derived_gateway_owner_lease_epoch: i64,
    pub(crate) derived_gateway_owner_revision: i64,
    pub(crate) derived_route_controller_fencing_token: i64,
    pub(crate) derived_route_incarnation: i64,
    pub(crate) derived_runtime_build_revision: String,
    pub(crate) derived_execution_ruleset_version: i64,
    pub(crate) derived_execution_ruleset_content_hash: String,
    pub(crate) derived_instance_manifest_digest: Option<String>,
    pub(crate) receipt_state: String,
    pub(crate) resulting_head_revision: i64,
    pub(crate) resulting_claim_revision: i64,
    pub(crate) resulting_claim_expires_at: DateTime<Utc>,
    pub(crate) resulting_token_issued_at: Option<DateTime<Utc>>,
    pub(crate) resulting_token_expires_at: Option<DateTime<Utc>>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptMutationRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) receipt_state: String,
    pub(crate) resulting_head_revision: i64,
    pub(crate) resulting_claim_revision: i64,
    pub(crate) resulting_claim_expires_at: DateTime<Utc>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptRecoveryScanRowV1 {
    pub(crate) application_id: String,
    pub(crate) interaction_id: String,
    pub(crate) receipt_state: String,
    pub(crate) head_revision: i64,
    pub(crate) claim_revision: i64,
    pub(crate) claim_expires_at: DateTime<Utc>,
    pub(crate) token_expires_at: Option<DateTime<Utc>>,
    pub(crate) through_claim_expires_at: DateTime<Utc>,
    pub(crate) through_application_id: String,
    pub(crate) through_interaction_id: String,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptRecoverRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) receipt_state: String,
    pub(crate) resulting_head_revision: i64,
    pub(crate) resulting_claim_revision: i64,
    pub(crate) resulting_claim_expires_at: DateTime<Utc>,
    pub(crate) resulting_gateway_owner_lease_epoch: Option<i64>,
    pub(crate) resulting_gateway_owner_revision: Option<i64>,
    pub(crate) resulting_serving_lease_epoch: Option<i64>,
    pub(crate) resulting_serving_revision: Option<i64>,
    pub(crate) root_tenant_id: Option<String>,
    pub(crate) root_installation_id: Option<String>,
    pub(crate) root_deployment_id: Option<String>,
    pub(crate) root_attestation_digest: Option<String>,
    pub(crate) root_guild_id: Option<String>,
    pub(crate) root_ruleset_key: Option<String>,
    pub(crate) root_target_version: Option<i64>,
    pub(crate) root_target_content_hash: Option<String>,
    pub(crate) root_binding_revision: Option<i64>,
    pub(crate) root_binding_fingerprint: Option<String>,
    pub(crate) root_runtime_generation: Option<i64>,
    pub(crate) root_process_instance_id: Option<String>,
    pub(crate) root_serving_lease_epoch: Option<i64>,
    pub(crate) root_serving_revision: Option<i64>,
    pub(crate) root_gateway_shard_id: Option<String>,
    pub(crate) root_gateway_owner_lease_epoch: Option<i64>,
    pub(crate) root_gateway_owner_revision: Option<i64>,
    pub(crate) root_route_controller_fencing_token: Option<i64>,
    pub(crate) root_route_incarnation: Option<i64>,
    pub(crate) root_runtime_build_revision: Option<String>,
    pub(crate) root_route_kind: Option<String>,
    pub(crate) root_route_key: Option<String>,
    pub(crate) root_instance_id: Option<String>,
    pub(crate) root_execution_ruleset_version: Option<i64>,
    pub(crate) root_execution_ruleset_content_hash: Option<String>,
    pub(crate) root_instance_manifest_digest: Option<String>,
    pub(crate) root_request_digest: Option<Vec<u8>>,
    pub(crate) token_encryption_suite: Option<String>,
    pub(crate) token_suite_version: Option<i16>,
    pub(crate) token_key_id: Option<String>,
    pub(crate) token_nonce: Option<Vec<u8>>,
    pub(crate) token_ciphertext: Option<Vec<u8>>,
    pub(crate) token_aad_digest: Option<Vec<u8>>,
    pub(crate) token_issued_at: Option<DateTime<Utc>>,
    pub(crate) token_expires_at: Option<DateTime<Utc>>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptTokenExpiryRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) receipt_state: String,
    pub(crate) resulting_head_revision: i64,
    pub(crate) resulting_claim_revision: i64,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ReceiptTerminalizeExpiredRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) receipt_state: String,
    pub(crate) resulting_head_revision: i64,
    pub(crate) resulting_claim_revision: i64,
    pub(crate) resulting_claim_expires_at: DateTime<Utc>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

pub(crate) struct ReceiptCheckpointV1 {
    pub(crate) outcome_name: String,
    pub(crate) state: InteractionReceiptStateV1,
    pub(crate) head_revision: u64,
    pub(crate) claim_revision: u64,
    pub(crate) claim_expires_at: DateTime<Utc>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

impl ReceiptAuthorityRowV1 {
    pub(crate) fn decode(
        self,
        candidate: InteractionReceiptClaimCandidateV1,
        route: RuntimeInteractionReceiptRouteV1,
    ) -> Result<RuntimeInteractionReceiptAuthorityV1, RuntimeInteractionPersistenceErrorV1> {
        validate_database_time(self.observed_database_now, false)?;
        validate_lower_hex(&self.attestation_id)?;
        let expected = candidate.expected_route();
        let scope = decode_scope(self.tenant_id, self.installation_id, self.deployment_id)?;
        if &scope != expected.scope()
            || positive_u64(self.route_controller_fencing_token)?
                != expected.route_fencing_token().get()
            || positive_u64(self.route_incarnation)? != expected.route_incarnation().get()
            || self.runtime_build_revision != expected.runtime_build_revision().as_str()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let binding = decode_binding(
            scope,
            expected.process_identity().clone(),
            expected.gateway_shard_identity().clone(),
            self.attestation_digest,
            self.serving_lease_epoch,
            self.serving_revision,
            self.gateway_owner_lease_epoch,
            self.gateway_owner_revision,
            self.route_controller_fencing_token,
            self.route_incarnation,
            self.runtime_build_revision,
            &route,
            self.execution_ruleset_version,
            self.execution_ruleset_content_hash,
            self.instance_manifest_digest,
        )?;
        let root = candidate
            .bind_authoritative(binding)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        Ok(RuntimeInteractionReceiptAuthorityV1::new(
            root,
            route,
            self.observed_database_now,
        ))
    }
}

impl ReceiptClaimRowV1 {
    pub(crate) fn decode(
        self,
        request: &RuntimeInteractionReceiptClaimRequestV1,
    ) -> Result<RuntimeInteractionReceiptClaimOutcomeV1, RuntimeInteractionPersistenceErrorV1> {
        validate_database_time(self.observed_database_now, false)?;
        validate_database_time(self.resulting_claim_expires_at, false)?;
        validate_lower_hex(&self.derived_attestation_id)?;
        let expected_root = request.claim_root();
        let expected_route = expected_root.route();
        let binding = decode_binding(
            decode_scope(
                self.derived_tenant_id,
                self.derived_installation_id,
                self.derived_deployment_id,
            )?,
            expected_route.process_identity().clone(),
            expected_route
                .serving_identity()
                .gateway_shard_identity()
                .clone(),
            self.derived_attestation_digest,
            self.derived_serving_lease_epoch,
            self.derived_serving_revision,
            self.derived_gateway_owner_lease_epoch,
            self.derived_gateway_owner_revision,
            self.derived_route_controller_fencing_token,
            self.derived_route_incarnation,
            self.derived_runtime_build_revision,
            request.authority().route(),
            self.derived_execution_ruleset_version,
            self.derived_execution_ruleset_content_hash,
            self.derived_instance_manifest_digest,
        )?;
        let decoded_root = InteractionReceiptClaimCandidateV1::new(
            expected_root.identity(),
            InteractionExpectedRouteV1::from_authoritative(expected_root.route()),
            expected_root.request_digest().clone(),
        )
        .bind_authoritative(binding)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if &decoded_root != expected_root {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let state = decode_state(&self.receipt_state)?;
        let head_revision = positive_u64(self.resulting_head_revision)?;
        let claim_revision = positive_u64(self.resulting_claim_revision)?;
        if claim_revision > head_revision
            || self.resulting_token_issued_at.is_some() != self.resulting_token_expires_at.is_some()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        if let Some(issued_at) = self.resulting_token_issued_at {
            let expires_at = self
                .resulting_token_expires_at
                .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            validate_database_time(issued_at, false)?;
            validate_database_time(expires_at, false)?;
            if issued_at >= expires_at {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
        }
        let identity = expected_root.identity();
        match self.outcome_name.as_str() {
            "claimed_new" | "pristine_claim_recovered" => {
                if state != InteractionReceiptStateV1::Claimed
                    || self.resulting_claim_expires_at <= self.observed_database_now
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                let returned_issued = self
                    .resulting_token_issued_at
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                let returned_expires = self
                    .resulting_token_expires_at
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                if returned_expires <= self.observed_database_now
                    || self.outcome_name == "claimed_new"
                        && (head_revision != 1
                            || claim_revision != 1
                            || returned_issued
                                != crate::receipt::datetime_from_unix_milliseconds(
                                    request
                                        .encrypted_token()
                                        .time()
                                        .issued_at_unix_milliseconds(),
                                )?
                            || returned_expires
                                != crate::receipt::datetime_from_unix_milliseconds(
                                    request
                                        .encrypted_token()
                                        .time()
                                        .expires_at_unix_milliseconds(),
                                )?)
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(RuntimeInteractionReceiptClaimOutcomeV1::Acquired(Box::new(
                    RuntimeInteractionReceiptExclusiveClaimV1::new(
                        decoded_root,
                        state,
                        head_revision,
                        claim_revision,
                        expected_root
                            .route()
                            .process_identity()
                            .process_instance_id
                            .clone(),
                        self.resulting_claim_expires_at,
                        self.observed_database_now,
                    ),
                )))
            }
            "in_flight_duplicate" => {
                if !state.is_in_flight()
                    || self.resulting_claim_expires_at <= self.observed_database_now
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(RuntimeInteractionReceiptClaimOutcomeV1::InFlightDuplicate(
                    duplicate(
                        identity,
                        state,
                        head_revision,
                        claim_revision,
                        self.resulting_claim_expires_at,
                        self.observed_database_now,
                    ),
                ))
            }
            "terminal_duplicate" => {
                let duplicate = duplicate(
                    identity,
                    state,
                    head_revision,
                    claim_revision,
                    self.resulting_claim_expires_at,
                    self.observed_database_now,
                );
                match state {
                    InteractionReceiptStateV1::Completed => Ok(
                        RuntimeInteractionReceiptClaimOutcomeV1::CompletedDuplicate(duplicate),
                    ),
                    InteractionReceiptStateV1::Failed => Ok(
                        RuntimeInteractionReceiptClaimOutcomeV1::TerminalDuplicate(duplicate),
                    ),
                    InteractionReceiptStateV1::RecoveryRequired => Ok(
                        RuntimeInteractionReceiptClaimOutcomeV1::RecoveryRequired(duplicate),
                    ),
                    _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
                }
            }
            "recovery_required_duplicate" => {
                if state != InteractionReceiptStateV1::RecoveryRequired {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(RuntimeInteractionReceiptClaimOutcomeV1::RecoveryRequired(
                    duplicate(
                        identity,
                        state,
                        head_revision,
                        claim_revision,
                        self.resulting_claim_expires_at,
                        self.observed_database_now,
                    ),
                ))
            }
            _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        }
    }
}

impl ReceiptMutationRowV1 {
    pub(crate) fn decode(
        self,
        expected_claim_revision: u64,
        expected_claim_expires_at: DateTime<Utc>,
    ) -> Result<ReceiptCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        let state = decode_state(&self.receipt_state)?;
        let head_revision = positive_u64(self.resulting_head_revision)?;
        let claim_revision = positive_u64(self.resulting_claim_revision)?;
        validate_database_time(self.resulting_claim_expires_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        if claim_revision != expected_claim_revision
            || claim_revision > head_revision
            || self.resulting_claim_expires_at != expected_claim_expires_at
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(ReceiptCheckpointV1 {
            outcome_name: self.outcome_name,
            state,
            head_revision,
            claim_revision,
            claim_expires_at: self.resulting_claim_expires_at,
            observed_database_now: self.observed_database_now,
        })
    }
}

impl ReceiptRecoveryScanRowV1 {
    pub(crate) fn decode(
        self,
    ) -> Result<
        (
            RuntimeInteractionReceiptRecoveryCandidateV1,
            RuntimeInteractionReceiptRecoveryScanKeyV1,
            DateTime<Utc>,
        ),
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.observed_database_now, false)?;
        let identity = decode_receipt_identity(self.application_id, self.interaction_id)?;
        let key = RuntimeInteractionReceiptRecoveryScanKeyV1::new(self.claim_expires_at, identity)?;
        let through = RuntimeInteractionReceiptRecoveryScanKeyV1::new(
            self.through_claim_expires_at,
            decode_receipt_identity(self.through_application_id, self.through_interaction_id)?,
        )?;
        let state = decode_state(&self.receipt_state)?;
        let head_revision = positive_u64(self.head_revision)?;
        let claim_revision = positive_u64(self.claim_revision)?;
        if !state.is_in_flight()
            || claim_revision > head_revision
            || key.claim_expires_at() > self.observed_database_now
            || key.cmp_c(&through).is_gt()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        if let Some(token_expires_at) = self.token_expires_at {
            validate_database_time(token_expires_at, false)?;
        }
        Ok((
            RuntimeInteractionReceiptRecoveryCandidateV1::new(
                key,
                state,
                head_revision,
                claim_revision,
                self.token_expires_at,
            ),
            through,
            self.observed_database_now,
        ))
    }
}

impl ReceiptRecoverRowV1 {
    pub(crate) fn decode(
        self,
        request: &RuntimeInteractionReceiptRecoveryRequestV1,
    ) -> Result<RuntimeInteractionReceiptRecoveryOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        validate_database_time(self.resulting_claim_expires_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        let state = decode_state(&self.receipt_state)?;
        let head_revision = positive_u64(self.resulting_head_revision)?;
        let claim_revision = positive_u64(self.resulting_claim_revision)?;
        if claim_revision > head_revision {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let identity = request.candidate().key().identity();
        let duplicate = || {
            duplicate(
                identity,
                state,
                head_revision,
                claim_revision,
                self.resulting_claim_expires_at,
                self.observed_database_now,
            )
        };
        match self.outcome_name.as_str() {
            "terminal_duplicate" => {
                ensure_recovery_payload_absent(&self)?;
                match state {
                    InteractionReceiptStateV1::Completed => Ok(
                        RuntimeInteractionReceiptRecoveryOutcomeV1::CompletedDuplicate(duplicate()),
                    ),
                    InteractionReceiptStateV1::Failed => Ok(
                        RuntimeInteractionReceiptRecoveryOutcomeV1::TerminalDuplicate(duplicate()),
                    ),
                    InteractionReceiptStateV1::RecoveryRequired => Ok(
                        RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryRequired {
                            receipt: duplicate(),
                            reason:
                                RuntimeInteractionReceiptRecoveryRequiredReasonV1::AlreadyRequired,
                        },
                    ),
                    _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
                }
            }
            "in_flight_duplicate" => {
                ensure_recovery_payload_absent(&self)?;
                if !state.is_in_flight()
                    || self.resulting_claim_expires_at <= self.observed_database_now
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(RuntimeInteractionReceiptRecoveryOutcomeV1::InFlightDuplicate(duplicate()))
            }
            "successor_process_recovery_deferred" => {
                ensure_recovery_payload_absent(&self)?;
                if !state.is_in_flight()
                    || head_revision != request.candidate().head_revision()
                    || claim_revision != request.candidate().claim_revision()
                    || self.resulting_claim_expires_at > self.observed_database_now
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(
                    RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryDeferred {
                        receipt: duplicate(),
                        reason: RuntimeInteractionReceiptRecoveryDeferredReasonV1::SuccessorProcess,
                    },
                )
            }
            "interaction_response_unrecoverable"
            | "interaction_token_unavailable"
            | "expired_claim_recovery_required" => {
                ensure_recovery_root_and_token_payload_absent(&self)?;
                positive_optional(self.resulting_gateway_owner_lease_epoch)?;
                positive_optional(self.resulting_gateway_owner_revision)?;
                positive_optional(self.resulting_serving_lease_epoch)?;
                positive_optional(self.resulting_serving_revision)?;
                if state != InteractionReceiptStateV1::RecoveryRequired
                    || head_revision != request.candidate().head_revision() + 1
                    || claim_revision != request.candidate().claim_revision()
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                let reason = match self.outcome_name.as_str() {
                    "interaction_response_unrecoverable" => {
                        RuntimeInteractionReceiptRecoveryRequiredReasonV1::ResponseUnrecoverable
                    }
                    "interaction_token_unavailable" => {
                        RuntimeInteractionReceiptRecoveryRequiredReasonV1::TokenUnavailable
                    }
                    "expired_claim_recovery_required" => {
                        RuntimeInteractionReceiptRecoveryRequiredReasonV1::UnsafeToResume
                    }
                    _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
                };
                Ok(
                    RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryRequired {
                        receipt: duplicate(),
                        reason,
                    },
                )
            }
            "claim_recovered" | "claim_recovered_acknowledged" => {
                if !state.is_in_flight()
                    || head_revision != request.candidate().head_revision() + 1
                    || claim_revision != request.candidate().claim_revision() + 1
                    || self.resulting_claim_expires_at <= self.observed_database_now
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                positive_optional(self.resulting_gateway_owner_lease_epoch)?;
                positive_optional(self.resulting_gateway_owner_revision)?;
                positive_optional(self.resulting_serving_lease_epoch)?;
                positive_optional(self.resulting_serving_revision)?;
                let (root, route) =
                    decode_recovery_root(&self, identity, request.expected_route())?;
                let token = decode_recovery_token(&self, &root)?;
                let now = unix_milliseconds(self.observed_database_now)?;
                token
                    .time()
                    .ensure_unexpired(now)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                let claim = RuntimeInteractionReceiptExclusiveClaimV1::new(
                    root,
                    state,
                    head_revision,
                    claim_revision,
                    request
                        .expected_route()
                        .process_identity()
                        .process_instance_id
                        .clone(),
                    self.resulting_claim_expires_at,
                    self.observed_database_now,
                );
                Ok(RuntimeInteractionReceiptRecoveryOutcomeV1::Recovered(
                    Box::new(RuntimeInteractionReceiptRecoveredClaimV1::new(
                        claim,
                        route,
                        token,
                        self.outcome_name == "claim_recovered_acknowledged",
                    )),
                ))
            }
            _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        }
    }
}

impl ReceiptTokenExpiryRowV1 {
    pub(crate) fn decode(
        self,
        expected_head_revision: u64,
        expected_claim_revision: u64,
    ) -> Result<RuntimeInteractionReceiptTokenExpiryOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        validate_database_time(self.observed_database_now, false)?;
        let state = decode_state(&self.receipt_state)?;
        let head_revision = positive_u64(self.resulting_head_revision)?;
        let claim_revision = positive_u64(self.resulting_claim_revision)?;
        if claim_revision > head_revision {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let disposition = match self.outcome_name.as_str() {
            "token_absent"
                if state.is_terminal()
                    && head_revision == expected_head_revision
                    && claim_revision == expected_claim_revision =>
            {
                RuntimeInteractionReceiptTokenExpiryDispositionV1::TokenAbsent
            }
            "token_not_expired"
                if head_revision == expected_head_revision
                    && claim_revision == expected_claim_revision =>
            {
                RuntimeInteractionReceiptTokenExpiryDispositionV1::TokenNotExpired
            }
            "terminal_token_deleted"
                if state.is_terminal()
                    && head_revision == expected_head_revision
                    && claim_revision == expected_claim_revision =>
            {
                RuntimeInteractionReceiptTokenExpiryDispositionV1::TerminalTokenDeleted
            }
            "interaction_token_expired" | "interaction_token_unavailable"
                if state == InteractionReceiptStateV1::RecoveryRequired
                    && head_revision == expected_head_revision + 1
                    && claim_revision == expected_claim_revision =>
            {
                RuntimeInteractionReceiptTokenExpiryDispositionV1::RecoveryRequired
            }
            _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        };
        Ok(RuntimeInteractionReceiptTokenExpiryOutcomeV1::new(
            disposition,
            state,
            head_revision,
            claim_revision,
            self.observed_database_now,
        ))
    }
}

impl ReceiptTerminalizeExpiredRowV1 {
    pub(crate) fn decode(
        self,
        expected_head_revision: u64,
        expected_claim_revision: u64,
    ) -> Result<
        RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.resulting_claim_expires_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        let state = decode_state(&self.receipt_state)?;
        let head_revision = positive_u64(self.resulting_head_revision)?;
        let claim_revision = positive_u64(self.resulting_claim_revision)?;
        if claim_revision > head_revision {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let revisions_match =
            head_revision == expected_head_revision && claim_revision == expected_claim_revision;
        let expired = self.resulting_claim_expires_at <= self.observed_database_now;
        let disposition = match self.outcome_name.as_str() {
            "expired_claim_recovery_required"
                if state == InteractionReceiptStateV1::RecoveryRequired
                    && head_revision == expected_head_revision + 1
                    && claim_revision == expected_claim_revision
                    && expired =>
            {
                RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RecoveryRequired
            }
            "expired_pristine_claim_abandoned"
                if state == InteractionReceiptStateV1::RecoveryRequired
                    && head_revision == expected_head_revision + 1
                    && claim_revision == expected_claim_revision
                    && expired =>
            {
                RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::PristineClaimAbandoned
            }
            "terminal_receipt_unchanged" if state.is_terminal() => {
                RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::TerminalReceipt
            }
            "claim_renewed" if state.is_in_flight() && revisions_match && !expired => {
                RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::ClaimRenewed
            }
            "revision_race" if !state.is_terminal() && !revisions_match => {
                RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RevisionRace
            }
            "route_authority_stale"
                if matches!(
                    state,
                    InteractionReceiptStateV1::Claimed
                        | InteractionReceiptStateV1::Acknowledging
                        | InteractionReceiptStateV1::Deferred
                        | InteractionReceiptStateV1::Prepared
                        | InteractionReceiptStateV1::Executing
                ) && revisions_match
                    && expired =>
            {
                RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RouteAuthorityStale
            }
            _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        };
        Ok(RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1::new(
            disposition,
            state,
            head_revision,
            claim_revision,
            self.resulting_claim_expires_at,
            self.observed_database_now,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_binding(
    scope: InteractionProductScopeV1,
    process_identity: RuntimeProcessIdentityV1,
    gateway_shard: InteractionGatewayShardIdentityV1,
    attestation_digest: String,
    serving_lease_epoch: i64,
    serving_revision: i64,
    gateway_owner_lease_epoch: i64,
    gateway_owner_revision: i64,
    route_fencing_token: i64,
    route_incarnation: i64,
    runtime_build_revision: String,
    route: &RuntimeInteractionReceiptRouteV1,
    execution_ruleset_version: i64,
    execution_ruleset_content_hash: String,
    instance_manifest_digest: Option<String>,
) -> Result<InteractionRouteBindingV1, RuntimeInteractionPersistenceErrorV1> {
    let execution_version = decode_ruleset_version(execution_ruleset_version)?;
    let execution_hash = RuleSetContentHash::parse_hex(&execution_ruleset_content_hash)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let serving = InteractionServingRouteIdentityV1::new(
        InteractionRouteAttestationDigestV1::parse(attestation_digest)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        InteractionServingLeaseEpochV1::new(positive_u64(serving_lease_epoch)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        InteractionServingLeaseRevisionV1::new(positive_u64(serving_revision)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        InteractionGatewayOwnerIdentityV1::new(
            gateway_shard,
            InteractionGatewayOwnerLeaseEpochV1::new(positive_u64(gateway_owner_lease_epoch)?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            InteractionGatewayOwnerRevisionV1::new(positive_u64(gateway_owner_revision)?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            InteractionRuntimeBuildRevisionV1::parse(runtime_build_revision)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        ),
        FencingToken::new(positive_u64(route_fencing_token)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        InteractionRouteIncarnationV1::new(positive_u64(route_incarnation)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    );
    match route {
        RuntimeInteractionReceiptRouteV1::Static { .. } => {
            if execution_version != process_identity.target.version
                || execution_hash != process_identity.target.content_hash
                || instance_manifest_digest.is_some()
            {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            InteractionRouteBindingV1::new_static(scope, process_identity, serving)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        }
        RuntimeInteractionReceiptRouteV1::Instance { instance_id, .. } => {
            let manifest = instance_manifest_digest
                .and_then(|value| InteractionInstanceManifestDigestV1::parse(value).ok())
                .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            InteractionRouteBindingV1::new_instance(
                scope,
                process_identity,
                serving,
                instance_id.clone(),
                execution_version,
                execution_hash,
                manifest,
            )
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        }
    }
}

fn decode_recovery_root(
    row: &ReceiptRecoverRowV1,
    identity: InteractionReceiptIdentityV1,
    expected_route: &InteractionExpectedRouteV1,
) -> Result<
    (
        InteractionReceiptClaimRootV1,
        RuntimeInteractionReceiptRouteV1,
    ),
    RuntimeInteractionPersistenceErrorV1,
> {
    let scope = decode_scope(
        required(&row.root_tenant_id)?.clone(),
        required(&row.root_installation_id)?.clone(),
        required(&row.root_deployment_id)?.clone(),
    )?;
    let guild_text = required(&row.root_guild_id)?;
    let guild_id = decode_guild_id(guild_text)?;
    let target = RuntimeDeploymentTargetV1 {
        guild_id,
        ruleset_key: RuleSetKey::parse(required(&row.root_ruleset_key)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        version: decode_ruleset_version(required(&row.root_target_version).copied()?)?,
        content_hash: RuleSetContentHash::parse_hex(required(&row.root_target_content_hash)?)
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        binding_revision: BindingRevision::new(positive_u64(
            required(&row.root_binding_revision).copied()?,
        )?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        binding_fingerprint: ResourceBindingFingerprint::parse(required(
            &row.root_binding_fingerprint,
        )?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    };
    let process = RuntimeProcessIdentityV1 {
        target,
        runtime_generation: RuntimeGeneration::new(positive_u64(
            required(&row.root_runtime_generation).copied()?,
        )?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        process_instance_id: ProcessInstanceId::parse(
            required(&row.root_process_instance_id)?.clone(),
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    };
    let route_kind = required(&row.root_route_kind)?;
    let route_key = required(&row.root_route_key)?.clone();
    let route = match route_kind.as_str() {
        "static"
            if row.root_instance_id.is_none() && row.root_instance_manifest_digest.is_none() =>
        {
            RuntimeInteractionReceiptRouteV1::static_route(route_key)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?
        }
        "instance" => RuntimeInteractionReceiptRouteV1::instance_route(
            route_key,
            InstanceId::parse(required(&row.root_instance_id)?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    };
    let binding = decode_binding(
        scope,
        process,
        InteractionGatewayShardIdentityV1::parse(required(&row.root_gateway_shard_id)?.clone())
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        required(&row.root_attestation_digest)?.clone(),
        required(&row.root_serving_lease_epoch).copied()?,
        required(&row.root_serving_revision).copied()?,
        required(&row.root_gateway_owner_lease_epoch).copied()?,
        required(&row.root_gateway_owner_revision).copied()?,
        required(&row.root_route_controller_fencing_token).copied()?,
        required(&row.root_route_incarnation).copied()?,
        required(&row.root_runtime_build_revision)?.clone(),
        &route,
        required(&row.root_execution_ruleset_version).copied()?,
        required(&row.root_execution_ruleset_content_hash)?.clone(),
        row.root_instance_manifest_digest.clone(),
    )?;
    if binding.scope() != expected_route.scope()
        || binding.process_identity().target != expected_route.process_identity().target
        || binding.process_identity().runtime_generation
            != expected_route.process_identity().runtime_generation
        || binding.process_identity().process_instance_id
            != expected_route.process_identity().process_instance_id
        || binding.serving_identity().gateway_shard_identity()
            != expected_route.gateway_shard_identity()
        || binding.serving_identity().runtime_build_revision()
            != expected_route.runtime_build_revision()
        || binding.serving_identity().route_fencing_token() != expected_route.route_fencing_token()
        || binding.serving_identity().route_incarnation() != expected_route.route_incarnation()
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    let request_digest =
        InteractionRequestDigestV1::parse(bytes_to_lower_hex(required(&row.root_request_digest)?))
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let root = InteractionReceiptClaimCandidateV1::new(
        identity,
        InteractionExpectedRouteV1::from_authoritative(&binding),
        request_digest,
    )
    .bind_authoritative(binding)
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok((root, route))
}

fn decode_recovery_token(
    row: &ReceiptRecoverRowV1,
    root: &InteractionReceiptClaimRootV1,
) -> Result<EncryptedInteractionTokenV1, RuntimeInteractionPersistenceErrorV1> {
    let ciphertext = required(&row.token_ciphertext)?.clone();
    if !(17..=4_112).contains(&ciphertext.len()) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    let issued_at = required(&row.token_issued_at).copied()?;
    let expires_at = required(&row.token_expires_at).copied()?;
    validate_database_time(issued_at, false)?;
    validate_database_time(expires_at, false)?;
    let time = InteractionTokenEnvelopeTimeV1::new(
        unix_milliseconds(issued_at)?,
        unix_milliseconds(expires_at)?,
    )
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let suite_version = u16::try_from(required(&row.token_suite_version).copied()?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let aad_digest = InteractionTokenAuthenticatedDataDigestV1::parse(bytes_to_lower_hex(
        required(&row.token_aad_digest)?,
    ))
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let token = EncryptedInteractionTokenV1::from_persisted_parts(
        ciphertext,
        required(&row.token_nonce)?.clone(),
        required(&row.token_key_id)?.clone(),
        required(&row.token_encryption_suite)?.clone(),
        suite_version,
        time,
        aad_digest,
    )
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    validate_envelope_authenticated_data(root, &token)?;
    Ok(token)
}

fn ensure_recovery_payload_absent(
    row: &ReceiptRecoverRowV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let present = row.resulting_gateway_owner_lease_epoch.is_some()
        || row.resulting_gateway_owner_revision.is_some()
        || row.resulting_serving_lease_epoch.is_some()
        || row.resulting_serving_revision.is_some()
        || recovery_root_or_token_payload_present(row);
    if present {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn ensure_recovery_root_and_token_payload_absent(
    row: &ReceiptRecoverRowV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if recovery_root_or_token_payload_present(row) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn recovery_root_or_token_payload_present(row: &ReceiptRecoverRowV1) -> bool {
    row.root_tenant_id.is_some()
        || row.root_installation_id.is_some()
        || row.root_deployment_id.is_some()
        || row.root_attestation_digest.is_some()
        || row.root_guild_id.is_some()
        || row.root_ruleset_key.is_some()
        || row.root_target_version.is_some()
        || row.root_target_content_hash.is_some()
        || row.root_binding_revision.is_some()
        || row.root_binding_fingerprint.is_some()
        || row.root_runtime_generation.is_some()
        || row.root_process_instance_id.is_some()
        || row.root_serving_lease_epoch.is_some()
        || row.root_serving_revision.is_some()
        || row.root_gateway_shard_id.is_some()
        || row.root_gateway_owner_lease_epoch.is_some()
        || row.root_gateway_owner_revision.is_some()
        || row.root_route_controller_fencing_token.is_some()
        || row.root_route_incarnation.is_some()
        || row.root_runtime_build_revision.is_some()
        || row.root_route_kind.is_some()
        || row.root_route_key.is_some()
        || row.root_instance_id.is_some()
        || row.root_execution_ruleset_version.is_some()
        || row.root_execution_ruleset_content_hash.is_some()
        || row.root_instance_manifest_digest.is_some()
        || row.root_request_digest.is_some()
        || row.token_encryption_suite.is_some()
        || row.token_suite_version.is_some()
        || row.token_key_id.is_some()
        || row.token_nonce.is_some()
        || row.token_ciphertext.is_some()
        || row.token_aad_digest.is_some()
        || row.token_issued_at.is_some()
        || row.token_expires_at.is_some()
}

fn decode_scope(
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
) -> Result<InteractionProductScopeV1, RuntimeInteractionPersistenceErrorV1> {
    Ok(InteractionProductScopeV1::new(
        TenantId::parse(tenant_id)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        InstallationId::parse(installation_id)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        DeploymentId::parse(deployment_id)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    ))
}

fn decode_receipt_identity(
    application_id: String,
    interaction_id: String,
) -> Result<InteractionReceiptIdentityV1, RuntimeInteractionPersistenceErrorV1> {
    let application = application_id
        .parse::<u64>()
        .ok()
        .filter(|value| value.to_string() == application_id)
        .and_then(|value| DiscordApplicationIdV1::new(value).ok())
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let interaction = interaction_id
        .parse::<u64>()
        .ok()
        .filter(|value| value.to_string() == interaction_id)
        .and_then(|value| DiscordInteractionIdV1::new(value).ok())
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok(InteractionReceiptIdentityV1::new(application, interaction))
}

fn decode_guild_id(value: &str) -> Result<GuildId, RuntimeInteractionPersistenceErrorV1> {
    let guild_id = value
        .parse::<u64>()
        .ok()
        .filter(|guild_id| *guild_id > 0 && guild_id.to_string() == value)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok(GuildId(guild_id))
}

pub(crate) fn decode_state(
    value: &str,
) -> Result<InteractionReceiptStateV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "claimed" => Ok(InteractionReceiptStateV1::Claimed),
        "acknowledging" => Ok(InteractionReceiptStateV1::Acknowledging),
        "deferred" => Ok(InteractionReceiptStateV1::Deferred),
        "prepared" => Ok(InteractionReceiptStateV1::Prepared),
        "executing" => Ok(InteractionReceiptStateV1::Executing),
        "completed" => Ok(InteractionReceiptStateV1::Completed),
        "failed" => Ok(InteractionReceiptStateV1::Failed),
        "recovery_required" => Ok(InteractionReceiptStateV1::RecoveryRequired),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

pub(crate) fn digest_bytes(value: &str) -> Result<Vec<u8>, RuntimeInteractionPersistenceErrorV1> {
    if value.len() != 64 {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = lower_hex_value(pair[0])?;
            let low = lower_hex_value(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn bytes_to_lower_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn lower_hex_value(value: u8) -> Result<u8, RuntimeInteractionPersistenceErrorV1> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(RuntimeInteractionPersistenceErrorV1::InvalidInput),
    }
}

fn validate_lower_hex(value: &str) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn decode_ruleset_version(
    value: i64,
) -> Result<RuleSetVersionId, RuntimeInteractionPersistenceErrorV1> {
    u32::try_from(value)
        .ok()
        .and_then(|value| RuleSetVersionId::new(value).ok())
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_u64(value: i64) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0 && *value <= i64::MAX as u64)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_optional(value: Option<i64>) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    positive_u64(value.ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?)
}

fn required<T>(value: &Option<T>) -> Result<&T, RuntimeInteractionPersistenceErrorV1> {
    value
        .as_ref()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn duplicate(
    identity: InteractionReceiptIdentityV1,
    state: InteractionReceiptStateV1,
    head_revision: u64,
    claim_revision: u64,
    claim_expires_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
) -> RuntimeInteractionReceiptClaimDuplicateV1 {
    RuntimeInteractionReceiptClaimDuplicateV1::new(
        identity,
        state,
        head_revision,
        claim_revision,
        claim_expires_at,
        observed_database_now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::{
        RuntimeInteractionReceiptClaimLeaseV1, RuntimeInteractionReceiptOpaqueDigestV1,
        RuntimeInteractionReceiptRecoveryObservationKindV1,
        RuntimeInteractionReceiptRecoveryRequestV1,
    };
    use chrono::TimeZone;

    fn recovery_request(
        head_revision: u64,
        claim_revision: u64,
    ) -> RuntimeInteractionReceiptRecoveryRequestV1 {
        let identity = InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(1).unwrap(),
            DiscordInteractionIdV1::new(2).unwrap(),
        );
        let process = RuntimeProcessIdentityV1 {
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(3),
                ruleset_key: RuleSetKey::parse("study").unwrap(),
                version: RuleSetVersionId::FIRST,
                content_hash: RuleSetContentHash::parse_hex(&"a".repeat(64)).unwrap(),
                binding_revision: BindingRevision::new(1).unwrap(),
                binding_fingerprint: ResourceBindingFingerprint::parse(&"b".repeat(64)).unwrap(),
            },
            runtime_generation: RuntimeGeneration::new(1).unwrap(),
            process_instance_id: ProcessInstanceId::parse("process-2").unwrap(),
        };
        let expected_route = InteractionExpectedRouteV1::new(
            InteractionProductScopeV1::new(
                TenantId::parse("tenant-1").unwrap(),
                InstallationId::parse("installation-1").unwrap(),
                DeploymentId::parse("deployment-1").unwrap(),
            ),
            process,
            InteractionGatewayShardIdentityV1::parse("gateway-1").unwrap(),
            InteractionRuntimeBuildRevisionV1::parse("build-1").unwrap(),
            FencingToken::new(1).unwrap(),
            InteractionRouteIncarnationV1::new(1).unwrap(),
        )
        .unwrap();
        let key = RuntimeInteractionReceiptRecoveryScanKeyV1::new(
            Utc.timestamp_millis_opt(1_000).single().unwrap(),
            identity,
        )
        .unwrap();
        let candidate = RuntimeInteractionReceiptRecoveryCandidateV1::new(
            key,
            InteractionReceiptStateV1::Prepared,
            head_revision,
            claim_revision,
            Some(Utc.timestamp_millis_opt(3_000).single().unwrap()),
        );
        RuntimeInteractionReceiptRecoveryRequestV1::new(
            candidate,
            expected_route,
            RuntimeInteractionReceiptRecoveryObservationKindV1::MutationsReconciled,
            RuntimeInteractionReceiptOpaqueDigestV1::new([7; 32]),
            RuntimeInteractionReceiptClaimLeaseV1::default(),
        )
        .unwrap()
    }

    fn recovery_terminal_row(outcome_name: &str) -> ReceiptRecoverRowV1 {
        ReceiptRecoverRowV1 {
            outcome_name: outcome_name.to_string(),
            receipt_state: "recovery_required".to_string(),
            resulting_head_revision: 5,
            resulting_claim_revision: 3,
            resulting_claim_expires_at: Utc.timestamp_millis_opt(1_000).single().unwrap(),
            resulting_gateway_owner_lease_epoch: Some(2),
            resulting_gateway_owner_revision: Some(2),
            resulting_serving_lease_epoch: Some(2),
            resulting_serving_revision: Some(2),
            root_tenant_id: None,
            root_installation_id: None,
            root_deployment_id: None,
            root_attestation_digest: None,
            root_guild_id: None,
            root_ruleset_key: None,
            root_target_version: None,
            root_target_content_hash: None,
            root_binding_revision: None,
            root_binding_fingerprint: None,
            root_runtime_generation: None,
            root_process_instance_id: None,
            root_serving_lease_epoch: None,
            root_serving_revision: None,
            root_gateway_shard_id: None,
            root_gateway_owner_lease_epoch: None,
            root_gateway_owner_revision: None,
            root_route_controller_fencing_token: None,
            root_route_incarnation: None,
            root_runtime_build_revision: None,
            root_route_kind: None,
            root_route_key: None,
            root_instance_id: None,
            root_execution_ruleset_version: None,
            root_execution_ruleset_content_hash: None,
            root_instance_manifest_digest: None,
            root_request_digest: None,
            token_encryption_suite: None,
            token_suite_version: None,
            token_key_id: None,
            token_nonce: None,
            token_ciphertext: None,
            token_aad_digest: None,
            token_issued_at: None,
            token_expires_at: None,
            observed_database_now: Utc.timestamp_millis_opt(2_000).single().unwrap(),
        }
    }

    #[test]
    fn state_decoder_is_closed() {
        for state in [
            "claimed",
            "acknowledging",
            "deferred",
            "prepared",
            "executing",
            "completed",
            "failed",
            "recovery_required",
        ] {
            assert!(decode_state(state).is_ok());
        }
        assert_eq!(
            decode_state("complete"),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn scan_row_rejects_noncanonical_identity_and_future_claim() {
        let observed = Utc.timestamp_millis_opt(2_000).single().unwrap();
        let row = ReceiptRecoveryScanRowV1 {
            application_id: "01".to_string(),
            interaction_id: "2".to_string(),
            receipt_state: "claimed".to_string(),
            head_revision: 1,
            claim_revision: 1,
            claim_expires_at: Utc.timestamp_millis_opt(1_000).single().unwrap(),
            token_expires_at: None,
            through_claim_expires_at: Utc.timestamp_millis_opt(1_000).single().unwrap(),
            through_application_id: "1".to_string(),
            through_interaction_id: "2".to_string(),
            observed_database_now: observed,
        };
        assert!(row.decode().is_err());
        let row = ReceiptRecoveryScanRowV1 {
            application_id: "1".to_string(),
            interaction_id: "2".to_string(),
            receipt_state: "claimed".to_string(),
            head_revision: 1,
            claim_revision: 1,
            claim_expires_at: Utc.timestamp_millis_opt(3_000).single().unwrap(),
            token_expires_at: None,
            through_claim_expires_at: Utc.timestamp_millis_opt(3_000).single().unwrap(),
            through_application_id: "1".to_string(),
            through_interaction_id: "2".to_string(),
            observed_database_now: observed,
        };
        assert!(row.decode().is_err());
    }

    #[test]
    fn digest_codec_is_exact_lower_hex() {
        assert_eq!(digest_bytes(&"ab".repeat(32)).unwrap(), vec![0xab; 32]);
        assert!(digest_bytes(&"AB".repeat(32)).is_err());
        assert!(digest_bytes("00").is_err());
        assert_eq!(bytes_to_lower_hex(&[0, 15, 16, 255]), "000f10ff");
    }

    #[test]
    fn token_expiry_row_accepts_only_closed_outcomes() {
        let observed = Utc.timestamp_millis_opt(2_000).single().unwrap();
        let row = ReceiptTokenExpiryRowV1 {
            outcome_name: "interaction_token_expired".to_string(),
            receipt_state: "recovery_required".to_string(),
            resulting_head_revision: 4,
            resulting_claim_revision: 2,
            observed_database_now: observed,
        };
        assert_eq!(
            row.decode(3, 2).unwrap().disposition(),
            RuntimeInteractionReceiptTokenExpiryDispositionV1::RecoveryRequired
        );
        let row = ReceiptTokenExpiryRowV1 {
            outcome_name: "interaction_token_unavailable".to_string(),
            receipt_state: "recovery_required".to_string(),
            resulting_head_revision: 4,
            resulting_claim_revision: 2,
            observed_database_now: observed,
        };
        assert_eq!(
            row.decode(3, 2).unwrap().disposition(),
            RuntimeInteractionReceiptTokenExpiryDispositionV1::RecoveryRequired
        );
        let row = ReceiptTokenExpiryRowV1 {
            outcome_name: "surprise".to_string(),
            receipt_state: "claimed".to_string(),
            resulting_head_revision: 3,
            resulting_claim_revision: 2,
            observed_database_now: observed,
        };
        assert!(row.decode(3, 2).is_err());
        let row = ReceiptTokenExpiryRowV1 {
            outcome_name: "terminal_token_deleted".to_string(),
            receipt_state: "completed".to_string(),
            resulting_head_revision: 5,
            resulting_claim_revision: 2,
            observed_database_now: observed,
        };
        assert!(row.decode(4, 2).is_err());
        let row = ReceiptTokenExpiryRowV1 {
            outcome_name: "token_absent".to_string(),
            receipt_state: "recovery_required".to_string(),
            resulting_head_revision: 5,
            resulting_claim_revision: 2,
            observed_database_now: observed,
        };
        assert!(row.decode(3, 1).is_err());
        let row = ReceiptTokenExpiryRowV1 {
            outcome_name: "token_absent".to_string(),
            receipt_state: "recovery_required".to_string(),
            resulting_head_revision: 5,
            resulting_claim_revision: 2,
            observed_database_now: observed,
        };
        assert_eq!(
            row.decode(5, 2).unwrap().disposition(),
            RuntimeInteractionReceiptTokenExpiryDispositionV1::TokenAbsent
        );
    }

    #[test]
    fn expired_non_pristine_claim_maps_to_unsafe_resume_terminal() {
        let outcome = recovery_terminal_row("expired_claim_recovery_required")
            .decode(&recovery_request(4, 3))
            .unwrap();
        assert!(matches!(
            outcome,
            RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryRequired {
                reason: RuntimeInteractionReceiptRecoveryRequiredReasonV1::UnsafeToResume,
                ..
            }
        ));
        let mut malformed = recovery_terminal_row("expired_claim_recovery_required");
        malformed.root_request_digest = Some(vec![1; 32]);
        assert!(malformed.decode(&recovery_request(4, 3)).is_err());
    }
}
