use automation_ruleset::CURRENT_RULESET_SCHEMA_VERSION;
use automation_runtime_controller::runtime_failure_message_v1;
use automation_runtime_convergence::{
    CommandGuardV1, FencingToken, LeaseRequestV1, RuntimeDeployment, RuntimeDeploymentError,
    RuntimeDeploymentPhaseV1, RuntimeFailureV1, RuntimePendingConditionV1, TransitionOutcomeV1,
};
use sqlx::types::Json;

use crate::error::database;
use crate::model::{
    ClaimDeploymentV1, ClaimExecutionReceiptV1, ClaimNextDeploymentV1, ClaimReceiptV1,
    DeploymentMutationV1, EnqueueDeploymentOutcomeV1, EnqueueDeploymentV1, MutationReceiptV1,
    RenewDeploymentV1, RuntimeConvergenceAttemptV1, RuntimeDeploymentScopeV1,
    SubmitDeploymentMutationV1,
};
use crate::persistence::desired_target_digest_v1;
use crate::prepare::prepare_requested_deployment_v1;
use crate::row::{
    runtime_i64, DeploymentProjection, DeploymentRow, PersistedDeployment, ServingLeaseRow,
    DEPLOYMENT_COLUMNS, SERVING_LEASE_COLUMNS,
};
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

use super::{attempt, DeploymentExecutionProjection};

impl PostgresRuntimeConvergence {
    pub async fn enqueue(
        &self,
        request: EnqueueDeploymentV1,
    ) -> Result<EnqueueDeploymentOutcomeV1, RuntimeConvergenceStoreError> {
        if request.installation_authority_revision == 0 {
            return Err(RuntimeConvergenceStoreError::InvalidInput(
                "installation authority revision",
            ));
        }
        let desired_digest = desired_target_digest_v1(
            &request.identity,
            &request.target,
            request.runtime_generation.get(),
            request.installation_authority_revision,
            request.previous_runtime.as_ref(),
        )?;
        let mut transaction = self.begin().await?;
        let existing = sqlx::query_as::<_, DeploymentRow>(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE"
        ))
        .bind(request.identity.deployment_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        if let Some(existing) = existing {
            let existing = existing.decode()?;
            let exact = existing.deployment.identity() == &request.identity
                && existing.deployment.target() == &request.target
                && existing.deployment.runtime_generation() == request.runtime_generation
                && existing.deployment.snapshot().previous_runtime == request.previous_runtime
                && existing.installation_authority_revision
                    == request.installation_authority_revision
                && existing.desired_target_digest == desired_digest;
            if exact {
                Self::assert_current_deployment_authority(&mut transaction, &existing).await?;
                let snapshot = existing.deployment.snapshot();
                transaction.commit().await.map_err(database)?;
                return Ok(EnqueueDeploymentOutcomeV1::ExactReplay(snapshot));
            }
            transaction.commit().await.map_err(database)?;
            return Err(RuntimeConvergenceStoreError::IdempotencyConflict);
        }
        let provisional_at = Self::database_now(&mut transaction).await?;
        let provisional = RuntimeDeployment::request(
            request.identity.clone(),
            request.target.clone(),
            request.runtime_generation,
            request.previous_runtime.clone(),
            provisional_at,
        )?;
        let provisional_snapshot = provisional.snapshot();
        Self::assert_current_snapshot_authority(
            &mut transaction,
            &provisional_snapshot,
            request.installation_authority_revision,
        )
        .await?;
        let requested_at =
            Self::assert_previous_runtime_and_now(&mut transaction, &provisional_snapshot).await?;
        let prepared = prepare_requested_deployment_v1(request.clone(), requested_at)?;
        let snapshot = prepared.snapshot().clone();
        let projection = DeploymentProjection::from_snapshot(&snapshot)?;
        let previous_runtime = prepared.previous_runtime_json().cloned().map(Json);
        let inserted = sqlx::query_as::<_, DeploymentRow>(&format!(
            "INSERT INTO public.runtime_deployments (deployment_id, tenant_id, installation_id, \
             promotion_id, activation_request_id, installation_authority_revision, guild_id, \
             ruleset_key, target_version, target_content_hash, binding_revision, \
             binding_fingerprint, desired_target_digest, runtime_generation, previous_runtime, \
             requested_at, snapshot_format_version, snapshot, revision, phase, controller_id, \
             controller_fencing_token, controller_acquired_at, controller_lease_expires_at, \
             last_fencing_token, next_retry_at, last_stable_error_code, live_attestation_id, \
             live_at, blocked_at, superseded_at, cancelled_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                     $16, 1, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, NULL, \
                     $27, $28, $29, $30, $16, $16) \
             ON CONFLICT (deployment_id) DO NOTHING RETURNING {DEPLOYMENT_COLUMNS}"
        ))
        .bind(snapshot.identity.deployment_id.as_str())
        .bind(snapshot.identity.tenant_id.as_str())
        .bind(snapshot.identity.installation_id.as_str())
        .bind(snapshot.identity.promotion_id.as_str())
        .bind(snapshot.identity.activation_request_id.as_str())
        .bind(runtime_i64(request.installation_authority_revision)?)
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .bind(i64::from(snapshot.target.version.get()))
        .bind(snapshot.target.content_hash.to_hex())
        .bind(runtime_i64(snapshot.target.binding_revision.get())?)
        .bind(snapshot.target.binding_fingerprint.as_str())
        .bind(desired_digest.as_str())
        .bind(runtime_i64(snapshot.runtime_generation.get())?)
        .bind(previous_runtime)
        .bind(snapshot.requested_at)
        .bind(Json(prepared.snapshot_json().clone()))
        .bind(runtime_i64(snapshot.revision.get())?)
        .bind(projection.phase)
        .bind(projection.controller_id)
        .bind(projection.controller_fencing_token)
        .bind(projection.controller_acquired_at)
        .bind(projection.controller_lease_expires_at)
        .bind(projection.last_fencing_token)
        .bind(projection.next_retry_at)
        .bind(projection.last_stable_error_code)
        .bind(projection.live_at)
        .bind(projection.blocked_at)
        .bind(projection.superseded_at)
        .bind(projection.cancelled_at)
        .fetch_optional(&mut *transaction)
        .await;
        let inserted = match inserted {
            Ok(Some(row)) => row.decode()?.deployment.snapshot(),
            Ok(None) => {
                transaction.rollback().await.map_err(database)?;
                return self.resolve_enqueue_replay(&request, &desired_digest).await;
            }
            Err(error) if is_unique_violation(&error) => {
                transaction.rollback().await.map_err(database)?;
                return Err(RuntimeConvergenceStoreError::IdempotencyConflict);
            }
            Err(error) => {
                transaction.rollback().await.map_err(database)?;
                return Err(database(error));
            }
        };
        transaction.commit().await.map_err(database)?;
        Ok(EnqueueDeploymentOutcomeV1::Created(inserted))
    }

