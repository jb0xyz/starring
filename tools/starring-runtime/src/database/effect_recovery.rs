use std::fmt::{Debug, Formatter};
use std::num::NonZeroUsize;
use std::sync::Arc;

use automation_runtime::{
    OwnedSharedGatewayDispatchServicesV3, RuntimeInteractionEffectInstanceRegistrationIdentityV1,
    RuntimeInteractionEffectRecoveryCompensationDispositionV1,
    RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1,
    RuntimeInteractionEffectRecoveryCompensationRequestV1,
    RuntimeInteractionEffectRecoveryDefinitionV1, RuntimeInteractionEffectRecoveryExecutorV1,
    RuntimeInteractionEffectRecoveryObservationDispositionV1,
    RuntimeInteractionEffectRecoveryObservationRequestV1,
    RuntimeInteractionEffectRecoveryRequiredV1, RuntimeInteractionEffectRecoveryRouteBlockV1,
};
use automation_runtime_convergence::{ProcessInstanceId, RuntimeProcessIdentityV1};
use automation_runtime_interaction::{
    InteractionEffectCompensationObservationOutcomeV1, InteractionEffectCompensationOutcomeV1,
    InteractionEffectObservationOutcomeV1, InteractionEffectStateV1, InteractionExpectedRouteV1,
    InteractionGatewayShardIdentityV1, InteractionReceiptClaimRootV1,
    InteractionRuntimeBuildRevisionV1, XChaCha20Poly1305InteractionTokenCipherV1,
};
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionEffectCompensationClaimV1,
    RuntimeInteractionEffectCompensationFinishRequestV1,
    RuntimeInteractionEffectCompensationIntendOutcomeV1,
    RuntimeInteractionEffectCompensationIntendRequestV1,
    RuntimeInteractionEffectMutationDispositionV1, RuntimeInteractionEffectReconcileRequestV1,
    RuntimeInteractionEffectReconciliationOutcomeV1, RuntimeInteractionEffectRecoveryBindingV1,
    RuntimeInteractionEffectRecoveryBlockReasonV1, RuntimeInteractionEffectRecoveryCandidateV1,
    RuntimeInteractionEffectRecoveryClaimOutcomeV1, RuntimeInteractionEffectRecoveryClaimRequestV1,
    RuntimeInteractionEffectRecoveryClaimV1, RuntimeInteractionEffectRecoveryPathV1,
    RuntimeInteractionEffectRecoveryScanCursorV1, RuntimeInteractionEffectResponseTailCandidateV1,
    RuntimeInteractionEffectResponseTailClaimOutcomeV1,
    RuntimeInteractionEffectResponseTailClaimRequestV1,
    RuntimeInteractionEffectResponseTailClaimV1,
    RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
    RuntimeInteractionEffectResponseTailFinalizeRequestV1,
    RuntimeInteractionEffectResponseTailRecoveryModeV1,
    RuntimeInteractionEffectResponseTailScanCursorV1, RuntimeInteractionPersistenceErrorV1,
    RuntimeInteractionReceiptClaimLeaseV1, RuntimeInteractionReceiptOpaqueDigestV1,
    MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
};

use crate::interaction_effect_recovery_supervisor::{
    RuntimeInteractionEffectRecoveryCandidateFutureV1,
    RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionEffectRecoveryScanFutureV1,
    RuntimeInteractionEffectRecoveryScanPageV1, RuntimeInteractionEffectRecoveryScanRequestV1,
    RuntimeInteractionEffectRecoverySupervisorPortV1,
};

use super::RuntimeInteractionDispatchDatabasePortV1;

const RESPONSE_TAIL_MINIMUM_TOKEN_BUDGET_MILLISECONDS_V1: u64 = 12_000;
const RESPONSE_TAIL_CLOSE_KNOWN_DIGEST_V1: [u8; 32] = [
    0x25, 0xd6, 0xf9, 0x86, 0x6e, 0xf4, 0x82, 0x08, 0x3a, 0x8b, 0x75, 0x1b, 0x31, 0x2f, 0xf6, 0xe9,
    0x5b, 0x22, 0xbd, 0x54, 0xfe, 0x09, 0x9f, 0x8e, 0x46, 0xc3, 0xc0, 0x01, 0x2c, 0x5a, 0x99, 0xdf,
];
const RESPONSE_TAIL_TOKEN_UNRECOVERABLE_DIGEST_V1: [u8; 32] = [
    0xda, 0xda, 0x46, 0xf1, 0x4d, 0xe5, 0x70, 0x88, 0x58, 0x5e, 0x42, 0xc0, 0x84, 0x84, 0x5b, 0x1f,
    0xa1, 0xeb, 0x71, 0x13, 0x9f, 0xe9, 0x32, 0x7f, 0xfd, 0x5d, 0x95, 0x8c, 0x18, 0x4b, 0xc7, 0x83,
];

#[derive(Default)]
pub(super) struct RuntimeInteractionEffectRecoveryDatabaseCursorV1 {
    effects: Option<RuntimeInteractionEffectRecoveryScanCursorV1>,
    effects_exhausted: bool,
    response_tails: Option<RuntimeInteractionEffectResponseTailScanCursorV1>,
    response_tails_exhausted: bool,
    prefer_response_tail: bool,
}