    async fn resolve_enqueue_replay(
        &self,
        request: &EnqueueDeploymentV1,
        desired_digest: &crate::model::RuntimeDigestV1,
    ) -> Result<EnqueueDeploymentOutcomeV1, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let existing = sqlx::query_as::<_, DeploymentRow>(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE"
        ))
        .bind(request.identity.deployment_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(RuntimeConvergenceStoreError::IdempotencyConflict)?
        .decode()?;
        let exact = existing.deployment.identity() == &request.identity
            && existing.deployment.target() == &request.target
            && existing.deployment.runtime_generation() == request.runtime_generation
            && existing.deployment.snapshot().previous_runtime == request.previous_runtime
            && existing.installation_authority_revision == request.installation_authority_revision
            && &existing.desired_target_digest == desired_digest;
        let snapshot = existing.deployment.snapshot();
        if exact {
            Self::assert_current_deployment_authority(&mut transaction, &existing).await?;
        }
        transaction.commit().await.map_err(database)?;
        if exact {
            Ok(EnqueueDeploymentOutcomeV1::ExactReplay(snapshot))
        } else {
            Err(RuntimeConvergenceStoreError::IdempotencyConflict)
        }
    }

    pub async fn claim(
        &self,
        request: ClaimDeploymentV1,
    ) -> Result<ClaimReceiptV1, RuntimeConvergenceStoreError> {
        self.claim_execution(request).await.map(Into::into)
    }

    pub async fn claim_execution(
        &self,
        request: ClaimDeploymentV1,
    ) -> Result<ClaimExecutionReceiptV1, RuntimeConvergenceStoreError> {
        let lease_duration = self.bounded_lease_duration(
            request.lease_for,
            self.config.maximum_controller_lease,
            "controller lease duration",
        )?;
        let mut transaction = self.begin().await?;
        let persisted = Self::load_scoped_for_update(&mut transaction, &request.scope).await?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let now = Self::mutation_now(&mut transaction).await?;
        if persisted.deployment.revision() != request.expected_revision {
            return Err(RuntimeConvergenceStoreError::RevisionConflict);
        }
        if persisted
            .deployment
            .controller_lease()
            .is_some_and(|lease| lease.expires_at > now)
        {
            return Err(RuntimeConvergenceStoreError::ExecutionClaimStale);
        }
        let receipt = Self::claim_locked(
            &mut transaction,
            &request.scope,
            persisted,
            request.controller_id,
            lease_duration,
            now,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(receipt)
    }

    pub async fn renew_execution(
        &self,
        request: RenewDeploymentV1,
    ) -> Result<ClaimExecutionReceiptV1, RuntimeConvergenceStoreError> {
        let lease_duration = self.bounded_lease_duration(
            request.lease_for,
            self.config.maximum_controller_lease,
            "controller lease duration",
        )?;
        let mut transaction = self.begin().await?;
        let persisted = Self::load_scoped_for_update(&mut transaction, &request.scope).await?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let convergence_attempt =
            Self::require_convergence_attempt(&persisted, request.convergence_attempt)?;
        let now = Self::mutation_now(&mut transaction).await?;
        if persisted.deployment.revision() != request.expected_revision {
            let replay_revision = request
                .expected_revision
                .next()
                .is_ok_and(|revision| revision == persisted.deployment.revision());
            if !replay_revision {
                return Err(RuntimeConvergenceStoreError::RevisionConflict);
            }
            let expected_fencing_token = request
                .fencing_token
                .next()
                .map_err(|_| RuntimeDeploymentError::FencingTokenNotMonotonic)?;
            let lease = persisted
                .deployment
                .controller_lease()
                .ok_or(RuntimeConvergenceStoreError::ExecutionClaimStale)?;
            let exact = persisted.deployment.runtime_generation() == request.runtime_generation
                && persisted.deployment.snapshot().last_fencing_token
                    == Some(expected_fencing_token)
                && lease.controller_id == request.controller_id
                && lease.fencing_token == expected_fencing_token
                && lease.expires_at > now
                && lease.expires_at - lease.acquired_at == lease_duration;
            if !exact {
                return Err(RuntimeConvergenceStoreError::ExecutionClaimStale);
            }
            let receipt = ClaimExecutionReceiptV1 {
                snapshot: persisted.deployment.snapshot(),
                controller_id: lease.controller_id.clone(),
                fencing_token: lease.fencing_token,
                convergence_attempt,
                acquired_at: lease.acquired_at,
                expires_at: lease.expires_at,
            };
            transaction.commit().await.map_err(database)?;
            return Ok(receipt);
        }
        Self::require_controller(
            &persisted.deployment,
            request.expected_revision,
            &request.controller_id,
            request.fencing_token,
            request.runtime_generation,
            now,
        )?;
        let current_expiry = persisted
            .deployment
            .controller_lease()
            .ok_or(RuntimeConvergenceStoreError::ExecutionClaimStale)?
            .expires_at;
        let renewed_expiry = now.checked_add_signed(lease_duration).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("controller lease expiry overflow"),
        )?;
        if renewed_expiry <= current_expiry {
            return Err(RuntimeConvergenceStoreError::InvalidInput(
                "controller renewal must extend lease",
            ));
        }
        let receipt = Self::claim_locked(
            &mut transaction,
            &request.scope,
            persisted,
            request.controller_id,
            lease_duration,
            now,
        )
        .await?;
        if receipt.convergence_attempt != convergence_attempt {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "renewed runtime convergence attempt",
            ));
        }
        transaction.commit().await.map_err(database)?;
        Ok(receipt)
    }

    pub async fn claim_next(
        &self,
        request: ClaimNextDeploymentV1,
    ) -> Result<Option<ClaimReceiptV1>, RuntimeConvergenceStoreError> {
        self.claim_next_execution(request)
            .await
            .map(|receipt| receipt.map(Into::into))
    }

    pub async fn claim_next_execution(
        &self,
        request: ClaimNextDeploymentV1,
    ) -> Result<Option<ClaimExecutionReceiptV1>, RuntimeConvergenceStoreError> {
        let lease_duration = self.bounded_lease_duration(
            request.lease_for,
            self.config.maximum_controller_lease,
            "controller lease duration",
        )?;
        let mut transaction = self.begin().await?;
        let row = sqlx::query_as::<_, DeploymentRow>(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM public.runtime_deployments WHERE deployment_id = (\
             SELECT deployment.deployment_id FROM public.runtime_deployments deployment \
             JOIN public.activation_requests activation \
               ON activation.id = deployment.activation_request_id \
              AND activation.state = 'applied' \
              AND activation.authority_kind = 'product_authoring' \
              AND activation.link_state_name = 'linked' \
              AND activation.promotion_id = deployment.promotion_id \
             JOIN public.authoring_promotions promotion \
               ON promotion.id = deployment.promotion_id \
              AND promotion.stage = 'activation_pending' \
              AND promotion.tenant_id = deployment.tenant_id \
             JOIN public.product_tenants tenant \
               ON tenant.tenant_id = deployment.tenant_id \
              AND tenant.lifecycle_state = 'active' \
             JOIN public.automation_installations installation \
               ON installation.tenant_id = deployment.tenant_id \
              AND installation.installation_id = deployment.installation_id \
              AND installation.lifecycle_state = 'active' \
             JOIN public.automation_installation_authority_versions historical_authority \
               ON historical_authority.tenant_id = installation.tenant_id \
              AND historical_authority.installation_id = installation.installation_id \
              AND historical_authority.revision = deployment.installation_authority_revision \
              AND historical_authority.binding_revision = deployment.binding_revision \
              AND historical_authority.binding_fingerprint = deployment.binding_fingerprint \
             JOIN public.automation_installation_authority_versions current_authority \
               ON current_authority.tenant_id = installation.tenant_id \
              AND current_authority.installation_id = installation.installation_id \
              AND current_authority.revision = installation.current_authority_revision \
              AND current_authority.binding_revision = deployment.binding_revision \
              AND current_authority.binding_fingerprint = deployment.binding_fingerprint \
              AND current_authority.resource_bindings \
                  IS NOT DISTINCT FROM historical_authority.resource_bindings \
             JOIN public.automation_ruleset_activations active \
               ON active.guild_id = deployment.guild_id \
              AND active.ruleset_key = deployment.ruleset_key \
              AND active.active_version = deployment.target_version \
             JOIN public.automation_ruleset_versions version \
               ON version.guild_id = active.guild_id \
              AND version.ruleset_key = active.ruleset_key \
              AND version.version = active.active_version \
              AND version.content_hash = deployment.target_content_hash \
              AND version.canonical_content_hash = version.content_hash \
              AND version.schema_version = $1 \
             WHERE deployment.phase NOT IN ('live','superseded','cancelled') \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{intent,authority,tenant_id}}' \
                   = deployment.tenant_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{intent,authority,installation_id}}' \
                   = deployment.installation_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{intent,authority,guild_id}}' \
                   = deployment.guild_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{intent,authority,ruleset_key}}' \
                   = deployment.ruleset_key \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{intent,authority,binding_revision}}' \
                   = deployment.binding_revision::TEXT \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{intent,evidence,context_fingerprint}}' \
                   = deployment.binding_fingerprint \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{stage,activation,request_id}}' \
                   = deployment.activation_request_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{stage,activation,target,guild_id}}' \
                   = deployment.guild_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{stage,activation,target,ruleset_key}}' \
                   = deployment.ruleset_key \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{stage,activation,target,version}}' \
                   = deployment.target_version::TEXT \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{{stage,activation,target,content_hash}}' \
                   = deployment.target_content_hash \
               AND deployment.blocked_at IS NULL \
               AND (deployment.next_retry_at IS NULL \
                    OR deployment.next_retry_at <= pg_catalog.clock_timestamp()) \
               AND (deployment.controller_lease_expires_at IS NULL \
                    OR deployment.controller_lease_expires_at <= pg_catalog.clock_timestamp()) \
             ORDER BY COALESCE(deployment.next_retry_at, deployment.requested_at), \
                      deployment.requested_at, deployment.deployment_id \
             LIMIT 1 FOR UPDATE OF deployment SKIP LOCKED) FOR UPDATE"
        ))
        .bind(i64::from(CURRENT_RULESET_SCHEMA_VERSION.get()))
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(database)?;
            return Ok(None);
        };
        let scope = RuntimeDeploymentScopeV1 {
            tenant_id: automation_runtime_convergence::TenantId::parse(row.tenant_id.clone())
                .map_err(|_| {
                    RuntimeConvergenceStoreError::InvalidPersistedState("deployment tenant")
                })?,
            installation_id: automation_runtime_convergence::InstallationId::parse(
                row.installation_id.clone(),
            )
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("deployment installation")
            })?,
            deployment_id: automation_runtime_convergence::DeploymentId::parse(
                row.deployment_id.clone(),
            )
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("deployment identity")
            })?,
        };
        let persisted = row.decode()?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let now = Self::mutation_now(&mut transaction).await?;
        let receipt = Self::claim_locked(
            &mut transaction,
            &scope,
            persisted,
            request.controller_id,
            lease_duration,
            now,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(Some(receipt))
    }

    async fn claim_locked(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        scope: &RuntimeDeploymentScopeV1,
        mut persisted: PersistedDeployment,
        controller_id: automation_runtime_convergence::ControllerId,
        lease_duration: chrono::TimeDelta,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<ClaimExecutionReceiptV1, RuntimeConvergenceStoreError> {
        let previous_revision = persisted.deployment.revision();
        let convergence_attempt = attempt::next_claim_attempt(&persisted, now)?;
        let next_fencing_value = persisted
            .deployment
            .snapshot()
            .last_fencing_token
            .map(|token| token.get().checked_add(1))
            .unwrap_or(Some(1))
            .ok_or(RuntimeDeploymentError::FencingTokenNotMonotonic)?;
        let fencing_token = FencingToken::new(next_fencing_value)
            .map_err(|_| RuntimeDeploymentError::FencingTokenNotMonotonic)?;
        let expires_at = now.checked_add_signed(lease_duration).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("controller lease expiry overflow"),
        )?;
        persisted.deployment.acquire_lease(LeaseRequestV1 {
            expected_revision: previous_revision,
            controller_id: controller_id.clone(),
            fencing_token,
            now,
            expires_at,
        })?;
        Self::persist_deployment(
            transaction,
            scope,
            previous_revision.get(),
            &persisted.deployment,
            DeploymentExecutionProjection {
                live_attestation_id: persisted.live_attestation_id.as_ref().map(|id| id.as_str()),
                last_controller_id: Some(controller_id.as_str()),
                convergence_attempt: RuntimeConvergenceAttemptV1::from(convergence_attempt),
                last_failure_attempt: persisted.last_failure_attempt,
            },
            now,
        )
        .await?;
        Ok(ClaimExecutionReceiptV1 {
            snapshot: persisted.deployment.snapshot(),
            controller_id,
            fencing_token,
            convergence_attempt,
            acquired_at: now,
            expires_at,
        })
    }

    pub async fn mutate(
        &self,
        request: SubmitDeploymentMutationV1,
    ) -> Result<MutationReceiptV1, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let mut persisted = Self::load_scoped_for_update(&mut transaction, &request.scope).await?;
        let convergence_attempt =
            Self::require_convergence_attempt(&persisted, request.convergence_attempt)?;
        let mutation = sanitize_failure_evidence(request.mutation.clone());
        if terminal_replay(&persisted, &request, &mutation, convergence_attempt) {
            let snapshot = persisted.deployment.snapshot();
            let outcome = TransitionOutcomeV1::Replayed {
                revision: snapshot.revision,
            };
            transaction.commit().await.map_err(database)?;
            return Ok(MutationReceiptV1 {
                outcome,
                snapshot,
                convergence_attempt,
            });
        }
        if attempt::failure_replay(
            self,
            &persisted,
            request.expected_revision,
            &request.controller_id,
            request.fencing_token,
            request.runtime_generation,
            &mutation,
        )? {
            Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
            let snapshot = persisted.deployment.snapshot();
            let outcome = TransitionOutcomeV1::Replayed {
                revision: snapshot.revision,
            };
            transaction.commit().await.map_err(database)?;
            return Ok(MutationReceiptV1 {
                outcome,
                snapshot,
                convergence_attempt,
            });
        }
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let now = match (&mutation, persisted.deployment.phase()) {
            (
                DeploymentMutationV1::AcceptDrain(attestation),
                RuntimeDeploymentPhaseV1::DrainRequested,
            ) => {
                Self::assert_accept_drain_previous_serving(
                    &mut transaction,
                    &persisted,
                    attestation.drained_at,
                )
                .await?
            }
            _ => Self::mutation_now(&mut transaction).await?,
        };
        if matches!(
            &mutation,
            DeploymentMutationV1::RecordRetryableFailure { attempt, .. }
                if *attempt != convergence_attempt
        ) {
            return Err(RuntimeConvergenceStoreError::ConvergenceAttemptConflict);
        }
        if matches!(&mutation, DeploymentMutationV1::ResumeRuntimePending) {
            attempt::validate_runtime_resume(&persisted, now)?;
        }
        let records_failure = matches!(
            &mutation,
            DeploymentMutationV1::RecordRetryableFailure { .. }
                | DeploymentMutationV1::RecordBlockedFailure { .. }
        );
        Self::require_controller(
            &persisted.deployment,
            request.expected_revision,
            &request.controller_id,
            request.fencing_token,
            request.runtime_generation,
            now,
        )?;
        self.validate_mutation_times(&mutation, now)?;
        let executing_controller_id = request.controller_id.clone();
        let guard = CommandGuardV1 {
            expected_revision: request.expected_revision,
            controller_id: request.controller_id,
            fencing_token: request.fencing_token,
            runtime_generation: request.runtime_generation,
            now,
        };
        let outcome = match mutation {
            DeploymentMutationV1::AcceptPreflight(attestation) => {
                persisted.deployment.accept_preflight(&guard, attestation)?
            }
            DeploymentMutationV1::RequestDrain => persisted.deployment.request_drain(&guard)?,
            DeploymentMutationV1::AcceptDrain(attestation) => {
                persisted.deployment.accept_drain(&guard, attestation)?
            }
            DeploymentMutationV1::BeginActivation => {
                persisted.deployment.begin_activation(&guard)?
            }
            DeploymentMutationV1::AcceptActivation(attestation) => persisted
                .deployment
                .accept_activation(&guard, attestation)?,
            DeploymentMutationV1::RecordRetryableFailure {
                failure_id,
                kind,
                code,
                message,
                attempt,
                retry_after,
            } => {
                let existing = matching_retryable_failure(
                    &persisted.deployment,
                    &failure_id,
                    kind,
                    &code,
                    &message,
                    attempt,
                );
                let (failure, retry_not_before) = if let Some(existing) = existing {
                    existing
                } else {
                    let delay = Self::bounded_duration(
                        retry_after,
                        self.config.maximum_retry_delay,
                        "retry delay",
                    )?;
                    let retry_not_before = now.checked_add_signed(delay).ok_or(
                        RuntimeConvergenceStoreError::InvalidInput("retry time overflow"),
                    )?;
                    (
                        RuntimeFailureV1 {
                            failure_id,
                            kind,
                            code,
                            message,
                            recorded_at: now,
                        },
                        retry_not_before,
                    )
                };
                persisted.deployment.record_retryable_failure(
                    &guard,
                    failure,
                    attempt,
                    retry_not_before,
                )?
            }
            DeploymentMutationV1::RecordBlockedFailure {
                failure_id,
                kind,
                code,
                message,
            } => {
                let failure = matching_blocked_failure(
                    &persisted.deployment,
                    &failure_id,
                    kind,
                    &code,
                    &message,
                )
                .unwrap_or(RuntimeFailureV1 {
                    failure_id,
                    kind,
                    code,
                    message,
                    recorded_at: now,
                });
                persisted
                    .deployment
                    .record_blocked_failure(&guard, failure)?
            }
            DeploymentMutationV1::ResumeRuntimePending => {
                persisted.deployment.resume_runtime_pending(&guard)?
            }
            DeploymentMutationV1::BeginPanelReconciliation => {
                persisted.deployment.begin_panel_reconciliation(&guard)?
            }
            DeploymentMutationV1::AcceptPanelCertificate(certificate) => persisted
                .deployment
                .accept_panel_certificate(&guard, certificate)?,
            DeploymentMutationV1::Supersede { by, reason } => {
                persisted.deployment.supersede(&guard, by, reason, now)?
            }
            DeploymentMutationV1::Cancel { reason } => {
                persisted.deployment.cancel(&guard, reason, now)?
            }
        };
        if matches!(outcome, TransitionOutcomeV1::Applied { .. }) {
            let last_failure_attempt = if records_failure {
                Some(convergence_attempt)
            } else {
                persisted.last_failure_attempt
            };
            Self::persist_deployment(
                &mut transaction,
                &request.scope,
                request.expected_revision.get(),
                &persisted.deployment,
                DeploymentExecutionProjection {
                    live_attestation_id: persisted
                        .live_attestation_id
                        .as_ref()
                        .map(|id| id.as_str()),
                    last_controller_id: Some(executing_controller_id.as_str()),
                    convergence_attempt: RuntimeConvergenceAttemptV1::from(convergence_attempt),
                    last_failure_attempt,
                },
                now,
            )
            .await?;
        }
        let snapshot = persisted.deployment.snapshot();
        transaction.commit().await.map_err(database)?;
        Ok(MutationReceiptV1 {
            outcome,
            snapshot,
            convergence_attempt,
        })
    }

    fn validate_mutation_times(
        &self,
        mutation: &DeploymentMutationV1,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        match mutation {
            DeploymentMutationV1::AcceptPreflight(attestation) => {
                self.ensure_not_future(attestation.checked_at, now)
            }
            DeploymentMutationV1::AcceptDrain(attestation) => {
                self.ensure_not_future(attestation.drained_at, now)
            }
            DeploymentMutationV1::AcceptActivation(attestation) => {
                self.ensure_not_future(attestation.activated_at, now)
            }
            DeploymentMutationV1::AcceptPanelCertificate(certificate) => {
                self.ensure_not_future(certificate.reconciled_at, now)
            }
            _ => Ok(()),
        }
    }

    async fn assert_accept_drain_previous_serving(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        persisted: &PersistedDeployment,
        drained_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<chrono::DateTime<chrono::Utc>, RuntimeConvergenceStoreError> {
        let snapshot = persisted.deployment.snapshot();
        Self::lock_serving_slot(
            transaction,
            &snapshot.target.guild_id.to_string(),
            snapshot.target.ruleset_key.as_str(),
        )
        .await?;
        let serving = sqlx::query_as::<_, ServingLeaseRow>(&format!(
            "SELECT {SERVING_LEASE_COLUMNS} FROM public.runtime_serving_leases \
             WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE"
        ))
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?;
        let now = Self::mutation_now(transaction).await?;
        let closure = match (snapshot.previous_runtime.as_ref(), serving.as_ref()) {
            (None, None) => DrainClosureV1::Absent,
            (None, Some(serving)) => {
                validate_drain_serving_state(serving, now)?;
                DrainClosureV1::Closed(
                    serving_closure_boundary(serving, now)
                        .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?,
                )
            }
            (Some(_), None) => return Err(RuntimeConvergenceStoreError::ServingLeaseConflict),
            (Some(previous), Some(serving)) => {
                validate_drain_serving_state(serving, now)?;
                let exact = serving.tenant_id == snapshot.identity.tenant_id.as_str()
                    && serving.installation_id == snapshot.identity.installation_id.as_str()
                    && serving.deployment_id != snapshot.identity.deployment_id.as_str()
                    && serving.guild_id == previous.target.guild_id.to_string()
                    && serving.ruleset_key == previous.target.ruleset_key.as_str()
                    && serving.target_version == i64::from(previous.target.version.get())
                    && serving.target_content_hash == previous.target.content_hash.to_hex()
                    && serving.binding_revision
                        == runtime_i64(previous.target.binding_revision.get())?
                    && serving.binding_fingerprint == previous.target.binding_fingerprint.as_str()
                    && serving.runtime_generation
                        == runtime_i64(previous.runtime_generation.get())?
                    && serving.process_instance_id == previous.process_instance_id.as_str()
                    && serving.acquired_at <= snapshot.requested_at;
                let boundary = serving_closure_boundary(serving, now)
                    .filter(|boundary| exact && *boundary >= snapshot.requested_at)
                    .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?;
                DrainClosureV1::Closed(boundary)
            }
        };
        match closure {
            DrainClosureV1::Closed(boundary) if drained_at < boundary => Err(
                automation_runtime_convergence::RuntimeDeploymentError::AttestationTimeRegression
                    .into(),
            ),
            DrainClosureV1::Absent | DrainClosureV1::Closed(_) => Ok(now),
        }
    }
}

enum DrainClosureV1 {
    Absent,
    Closed(chrono::DateTime<chrono::Utc>),
}

fn serving_closure_boundary(
    serving: &ServingLeaseRow,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    if !serving.connected {
        Some(serving.last_heartbeat_at)
    } else if serving.expires_at <= now {
        Some(serving.expires_at)
    } else {
        None
    }
}

fn validate_drain_serving_state(
    serving: &ServingLeaseRow,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), RuntimeConvergenceStoreError> {
    serving.validate()?;
    let temporal_state_valid = if serving.connected {
        serving.last_heartbeat_at < serving.expires_at && serving.last_heartbeat_at <= now
    } else {
        serving.last_heartbeat_at == serving.expires_at && serving.expires_at <= now
    };
    if serving.acquired_at <= now && temporal_state_valid {
        Ok(())
    } else {
        Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "accept drain serving lease projections",
        ))
    }
}

fn terminal_replay(
    persisted: &PersistedDeployment,
    request: &SubmitDeploymentMutationV1,
    mutation: &DeploymentMutationV1,
    convergence_attempt: std::num::NonZeroU32,
) -> bool {
    let snapshot = persisted.deployment.snapshot();
    let guard_is_exact = request
        .expected_revision
        .next()
        .is_ok_and(|revision| revision == snapshot.revision)
        && snapshot.last_fencing_token == Some(request.fencing_token)
        && persisted.last_controller_id.as_ref() == Some(&request.controller_id)
        && snapshot.runtime_generation == request.runtime_generation
        && persisted
            .exact_convergence_attempt()
            .is_ok_and(|attempt| attempt.started() == Some(convergence_attempt));
    guard_is_exact
        && match (persisted.deployment.phase(), mutation) {
            (
                RuntimeDeploymentPhaseV1::Superseded {
                    by,
                    reason: current_reason,
                    ..
                },
                DeploymentMutationV1::Supersede {
                    by: requested,
                    reason,
                },
            ) => by == requested && current_reason == reason,
            (
                RuntimeDeploymentPhaseV1::Cancelled {
                    reason: current_reason,
                    ..
                },
                DeploymentMutationV1::Cancel { reason },
            ) => current_reason == reason,
            _ => false,
        }
}