pub(super) enum RuntimeInteractionEffectRecoveryDatabaseCandidateV1 {
    Effect(Box<RuntimeInteractionEffectRecoveryCandidateV1>),
    ResponseTail(Box<RuntimeInteractionEffectResponseTailCandidateV1>),
}

pub(super) struct RuntimeInteractionEffectRecoveryDatabasePortV1 {
    inner: Arc<OwnedSharedGatewayDispatchServicesV3<PostgresRuntimeInteractionV1>>,
    store: PostgresRuntimeInteractionV1,
    cipher: XChaCha20Poly1305InteractionTokenCipherV1,
    gateway_shard_identity: InteractionGatewayShardIdentityV1,
    runtime_build_revision: InteractionRuntimeBuildRevisionV1,
    process_instance_id: ProcessInstanceId,
}

impl RuntimeInteractionEffectRecoveryDatabasePortV1 {
    pub(super) fn from_dispatch_port_v1(port: &RuntimeInteractionDispatchDatabasePortV1) -> Self {
        Self {
            inner: Arc::clone(&port.inner),
            store: port.receipt.store.clone(),
            cipher: port.receipt.cipher.clone(),
            gateway_shard_identity: port.gateway_shard_identity.clone(),
            runtime_build_revision: port.runtime_build_revision.clone(),
            process_instance_id: port.process_instance_id.clone(),
        }
    }

    fn expected_route_v1(
        &self,
        claim_root: &InteractionReceiptClaimRootV1,
    ) -> Result<InteractionExpectedRouteV1, RuntimeInteractionPersistenceErrorV1> {
        let authoritative = claim_root.route();
        let source_process = authoritative.process_identity();
        InteractionExpectedRouteV1::new(
            authoritative.scope().clone(),
            RuntimeProcessIdentityV1 {
                target: source_process.target.clone(),
                runtime_generation: source_process.runtime_generation,
                process_instance_id: self.process_instance_id.clone(),
            },
            self.gateway_shard_identity.clone(),
            self.runtime_build_revision.clone(),
            authoritative.serving_identity().route_fencing_token(),
            authoritative.serving_identity().route_incarnation(),
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }

    fn recovery_definition_v1(
        candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
        recovered: &RuntimeInteractionEffectRecoveryBindingV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDefinitionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let process = candidate.origin().claim_root().route().process_identity();
        let ruleset_key = process.target.ruleset_key.clone();
        let registration_identity = match recovered.binding().target() {
            automation_runtime_interaction::InteractionEffectRecoveryTargetV1::RegisterInstance {
                kind,
                ..
            } => Some(
                RuntimeInteractionEffectInstanceRegistrationIdentityV1::from_ruleset_version_v1(
                ruleset_key.clone(),
                process.target.version,
                kind.clone(),
                candidate.origin().actor_user_id(),
                candidate
                    .resolved_instance_manifest_digest()
                    .cloned()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                ),
            ),
            _ => None,
        };
        RuntimeInteractionEffectRecoveryDefinitionV1::new(
            recovered.binding().clone(),
            recovered.instance_id().cloned(),
            registration_identity,
            ruleset_key,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }

    async fn recover_effect_v1(
        &self,
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let expected_route = self.expected_route_v1(candidate.origin().claim_root())?;
        let recovered = candidate.strict_recovery_binding_v1()?;
        match candidate.recovery_path() {
            RuntimeInteractionEffectRecoveryPathV1::Observation => {
                self.recover_observation_v1(candidate, expected_route, recovered)
                    .await
            }
            RuntimeInteractionEffectRecoveryPathV1::Compensation => {
                self.recover_compensation_v1(candidate, expected_route, recovered)
                    .await
            }
            RuntimeInteractionEffectRecoveryPathV1::ResponseTail => {
                Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    async fn recover_response_tail_v1(
        &self,
        candidate: RuntimeInteractionEffectResponseTailCandidateV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let expected_route = self.expected_route_v1(candidate.origin().claim_root())?;
        match candidate.recovery_mode() {
            RuntimeInteractionEffectResponseTailRecoveryModeV1::CloseKnown => {
                let request = RuntimeInteractionEffectResponseTailFinalizeRequestV1::close_known(
                    &candidate,
                    expected_route,
                    RuntimeInteractionReceiptOpaqueDigestV1::new(
                        RESPONSE_TAIL_CLOSE_KNOWN_DIGEST_V1,
                    ),
                )?;
                let outcome = self
                    .store
                    .finalize_interaction_response_tail_v1(request)
                    .await?;
                Ok(response_tail_finalize_disposition_v1(outcome.disposition()))
            }
            RuntimeInteractionEffectResponseTailRecoveryModeV1::Observe => {
                self.observe_response_tail_v1(candidate, expected_route)
                    .await
            }
        }
    }

    async fn observe_response_tail_v1(
        &self,
        candidate: RuntimeInteractionEffectResponseTailCandidateV1,
        expected_route: InteractionExpectedRouteV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let binding = candidate.strict_recovery_binding_v1()?;
        let ruleset_key = candidate
            .origin()
            .claim_root()
            .route()
            .process_identity()
            .target
            .ruleset_key
            .clone();
        let request = RuntimeInteractionEffectResponseTailClaimRequestV1::new(
            candidate,
            expected_route,
            RuntimeInteractionReceiptClaimLeaseV1::default(),
            RuntimeInteractionReceiptOpaqueDigestV1::new(
                RESPONSE_TAIL_TOKEN_UNRECOVERABLE_DIGEST_V1,
            ),
        )?;
        let claim = match self
            .store
            .claim_interaction_response_tail_v1(request)
            .await?
        {
            RuntimeInteractionEffectResponseTailClaimOutcomeV1::Claimed(claim) => claim,
            RuntimeInteractionEffectResponseTailClaimOutcomeV1::Unrecoverable(_) => {
                return Ok(RuntimeInteractionEffectRecoveryDispositionV1::Reconciled)
            }
        };
        let database_now = u64::try_from(claim.observed_database_now().timestamp_millis())
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if !response_token_has_minimum_budget_v1(
            claim
                .encrypted_token()
                .time()
                .expires_at_unix_milliseconds(),
            database_now,
        ) {
            return self.finalize_response_token_unrecoverable_v1(&claim).await;
        }
        let token = match self.cipher.decrypt(
            claim.encrypted_token(),
            claim.candidate().origin().claim_root(),
            database_now,
        ) {
            Ok(token) => token,
            Err(_) => return self.finalize_response_token_unrecoverable_v1(&claim).await,
        };
        let definition =
            RuntimeInteractionEffectRecoveryDefinitionV1::new(binding, None, None, ruleset_key)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let discord = self.inner.discord_effects_v1();
        let adapter = discord.adapter_v1(definition.ruleset_key());
        let response = self.inner.original_response_observer_v1();
        let internal = self.inner.internal_effect_recovery_v1();
        let executor = RuntimeInteractionEffectRecoveryExecutorV1::new(
            &adapter,
            &response,
            &internal,
            discord.bot_user_v1(),
        );
        let request =
            RuntimeInteractionEffectRecoveryObservationRequestV1::new(definition, Some(token))
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        match executor.observe_v1(request).await {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(outcome) => {
                let request =
                    RuntimeInteractionEffectResponseTailFinalizeRequestV1::from_observation(
                        &claim,
                        outcome,
                        MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
                    )?;
                let outcome = self
                    .store
                    .finalize_interaction_response_tail_v1(request)
                    .await?;
                Ok(response_tail_finalize_disposition_v1(outcome.disposition()))
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(_) => {
                Ok(RuntimeInteractionEffectRecoveryDispositionV1::Deferred)
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(reason) => {
                self.persist_response_tail_recovery_block_v1(
                    &claim,
                    recovery_required_block_reason_v1(reason),
                )
                .await
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RouteBlocked(reason) => {
                self.persist_response_tail_recovery_block_v1(
                    &claim,
                    recovery_route_block_reason_v1(reason),
                )
                .await
            }
        }
    }

    async fn finalize_response_token_unrecoverable_v1(
        &self,
        claim: &automation_runtime_interaction_postgres::RuntimeInteractionEffectResponseTailClaimV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let request = RuntimeInteractionEffectResponseTailFinalizeRequestV1::token_unrecoverable(
            claim,
            RuntimeInteractionReceiptOpaqueDigestV1::new(
                RESPONSE_TAIL_TOKEN_UNRECOVERABLE_DIGEST_V1,
            ),
        );
        let outcome = self
            .store
            .finalize_interaction_response_tail_v1(request)
            .await?;
        Ok(response_tail_finalize_disposition_v1(outcome.disposition()))
    }

    async fn persist_recovery_block_v1(
        &self,
        claim: &RuntimeInteractionEffectRecoveryClaimV1,
        reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let request = RuntimeInteractionEffectReconcileRequestV1::recovery_blocked(claim, reason)?;
        let checkpoint = self.store.reconcile_interaction_effect_v1(request).await?;
        durable_effect_recovery_block_disposition_v1(checkpoint.disposition(), checkpoint.state())
    }

    async fn persist_compensation_block_v1(
        &self,
        claim: &RuntimeInteractionEffectCompensationClaimV1,
        reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let request =
            RuntimeInteractionEffectReconcileRequestV1::compensation_blocked(claim, reason)?;
        let checkpoint = self.store.reconcile_interaction_effect_v1(request).await?;
        durable_effect_recovery_block_disposition_v1(checkpoint.disposition(), checkpoint.state())
    }

    async fn persist_response_tail_recovery_block_v1(
        &self,
        claim: &RuntimeInteractionEffectResponseTailClaimV1,
        reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let request =
            RuntimeInteractionEffectResponseTailFinalizeRequestV1::recovery_blocked(claim, reason)?;
        let outcome = self
            .store
            .finalize_interaction_response_tail_v1(request)
            .await?;
        durable_response_tail_recovery_block_disposition_v1(
            outcome.disposition(),
            outcome.effect_state(),
        )
    }

    async fn scan_recoverable_database_v1(
        &self,
        mut cursor: RuntimeInteractionEffectRecoveryDatabaseCursorV1,
        limit: NonZeroUsize,
    ) -> Result<
        RuntimeInteractionEffectRecoveryScanPageV1<
            RuntimeInteractionEffectRecoveryDatabaseCursorV1,
            RuntimeInteractionEffectRecoveryDatabaseCandidateV1,
        >,
        RuntimeInteractionPersistenceErrorV1,
    > {
        let effects_active = !cursor.effects_exhausted;
        let response_tails_active = !cursor.response_tails_exhausted;
        let total = limit.get();
        let (effects_limit, response_tails_limit) = recovery_scan_budgets_v1(
            effects_active,
            response_tails_active,
            total,
            cursor.prefer_response_tail,
        );
        cursor.prefer_response_tail = !cursor.prefer_response_tail;
        let mut candidates = Vec::with_capacity(total);
        let mut effects_scanned = false;
        let mut response_tails_scanned = false;
        if let Some(limit) = NonZeroUsize::new(effects_limit) {
            effects_scanned = true;
            self.scan_effects_into_v1(&mut cursor, limit, &mut candidates)
                .await?;
        }
        if let Some(limit) = NonZeroUsize::new(response_tails_limit) {
            response_tails_scanned = true;
            self.scan_response_tails_into_v1(&mut cursor, limit, &mut candidates)
                .await?;
        }
        if candidates.is_empty() && !cursor.effects_exhausted && !effects_scanned {
            self.scan_effects_into_v1(&mut cursor, limit, &mut candidates)
                .await?;
        }
        if candidates.is_empty() && !cursor.response_tails_exhausted && !response_tails_scanned {
            self.scan_response_tails_into_v1(&mut cursor, limit, &mut candidates)
                .await?;
        }
        let exhausted = cursor.effects_exhausted && cursor.response_tails_exhausted;
        RuntimeInteractionEffectRecoveryScanPageV1::new(
            candidates,
            (!exhausted).then_some(cursor),
            exhausted,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }

    async fn scan_effects_into_v1(
        &self,
        cursor: &mut RuntimeInteractionEffectRecoveryDatabaseCursorV1,
        limit: NonZeroUsize,
        candidates: &mut Vec<RuntimeInteractionEffectRecoveryDatabaseCandidateV1>,
    ) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
        let page = self
            .store
            .scan_recoverable_interaction_effects_v1(
                cursor.effects.take().unwrap_or_default(),
                limit,
            )
            .await?;
        candidates.extend(
            page.candidates()
                .iter()
                .cloned()
                .map(Box::new)
                .map(RuntimeInteractionEffectRecoveryDatabaseCandidateV1::Effect),
        );
        cursor.effects_exhausted = page.exhausted();
        cursor.effects = page.next_cursor();
        Ok(())
    }

    async fn scan_response_tails_into_v1(
        &self,
        cursor: &mut RuntimeInteractionEffectRecoveryDatabaseCursorV1,
        limit: NonZeroUsize,
        candidates: &mut Vec<RuntimeInteractionEffectRecoveryDatabaseCandidateV1>,
    ) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
        let source_cursor = cursor.response_tails.take().unwrap_or_default();
        let page = self
            .store
            .scan_recoverable_interaction_response_tails_v1(&source_cursor, limit)
            .await?;
        candidates.extend(
            page.candidates()
                .iter()
                .cloned()
                .map(Box::new)
                .map(RuntimeInteractionEffectRecoveryDatabaseCandidateV1::ResponseTail),
        );
        cursor.response_tails_exhausted = page.exhausted();
        cursor.response_tails = page.next_cursor();
        Ok(())
    }

    async fn recover_observation_v1(
        &self,
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        recovered: RuntimeInteractionEffectRecoveryBindingV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let definition = Self::recovery_definition_v1(&candidate, &recovered)?;
        let claim = self
            .store
            .claim_interaction_effect_recovery_v1(
                RuntimeInteractionEffectRecoveryClaimRequestV1::new(
                    candidate,
                    expected_route,
                    RuntimeInteractionReceiptClaimLeaseV1::default(),
                )?,
            )
            .await?;
        let claim = match claim {
            RuntimeInteractionEffectRecoveryClaimOutcomeV1::Claimed(claim) => claim,
            RuntimeInteractionEffectRecoveryClaimOutcomeV1::RecoveryBlocked(_) => {
                return Ok(RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked);
            }
        };
        let discord = self.inner.discord_effects_v1();
        let adapter = discord.adapter_v1(definition.ruleset_key());
        let response = self.inner.original_response_observer_v1();
        let internal = self.inner.internal_effect_recovery_v1();
        let executor = RuntimeInteractionEffectRecoveryExecutorV1::new(
            &adapter,
            &response,
            &internal,
            discord.bot_user_v1(),
        );
        let request = RuntimeInteractionEffectRecoveryObservationRequestV1::new(definition, None)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        match executor.observe_v1(request).await {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(outcome) => {
                let disposition = observation_disposition_v1(&outcome);
                let request = RuntimeInteractionEffectReconcileRequestV1::new_recovery_bound(
                    &claim,
                    &recovered,
                    RuntimeInteractionEffectReconciliationOutcomeV1::Observation(outcome),
                    MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
                )?;
                self.store.reconcile_interaction_effect_v1(request).await?;
                Ok(disposition)
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(_) => {
                Ok(RuntimeInteractionEffectRecoveryDispositionV1::Deferred)
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(reason) => {
                self.persist_recovery_block_v1(&claim, recovery_required_block_reason_v1(reason))
                    .await
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RouteBlocked(reason) => {
                self.persist_recovery_block_v1(&claim, recovery_route_block_reason_v1(reason))
                    .await
            }
        }
    }

    async fn recover_compensation_v1(
        &self,
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        recovered: RuntimeInteractionEffectRecoveryBindingV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        match compensation_recovery_stage_v1(candidate.state())? {
            RuntimeInteractionEffectCompensationRecoveryStageV1::Begin => {
                self.begin_compensation_v1(candidate, expected_route, recovered)
                    .await
            }
            RuntimeInteractionEffectCompensationRecoveryStageV1::Observe => {
                self.observe_compensation_v1(candidate, expected_route, recovered)
                    .await
            }
        }
    }

    async fn begin_compensation_v1(
        &self,
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        recovered: RuntimeInteractionEffectRecoveryBindingV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let definition = Self::recovery_definition_v1(&candidate, &recovered)?;
        let successful_output = recovered
            .successful_output()
            .cloned()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let claim = self
            .store
            .intend_interaction_effect_compensation_v1(
                RuntimeInteractionEffectCompensationIntendRequestV1::new_recovery_bound(
                    candidate,
                    expected_route,
                    &recovered,
                    MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
                )?,
            )
            .await?;
        let claim = match claim {
            RuntimeInteractionEffectCompensationIntendOutcomeV1::Claimed(claim) => claim,
            RuntimeInteractionEffectCompensationIntendOutcomeV1::RecoveryBlocked(_) => {
                return Ok(RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked);
            }
        };
        if !compensation_intent_authorizes_external_call_v1(claim.disposition()) {
            return Ok(RuntimeInteractionEffectRecoveryDispositionV1::Deferred);
        }
        let discord = self.inner.discord_effects_v1();
        let adapter = discord.adapter_v1(definition.ruleset_key());
        let response = self.inner.original_response_observer_v1();
        let internal = self.inner.internal_effect_recovery_v1();
        let executor = RuntimeInteractionEffectRecoveryExecutorV1::new(
            &adapter,
            &response,
            &internal,
            discord.bot_user_v1(),
        );
        let request = RuntimeInteractionEffectRecoveryCompensationRequestV1::new(
            definition,
            successful_output,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        match executor.compensate_v1(request).await {
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(outcome) => {
                let disposition = compensation_disposition_v1(&outcome);
                let request =
                    RuntimeInteractionEffectCompensationFinishRequestV1::new_recovery_bound(
                        &claim,
                        &recovered,
                        outcome,
                        MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
                    )?;
                self.store
                    .finish_interaction_effect_compensation_v1(request)
                    .await?;
                Ok(disposition)
            }
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::Deferred(_) => {
                Ok(RuntimeInteractionEffectRecoveryDispositionV1::Deferred)
            }
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::RecoveryRequired(reason) => {
                self.persist_compensation_block_v1(
                    &claim,
                    recovery_required_block_reason_v1(reason),
                )
                .await
            }
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::RouteBlocked(reason) => {
                self.persist_compensation_block_v1(&claim, recovery_route_block_reason_v1(reason))
                    .await
            }
        }
    }

    async fn observe_compensation_v1(
        &self,
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        recovered: RuntimeInteractionEffectRecoveryBindingV1,
    ) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let definition = Self::recovery_definition_v1(&candidate, &recovered)?;
        let successful_output = recovered
            .successful_output()
            .cloned()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let claim = self
            .store
            .claim_interaction_effect_recovery_v1(
                RuntimeInteractionEffectRecoveryClaimRequestV1::new(
                    candidate,
                    expected_route,
                    RuntimeInteractionReceiptClaimLeaseV1::default(),
                )?,
            )
            .await?;
        let claim = match claim {
            RuntimeInteractionEffectRecoveryClaimOutcomeV1::Claimed(claim) => claim,
            RuntimeInteractionEffectRecoveryClaimOutcomeV1::RecoveryBlocked(_) => {
                return Ok(RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked);
            }
        };
        let discord = self.inner.discord_effects_v1();
        let adapter = discord.adapter_v1(definition.ruleset_key());
        let response = self.inner.original_response_observer_v1();
        let internal = self.inner.internal_effect_recovery_v1();
        let executor = RuntimeInteractionEffectRecoveryExecutorV1::new(
            &adapter,
            &response,
            &internal,
            discord.bot_user_v1(),
        );
        let request = RuntimeInteractionEffectRecoveryCompensationRequestV1::new(
            definition,
            successful_output,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        match executor.observe_compensation_v1(request).await {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                outcome,
            ) => {
                let disposition = compensation_observation_disposition_v1(&outcome);
                let request = RuntimeInteractionEffectReconcileRequestV1::new_recovery_bound(
                    &claim,
                    &recovered,
                    RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                        outcome,
                    ),
                    MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
                )?;
                self.store.reconcile_interaction_effect_v1(request).await?;
                Ok(disposition)
            }
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Deferred(_) => {
                Ok(RuntimeInteractionEffectRecoveryDispositionV1::Deferred)
            }
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RecoveryRequired(
                reason,
            ) => {
                self.persist_recovery_block_v1(
                    &claim,
                    recovery_required_block_reason_v1(reason),
                )
                .await
            }
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RouteBlocked(
                reason,
            ) => {
                self.persist_recovery_block_v1(
                    &claim,
                    recovery_route_block_reason_v1(reason),
                )
                .await
            }
        }
    }
}

impl RuntimeInteractionEffectRecoverySupervisorPortV1
    for RuntimeInteractionEffectRecoveryDatabasePortV1
{
    type Cursor = RuntimeInteractionEffectRecoveryDatabaseCursorV1;
    type Candidate = RuntimeInteractionEffectRecoveryDatabaseCandidateV1;
    type Error = RuntimeInteractionPersistenceErrorV1;

    fn scan_recoverable_v1(
        self: Arc<Self>,
        request: RuntimeInteractionEffectRecoveryScanRequestV1<Self::Cursor>,
    ) -> RuntimeInteractionEffectRecoveryScanFutureV1<Self::Cursor, Self::Candidate, Self::Error>
    {
        Box::pin(async move {
            let (cursor, limit) = request.into_parts();
            self.scan_recoverable_database_v1(cursor.unwrap_or_default(), limit)
                .await
        })
    }

    fn recover_candidate_v1(
        self: Arc<Self>,
        candidate: Self::Candidate,
    ) -> RuntimeInteractionEffectRecoveryCandidateFutureV1<Self::Error> {
        Box::pin(async move {
            match candidate {
                RuntimeInteractionEffectRecoveryDatabaseCandidateV1::Effect(candidate) => {
                    self.recover_effect_v1(*candidate).await
                }
                RuntimeInteractionEffectRecoveryDatabaseCandidateV1::ResponseTail(candidate) => {
                    self.recover_response_tail_v1(*candidate).await
                }
            }
        })
    }
}

impl Debug for RuntimeInteractionEffectRecoveryDatabasePortV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryDatabasePortV1(<redacted>)")
    }
}

fn observation_disposition_v1(
    outcome: &InteractionEffectObservationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryDispositionV1 {
    match outcome {
        InteractionEffectObservationOutcomeV1::ExactMatch { .. }
        | InteractionEffectObservationOutcomeV1::ExactAbsence { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::Reconciled
        }
        InteractionEffectObservationOutcomeV1::Pending { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::Deferred
        }
        InteractionEffectObservationOutcomeV1::Conflict { .. }
        | InteractionEffectObservationOutcomeV1::Unsupported { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked
        }
    }
}

fn compensation_disposition_v1(
    outcome: &InteractionEffectCompensationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryDispositionV1 {
    match outcome {
        InteractionEffectCompensationOutcomeV1::Succeeded { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::Compensated
        }
        InteractionEffectCompensationOutcomeV1::Indeterminate(_) => {
            RuntimeInteractionEffectRecoveryDispositionV1::Deferred
        }
        InteractionEffectCompensationOutcomeV1::KnownFailed(_) => {
            RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked
        }
    }
}

fn compensation_observation_disposition_v1(
    outcome: &InteractionEffectCompensationObservationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryDispositionV1 {
    match outcome {
        InteractionEffectCompensationObservationOutcomeV1::Restored { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::Compensated
        }
        InteractionEffectCompensationObservationOutcomeV1::Pending { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::Deferred
        }
        InteractionEffectCompensationObservationOutcomeV1::Conflict { .. }
        | InteractionEffectCompensationObservationOutcomeV1::Unsupported { .. } => {
            RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked
        }
    }
}

fn response_tail_finalize_disposition_v1(
    disposition: RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
) -> RuntimeInteractionEffectRecoveryDispositionV1 {
    match disposition {
        RuntimeInteractionEffectResponseTailFinalizeDispositionV1::Deferred => {
            RuntimeInteractionEffectRecoveryDispositionV1::Deferred
        }
        RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted
        | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
        | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnrecoverable
        | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ExactReplay => {
            RuntimeInteractionEffectRecoveryDispositionV1::Reconciled
        }
    }
}

fn recovery_required_block_reason_v1(
    reason: RuntimeInteractionEffectRecoveryRequiredV1,
) -> RuntimeInteractionEffectRecoveryBlockReasonV1 {
    match reason {
        RuntimeInteractionEffectRecoveryRequiredV1::DiscordReadRejected => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::DiscordReadRejected
        }
        RuntimeInteractionEffectRecoveryRequiredV1::ResponseTokenUnavailable => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::ResponseTokenUnavailable
        }
        RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::ObservationProtocol
        }
        RuntimeInteractionEffectRecoveryRequiredV1::CompensationConflict => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::CompensationConflict
        }
        RuntimeInteractionEffectRecoveryRequiredV1::CompensationUnsupported => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::CompensationUnsupported
        }
        RuntimeInteractionEffectRecoveryRequiredV1::NonCompensable => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::NonCompensable
        }
        RuntimeInteractionEffectRecoveryRequiredV1::InternalConflict => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::InternalConflict
        }
    }
}

fn recovery_route_block_reason_v1(
    reason: RuntimeInteractionEffectRecoveryRouteBlockV1,
) -> RuntimeInteractionEffectRecoveryBlockReasonV1 {
    match reason {
        RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::DiscordForbidden
        }
        RuntimeInteractionEffectRecoveryRouteBlockV1::InternalAuthority => {
            RuntimeInteractionEffectRecoveryBlockReasonV1::InternalAuthority
        }
    }
}

fn durable_effect_recovery_block_disposition_v1(
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    state: InteractionEffectStateV1,
) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1> {
    if state != InteractionEffectStateV1::RecoveryRequired {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    match disposition {
        RuntimeInteractionEffectMutationDispositionV1::Applied
        | RuntimeInteractionEffectMutationDispositionV1::ExactReplay => {
            Ok(RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked)
        }
    }
}

fn durable_response_tail_recovery_block_disposition_v1(
    disposition: RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
    state: InteractionEffectStateV1,
) -> Result<RuntimeInteractionEffectRecoveryDispositionV1, RuntimeInteractionPersistenceErrorV1> {
    if state != InteractionEffectStateV1::RecoveryRequired {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    match disposition {
        RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnrecoverable
        | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ExactReplay => {
            Ok(RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked)
        }
        RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted
        | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed
        | RuntimeInteractionEffectResponseTailFinalizeDispositionV1::Deferred => {
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        }
    }
}

fn recovery_scan_budgets_v1(
    effects_active: bool,
    response_tails_active: bool,
    total: usize,
    prefer_response_tail: bool,
) -> (usize, usize) {
    match (
        effects_active,
        response_tails_active,
        total,
        prefer_response_tail,
    ) {
        (true, true, 1, false) => (1, 0),
        (true, true, 1, true) => (0, 1),
        (true, true, _, _) => (total / 2, total - total / 2),
        (true, false, _, _) => (total, 0),
        (false, true, _, _) => (0, total),
        (false, false, _, _) => (0, 0),
    }
}

fn response_token_has_minimum_budget_v1(expires_at: u64, database_now: u64) -> bool {
    database_now
        .checked_add(RESPONSE_TAIL_MINIMUM_TOKEN_BUDGET_MILLISECONDS_V1)
        .is_some_and(|minimum_expiry| expires_at > minimum_expiry)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeInteractionEffectCompensationRecoveryStageV1 {
    Begin,
    Observe,
}

fn compensation_recovery_stage_v1(
    state: InteractionEffectStateV1,
) -> Result<RuntimeInteractionEffectCompensationRecoveryStageV1, RuntimeInteractionPersistenceErrorV1>
{
    match state {
        InteractionEffectStateV1::KnownSucceeded
        | InteractionEffectStateV1::ReconciledSucceeded => {
            Ok(RuntimeInteractionEffectCompensationRecoveryStageV1::Begin)
        }
        InteractionEffectStateV1::CompensationIntended
        | InteractionEffectStateV1::CompensationIndeterminate
        | InteractionEffectStateV1::CompensationObserving
        | InteractionEffectStateV1::CompensationObservationPending => {
            Ok(RuntimeInteractionEffectCompensationRecoveryStageV1::Observe)
        }
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn compensation_intent_authorizes_external_call_v1(
    disposition: RuntimeInteractionEffectMutationDispositionV1,
) -> bool {
    disposition == RuntimeInteractionEffectMutationDispositionV1::Applied
}

#[cfg(test)]
mod tests {
    use super::{
        compensation_intent_authorizes_external_call_v1, compensation_recovery_stage_v1,
        durable_effect_recovery_block_disposition_v1,
        durable_response_tail_recovery_block_disposition_v1, recovery_required_block_reason_v1,
        recovery_route_block_reason_v1, recovery_scan_budgets_v1,
        response_token_has_minimum_budget_v1, RuntimeInteractionEffectCompensationRecoveryStageV1,
        RuntimeInteractionEffectRecoveryBlockReasonV1,
        RuntimeInteractionEffectRecoveryDispositionV1,
        RuntimeInteractionEffectResponseTailFinalizeDispositionV1,
        RESPONSE_TAIL_MINIMUM_TOKEN_BUDGET_MILLISECONDS_V1,
    };
    use automation_runtime::{
        RuntimeInteractionEffectRecoveryRequiredV1, RuntimeInteractionEffectRecoveryRouteBlockV1,
    };
    use automation_runtime_interaction::InteractionEffectStateV1;
    use automation_runtime_interaction_postgres::RuntimeInteractionEffectMutationDispositionV1;

    #[test]
    fn combined_scan_budget_is_fair_and_bounded() {
        assert_eq!(recovery_scan_budgets_v1(true, true, 64, false), (32, 32));
        assert_eq!(recovery_scan_budgets_v1(true, true, 63, false), (31, 32));
        assert_eq!(recovery_scan_budgets_v1(true, false, 64, true), (64, 0));
        assert_eq!(recovery_scan_budgets_v1(false, true, 64, false), (0, 64));
        assert_eq!(recovery_scan_budgets_v1(false, false, 64, false), (0, 0));
        assert_eq!(recovery_scan_budgets_v1(true, true, 1, false), (1, 0));
        assert_eq!(recovery_scan_budgets_v1(true, true, 1, true), (0, 1));
    }

    #[test]
    fn response_token_budget_rejects_boundary_and_overflow() {
        let now = 1_000_000;
        assert!(!response_token_has_minimum_budget_v1(
            now + RESPONSE_TAIL_MINIMUM_TOKEN_BUDGET_MILLISECONDS_V1,
            now,
        ));
        assert!(response_token_has_minimum_budget_v1(
            now + RESPONSE_TAIL_MINIMUM_TOKEN_BUDGET_MILLISECONDS_V1 + 1,
            now,
        ));
        assert!(!response_token_has_minimum_budget_v1(u64::MAX, u64::MAX));
    }

    #[test]
    fn executor_block_reasons_map_one_to_one() {
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::DiscordReadRejected,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::DiscordReadRejected
        );
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::ResponseTokenUnavailable,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::ResponseTokenUnavailable
        );
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::ObservationProtocol
        );
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::CompensationConflict,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::CompensationConflict
        );
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::CompensationUnsupported,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::CompensationUnsupported
        );
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::NonCompensable,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::NonCompensable
        );
        assert_eq!(
            recovery_required_block_reason_v1(
                RuntimeInteractionEffectRecoveryRequiredV1::InternalConflict,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::InternalConflict
        );
        assert_eq!(
            recovery_route_block_reason_v1(
                RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::DiscordForbidden
        );
        assert_eq!(
            recovery_route_block_reason_v1(
                RuntimeInteractionEffectRecoveryRouteBlockV1::InternalAuthority,
            ),
            RuntimeInteractionEffectRecoveryBlockReasonV1::InternalAuthority
        );
    }

    #[test]
    fn only_durable_recovery_required_results_report_route_blocked() {
        for disposition in [
            RuntimeInteractionEffectMutationDispositionV1::Applied,
            RuntimeInteractionEffectMutationDispositionV1::ExactReplay,
        ] {
            assert_eq!(
                durable_effect_recovery_block_disposition_v1(
                    disposition,
                    InteractionEffectStateV1::RecoveryRequired,
                )
                .unwrap(),
                RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked
            );
            assert!(durable_effect_recovery_block_disposition_v1(
                disposition,
                InteractionEffectStateV1::Observing,
            )
            .is_err());
        }
        for disposition in [
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnrecoverable,
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ExactReplay,
        ] {
            assert_eq!(
                durable_response_tail_recovery_block_disposition_v1(
                    disposition,
                    InteractionEffectStateV1::RecoveryRequired,
                )
                .unwrap(),
                RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked
            );
        }
        for disposition in [
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::EffectsCompleted,
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::ResponseUnconfirmed,
            RuntimeInteractionEffectResponseTailFinalizeDispositionV1::Deferred,
        ] {
            assert!(durable_response_tail_recovery_block_disposition_v1(
                disposition,
                InteractionEffectStateV1::RecoveryRequired,
            )
            .is_err());
        }
    }

    #[test]
    fn compensation_states_select_one_exact_recovery_stage() {
        for state in [
            InteractionEffectStateV1::KnownSucceeded,
            InteractionEffectStateV1::ReconciledSucceeded,
        ] {
            assert_eq!(
                compensation_recovery_stage_v1(state).unwrap(),
                RuntimeInteractionEffectCompensationRecoveryStageV1::Begin
            );
        }
        for state in [
            InteractionEffectStateV1::CompensationIntended,
            InteractionEffectStateV1::CompensationIndeterminate,
            InteractionEffectStateV1::CompensationObserving,
            InteractionEffectStateV1::CompensationObservationPending,
        ] {
            assert_eq!(
                compensation_recovery_stage_v1(state).unwrap(),
                RuntimeInteractionEffectCompensationRecoveryStageV1::Observe
            );
        }
        for state in [
            InteractionEffectStateV1::Planned,
            InteractionEffectStateV1::Intended,
            InteractionEffectStateV1::KnownFailed,
            InteractionEffectStateV1::Indeterminate,
            InteractionEffectStateV1::Observing,
            InteractionEffectStateV1::ObservationPending,
            InteractionEffectStateV1::Compensated,
            InteractionEffectStateV1::RecoveryRequired,
        ] {
            assert!(compensation_recovery_stage_v1(state).is_err());
        }
    }

    #[test]
    fn compensation_exact_replay_never_authorizes_an_external_call() {
        assert!(compensation_intent_authorizes_external_call_v1(
            RuntimeInteractionEffectMutationDispositionV1::Applied,
        ));
        assert!(!compensation_intent_authorizes_external_call_v1(
            RuntimeInteractionEffectMutationDispositionV1::ExactReplay,
        ));
    }
}