fn matching_retryable_failure(
    deployment: &RuntimeDeployment,
    failure_id: &automation_runtime_convergence::RuntimeFailureId,
    kind: automation_runtime_convergence::RuntimeFailureKindV1,
    code: &str,
    message: &str,
    attempt: std::num::NonZeroU32,
) -> Option<(RuntimeFailureV1, chrono::DateTime<chrono::Utc>)> {
    match deployment.phase() {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    failure,
                    attempt: current_attempt,
                    retry_not_before,
                },
        } if &failure.failure_id == failure_id
            && failure.kind == kind
            && failure.code == code
            && failure.message == message
            && *current_attempt == attempt =>
        {
            Some((failure.clone(), *retry_not_before))
        }
        _ => None,
    }
}

fn matching_blocked_failure(
    deployment: &RuntimeDeployment,
    failure_id: &automation_runtime_convergence::RuntimeFailureId,
    kind: automation_runtime_convergence::RuntimeFailureKindV1,
    code: &str,
    message: &str,
) -> Option<RuntimeFailureV1> {
    match deployment.phase() {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { failure },
        } if &failure.failure_id == failure_id
            && failure.kind == kind
            && failure.code == code
            && failure.message == message =>
        {
            Some(failure.clone())
        }
        _ => None,
    }
}

fn sanitize_failure_evidence(mutation: DeploymentMutationV1) -> DeploymentMutationV1 {
    match mutation {
        DeploymentMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            attempt,
            retry_after,
            ..
        } => DeploymentMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            message: runtime_failure_message_v1(kind).to_string(),
            attempt,
            retry_after,
        },
        DeploymentMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
            ..
        } => DeploymentMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
            message: runtime_failure_message_v1(kind).to_string(),
        },
        mutation => mutation,
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23505")
}
