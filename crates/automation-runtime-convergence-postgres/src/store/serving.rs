use automation_runtime_convergence::{
    CommandGuardV1, LiveLossKindV1, RecoverLiveRequestV1, RuntimeDeploymentPhaseV1,
    TransitionOutcomeV1,
};
use sqlx::types::Json;

use crate::digest::live_attestation_digest;
use crate::error::database;
use crate::model::{
    AttestationIdV1, AttestationRecordV1, HeartbeatServingLeaseV1, MarkServingDisconnectedV1,
    MutationReceiptV1, RecoverStaleLiveV1, ServingLeaseIdentityV1, ServingLeaseReceiptV1,
    SubmitLiveAttestationV1,
};
use crate::row::{
    gateway_ready_kind_name, runtime_i64, AttestationRow, ServingLeaseRow, ATTESTATION_COLUMNS,
    SERVING_LEASE_COLUMNS,
};
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

impl PostgresRuntimeConvergence {
    pub async fn certify_live(
        &self,
        request: SubmitLiveAttestationV1,
    ) -> Result<(MutationReceiptV1, ServingLeaseReceiptV1), RuntimeConvergenceStoreError> {
        let serving_duration = self.bounded_lease_duration(
            request.serving_lease_for,
            self.config.maximum_serving_lease,
            "serving lease duration",
        )?;
        let mut transaction = self.begin().await?;
        let mut persisted = Self::load_scoped_for_update(&mut transaction, &request.scope).await?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let current_serving = Self::load_serving_lease_for_update(
            &mut transaction,
            persisted.deployment.target().guild_id.to_string(),
            persisted.deployment.target().ruleset_key.as_str(),
        )
        .await?;
        let now = Self::mutation_now(&mut transaction).await?;
        if matches!(persisted.deployment.phase(), RuntimeDeploymentPhaseV1::Live) {
            let serving = current_serving
                .as_ref()
                .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?;
            let replay =
                Self::replay_live(&mut transaction, &request, &persisted, serving, now).await?;
            transaction.commit().await.map_err(database)?;
            return Ok(replay);
        }
        self.ensure_gateway_ready_fresh(request.gateway_ready.ready_at, now)?;
        Self::require_controller(
            &persisted.deployment,
            request.expected_revision,
            &request.controller_id,
            request.fencing_token,
            request.runtime_generation,
            now,
        )?;
        let guard = CommandGuardV1 {
            expected_revision: request.expected_revision,
            controller_id: request.controller_id,
            fencing_token: request.fencing_token,
            runtime_generation: request.runtime_generation,
            now,
        };
        let outcome = persisted
            .deployment
            .certify_live(&guard, request.gateway_ready, now)?;
        let snapshot = persisted.deployment.snapshot();
        let live =
            snapshot
                .live
                .clone()
                .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "Live transition did not produce an attestation",
                ))?;
        let record = AttestationRecordV1 {
            live: live.clone(),
            runtime_build_revision: request.metadata.runtime_build_revision,
            panel_report_digest: request.metadata.panel_report_digest,
            gateway_shard_id: request.metadata.gateway_shard_id,
            controller_fencing_token: request.fencing_token,
            deployment_revision: snapshot.revision,
        };
        let attestation_id =
            AttestationIdV1::from(live_attestation_digest(&record).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidInput("Live attestation serialization")
            })?);
        let existing_attestation =
            Self::load_attestation(&mut transaction, &request.scope, &attestation_id, true).await?;
        match (outcome, existing_attestation) {
            (TransitionOutcomeV1::Applied { .. }, None) => {
                Self::insert_attestation(
                    &mut transaction,
                    &request.scope,
                    &attestation_id,
                    &record,
                    &snapshot,
                )
                .await?;
            }
            (TransitionOutcomeV1::Replayed { .. }, Some(existing)) if existing.record == record => {
            }
            (TransitionOutcomeV1::Applied { .. }, Some(_))
            | (TransitionOutcomeV1::Replayed { .. }, None)
            | (TransitionOutcomeV1::Replayed { .. }, Some(_)) => {
                return Err(RuntimeConvergenceStoreError::AttestationConflict);
            }
        }
        if let Some(current) = current_serving.as_ref() {
            let exact = current.tenant_id == request.scope.tenant_id.as_str()
                && current.installation_id == request.scope.installation_id.as_str()
                && current.deployment_id == request.scope.deployment_id.as_str()
                && current.attestation_id == attestation_id.as_str()
                && current.process_instance_id == live.process_instance_id.as_str()
                && current.runtime_generation == runtime_i64(live.runtime_generation.get())?;
            if exact && current.connected && current.serving && current.expires_at > now {
                if !matches!(outcome, TransitionOutcomeV1::Replayed { .. }) {
                    return Err(RuntimeConvergenceStoreError::AttestationConflict);
                }
                let receipt = serving_receipt(&request.scope, current)?;
                transaction.commit().await.map_err(database)?;
                return Ok((MutationReceiptV1 { outcome, snapshot }, receipt));
            }
            if current.expires_at > now && (current.connected || current.serving) {
                return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
            }
            if matches!(outcome, TransitionOutcomeV1::Replayed { .. }) {
                return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
            }
        }
        let (lease_epoch, serving_revision) = match current_serving.as_ref() {
            Some(current) => (
                current.checked_epoch()?.checked_add(1).ok_or(
                    RuntimeConvergenceStoreError::InvalidPersistedState(
                        "serving lease epoch overflow",
                    ),
                )?,
                current.checked_revision()?.checked_add(1).ok_or(
                    RuntimeConvergenceStoreError::InvalidPersistedState(
                        "serving lease revision overflow",
                    ),
                )?,
            ),
            None => (1, 1),
        };
        let expires_at = now.checked_add_signed(serving_duration).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("serving lease expiry overflow"),
        )?;
        let serving = if let Some(current) = current_serving.as_ref() {
            sqlx::query_as::<_, ServingLeaseRow>(&format!(
                "UPDATE public.runtime_serving_leases SET tenant_id = $3, installation_id = $4, \
                 deployment_id = $5, attestation_id = $6, process_instance_id = $7, \
                 runtime_generation = $8, target_version = $9, target_content_hash = $10, \
                 binding_revision = $11, binding_fingerprint = $12, lease_epoch = $13, \
                 revision = $14, connected = TRUE, serving = TRUE, acquired_at = $15, \
                 last_heartbeat_at = $15, expires_at = $16 \
                 WHERE guild_id = $1 AND ruleset_key = $2 AND lease_epoch = $17 AND revision = $18 \
                 RETURNING {SERVING_LEASE_COLUMNS}"
            ))
            .bind(live.target.guild_id.to_string())
            .bind(live.target.ruleset_key.as_str())
            .bind(request.scope.tenant_id.as_str())
            .bind(request.scope.installation_id.as_str())
            .bind(request.scope.deployment_id.as_str())
            .bind(attestation_id.as_str())
            .bind(live.process_instance_id.as_str())
            .bind(runtime_i64(live.runtime_generation.get())?)
            .bind(i64::from(live.target.version.get()))
            .bind(live.target.content_hash.to_hex())
            .bind(runtime_i64(live.target.binding_revision.get())?)
            .bind(live.target.binding_fingerprint.as_str())
            .bind(runtime_i64(lease_epoch)?)
            .bind(runtime_i64(serving_revision)?)
            .bind(now)
            .bind(expires_at)
            .bind(current.lease_epoch)
            .bind(current.revision)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_serving_database)?
            .ok_or(RuntimeConvergenceStoreError::RevisionConflict)?
        } else {
            sqlx::query_as::<_, ServingLeaseRow>(&format!(
                "INSERT INTO public.runtime_serving_leases (guild_id, ruleset_key, tenant_id, \
                 installation_id, deployment_id, attestation_id, process_instance_id, \
                 runtime_generation, target_version, target_content_hash, binding_revision, \
                 binding_fingerprint, lease_epoch, revision, connected, serving, acquired_at, \
                 last_heartbeat_at, expires_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, TRUE, \
                         TRUE, $15, $15, $16) RETURNING {SERVING_LEASE_COLUMNS}"
            ))
            .bind(live.target.guild_id.to_string())
            .bind(live.target.ruleset_key.as_str())
            .bind(request.scope.tenant_id.as_str())
            .bind(request.scope.installation_id.as_str())
            .bind(request.scope.deployment_id.as_str())
            .bind(attestation_id.as_str())
            .bind(live.process_instance_id.as_str())
            .bind(runtime_i64(live.runtime_generation.get())?)
            .bind(i64::from(live.target.version.get()))
            .bind(live.target.content_hash.to_hex())
            .bind(runtime_i64(live.target.binding_revision.get())?)
            .bind(live.target.binding_fingerprint.as_str())
            .bind(runtime_i64(lease_epoch)?)
            .bind(runtime_i64(serving_revision)?)
            .bind(now)
            .bind(expires_at)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_serving_database)?
        };
        if matches!(outcome, TransitionOutcomeV1::Applied { .. }) {
            Self::persist_deployment(
                &mut transaction,
                &request.scope,
                request.expected_revision.get(),
                &persisted.deployment,
                Some(attestation_id.as_str()),
                now,
            )
            .await?;
        }
        let receipt = serving_receipt(&request.scope, &serving)?;
        transaction.commit().await.map_err(database)?;
        Ok((MutationReceiptV1 { outcome, snapshot }, receipt))
    }

    pub async fn heartbeat_serving(
        &self,
        request: HeartbeatServingLeaseV1,
    ) -> Result<ServingLeaseReceiptV1, RuntimeConvergenceStoreError> {
        let serving_duration = self.bounded_lease_duration(
            request.lease_for,
            self.config.maximum_serving_lease,
            "serving lease duration",
        )?;
        let mut transaction = self.begin().await?;
        let persisted =
            Self::load_scoped_for_update(&mut transaction, &request.identity.scope).await?;
        if !matches!(persisted.deployment.phase(), RuntimeDeploymentPhaseV1::Live)
            || persisted.live_attestation_id.as_ref() != Some(&request.identity.attestation_id)
        {
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        }
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let current = Self::load_serving_lease_for_update(
            &mut transaction,
            persisted.deployment.target().guild_id.to_string(),
            persisted.deployment.target().ruleset_key.as_str(),
        )
        .await?
        .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?;
        let now = Self::mutation_now(&mut transaction).await?;
        validate_serving_identity(&current, &request.identity)?;
        if current.expires_at <= now || !current.connected || !current.serving {
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        }
        let next_revision = current.checked_revision()?.checked_add(1).ok_or(
            RuntimeConvergenceStoreError::InvalidPersistedState("serving lease revision overflow"),
        )?;
        let expires_at = now.checked_add_signed(serving_duration).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("serving lease expiry overflow"),
        )?;
        let updated = sqlx::query_as::<_, ServingLeaseRow>(&format!(
            "UPDATE public.runtime_serving_leases SET revision = $4, connected = TRUE, serving = TRUE, \
             last_heartbeat_at = $5, expires_at = $6 \
             WHERE guild_id = $1 AND ruleset_key = $2 AND revision = $3 \
             RETURNING {SERVING_LEASE_COLUMNS}"
        ))
        .bind(&current.guild_id)
        .bind(&current.ruleset_key)
        .bind(runtime_i64(request.identity.expected_revision)?)
        .bind(runtime_i64(next_revision)?)
        .bind(now)
        .bind(expires_at)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_serving_database)?
        .ok_or(RuntimeConvergenceStoreError::RevisionConflict)?;
        let receipt = serving_receipt(&request.identity.scope, &updated)?;
        transaction.commit().await.map_err(database)?;
        Ok(receipt)
    }

    pub async fn mark_serving_disconnected(
        &self,
        request: MarkServingDisconnectedV1,
    ) -> Result<ServingLeaseReceiptV1, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let persisted =
            Self::load_scoped_for_update(&mut transaction, &request.identity.scope).await?;
        let current = Self::load_serving_lease_for_update(
            &mut transaction,
            persisted.deployment.target().guild_id.to_string(),
            persisted.deployment.target().ruleset_key.as_str(),
        )
        .await?
        .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?;
        let now = Self::mutation_now(&mut transaction).await?;
        validate_serving_identity_except_revision(&current, &request.identity)?;
        if !current.connected && !current.serving {
            let receipt = serving_receipt(&request.identity.scope, &current)?;
            transaction.commit().await.map_err(database)?;
            return Ok(receipt);
        }
        if current.checked_revision()? != request.identity.expected_revision {
            return Err(RuntimeConvergenceStoreError::RevisionConflict);
        }
        let next_revision = current.checked_revision()?.checked_add(1).ok_or(
            RuntimeConvergenceStoreError::InvalidPersistedState("serving lease revision overflow"),
        )?;
        let updated = sqlx::query_as::<_, ServingLeaseRow>(&format!(
            "UPDATE public.runtime_serving_leases SET revision = $4, connected = FALSE, serving = FALSE, \
             last_heartbeat_at = $5, expires_at = $5 \
             WHERE guild_id = $1 AND ruleset_key = $2 AND revision = $3 \
             RETURNING {SERVING_LEASE_COLUMNS}"
        ))
        .bind(&current.guild_id)
        .bind(&current.ruleset_key)
        .bind(runtime_i64(request.identity.expected_revision)?)
        .bind(runtime_i64(next_revision)?)
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_serving_database)?
        .ok_or(RuntimeConvergenceStoreError::RevisionConflict)?;
        let receipt = serving_receipt(&request.identity.scope, &updated)?;
        transaction.commit().await.map_err(database)?;
        Ok(receipt)
    }

    pub async fn recover_stale_live(
        &self,
        request: RecoverStaleLiveV1,
    ) -> Result<MutationReceiptV1, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let persisted =
            Self::load_scoped_for_update(&mut transaction, &request.identity.scope).await?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let current = Self::load_serving_lease_for_update(
            &mut transaction,
            persisted.deployment.target().guild_id.to_string(),
            persisted.deployment.target().ruleset_key.as_str(),
        )
        .await?
        .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?;
        let now = Self::mutation_now(&mut transaction).await?;
        let receipt =
            Self::recover_locked(&mut transaction, request, persisted, current, now).await?;
        transaction.commit().await.map_err(database)?;
        Ok(receipt)
    }

    async fn recover_locked(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        request: RecoverStaleLiveV1,
        mut persisted: crate::row::PersistedDeployment,
        current: ServingLeaseRow,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<MutationReceiptV1, RuntimeConvergenceStoreError> {
        validate_serving_identity(&current, &request.identity)?;
        let (kind, evidence_at) = if !current.connected || !current.serving {
            (
                LiveLossKindV1::ServingDisconnected,
                current.last_heartbeat_at,
            )
        } else if current.expires_at <= now {
            (LiveLossKindV1::ServingLeaseExpired, current.expires_at)
        } else {
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        };
        let snapshot = persisted.deployment.snapshot();
        if !matches!(snapshot.phase, RuntimeDeploymentPhaseV1::Live) {
            let exact_replay = request
                .expected_deployment_revision
                .next()
                .is_ok_and(|revision| revision == snapshot.revision)
                && snapshot
                    .last_live_recovery
                    .as_ref()
                    .is_some_and(|recovery| {
                        recovery.prior_live.process_instance_id
                            == request.identity.process_instance_id
                            && recovery.kind == kind
                            && recovery.evidence_at == evidence_at
                    });
            if exact_replay {
                let outcome = TransitionOutcomeV1::Replayed {
                    revision: snapshot.revision,
                };
                return Ok(MutationReceiptV1 { outcome, snapshot });
            }
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        }
        if snapshot.revision != request.expected_deployment_revision
            || persisted.live_attestation_id.as_ref() != Some(&request.identity.attestation_id)
            || snapshot.live.as_ref().is_none_or(|live| {
                live.process_instance_id != request.identity.process_instance_id
                    || live.runtime_generation != snapshot.runtime_generation
            })
        {
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        }
        let newer_unresolved = sqlx::query_scalar::<_, String>(
            "SELECT deployment_id FROM public.runtime_deployments \
             WHERE guild_id = $1 AND ruleset_key = $2 AND deployment_id <> $3 \
               AND phase NOT IN ('live','superseded','cancelled') \
             ORDER BY deployment_id LIMIT 1",
        )
        .bind(snapshot.target.guild_id.to_string())
        .bind(snapshot.target.ruleset_key.as_str())
        .bind(snapshot.identity.deployment_id.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)?;
        if newer_unresolved.is_some() {
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        }
        let outcome = persisted.deployment.recover_live(RecoverLiveRequestV1 {
            expected_revision: request.expected_deployment_revision,
            expected_runtime_generation: snapshot.runtime_generation,
            expected_process_instance_id: request.identity.process_instance_id,
            kind,
            evidence_at,
            recovered_at: now,
        })?;
        Self::persist_deployment(
            transaction,
            &request.identity.scope,
            request.expected_deployment_revision.get(),
            &persisted.deployment,
            None,
            now,
        )
        .await?;
        let snapshot = persisted.deployment.snapshot();
        Ok(MutationReceiptV1 { outcome, snapshot })
    }

    pub async fn recover_next_stale_live(
        &self,
    ) -> Result<Option<MutationReceiptV1>, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin().await?;
        let candidate = sqlx::query_as::<_, StaleLiveCandidateRow>(
            "SELECT deployment.tenant_id, deployment.installation_id, \
                    deployment.deployment_id, deployment.revision AS deployment_revision, \
                    serving.attestation_id, serving.process_instance_id, serving.lease_epoch, \
                    serving.revision AS serving_revision \
             FROM public.runtime_deployments deployment \
             JOIN public.runtime_serving_leases serving \
               ON serving.guild_id = deployment.guild_id \
              AND serving.ruleset_key = deployment.ruleset_key \
              AND serving.tenant_id = deployment.tenant_id \
              AND serving.installation_id = deployment.installation_id \
              AND serving.deployment_id = deployment.deployment_id \
              AND serving.attestation_id = deployment.live_attestation_id \
             JOIN public.activation_requests activation \
               ON activation.id = deployment.activation_request_id \
              AND activation.authority_kind = 'product_authoring' \
              AND activation.link_state_name = 'linked' \
              AND activation.state = 'applied' \
              AND activation.promotion_id = deployment.promotion_id \
              AND activation.guild_id = deployment.guild_id \
              AND activation.ruleset_key = deployment.ruleset_key \
              AND activation.target_version = deployment.target_version \
              AND activation.target_content_hash = deployment.target_content_hash \
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
             WHERE deployment.phase = 'live' \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,tenant_id}' \
                   = deployment.tenant_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,installation_id}' \
                   = deployment.installation_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,guild_id}' \
                   = deployment.guild_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,ruleset_key}' \
                   = deployment.ruleset_key \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,authority,binding_revision}' \
                   = deployment.binding_revision::TEXT \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{intent,evidence,context_fingerprint}' \
                   = deployment.binding_fingerprint \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,request_id}' \
                   = deployment.activation_request_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,guild_id}' \
                   = deployment.guild_id \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,ruleset_key}' \
                   = deployment.ruleset_key \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,version}' \
                   = deployment.target_version::TEXT \
               AND promotion.record OPERATOR(pg_catalog.#>>) '{stage,activation,target,content_hash}' \
                   = deployment.target_content_hash \
               AND serving.process_instance_id \
                   = deployment.snapshot OPERATOR(pg_catalog.#>>) '{live,process_instance_id}' \
               AND serving.runtime_generation = deployment.runtime_generation \
               AND (NOT serving.connected OR NOT serving.serving \
                    OR serving.expires_at <= pg_catalog.clock_timestamp()) \
               AND NOT EXISTS (SELECT 1 FROM public.runtime_deployments newer \
                   WHERE newer.guild_id = deployment.guild_id \
                     AND newer.ruleset_key = deployment.ruleset_key \
                     AND newer.deployment_id <> deployment.deployment_id \
                     AND newer.phase NOT IN ('live','superseded','cancelled')) \
             ORDER BY serving.expires_at, deployment.updated_at, deployment.deployment_id \
             LIMIT 1 FOR UPDATE OF deployment SKIP LOCKED",
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        let Some(candidate) = candidate else {
            transaction.commit().await.map_err(database)?;
            return Ok(None);
        };
        let request = candidate.recovery_request()?;
        let persisted =
            Self::load_scoped_for_update(&mut transaction, &request.identity.scope).await?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let current = Self::load_serving_lease_for_update(
            &mut transaction,
            persisted.deployment.target().guild_id.to_string(),
            persisted.deployment.target().ruleset_key.as_str(),
        )
        .await?
        .ok_or(RuntimeConvergenceStoreError::ServingLeaseConflict)?;
        let now = Self::mutation_now(&mut transaction).await?;
        let receipt =
            Self::recover_locked(&mut transaction, request, persisted, current, now).await?;
        transaction.commit().await.map_err(database)?;
        Ok(Some(receipt))
    }

    async fn replay_live(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        request: &SubmitLiveAttestationV1,
        persisted: &crate::row::PersistedDeployment,
        serving: &ServingLeaseRow,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(MutationReceiptV1, ServingLeaseReceiptV1), RuntimeConvergenceStoreError> {
        let snapshot = persisted.deployment.snapshot();
        let live =
            snapshot
                .live
                .clone()
                .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "Live evidence is missing",
                ))?;
        if request.expected_revision.next().ok() != Some(snapshot.revision)
            || request.runtime_generation != snapshot.runtime_generation
            || request.gateway_ready != live.gateway_ready
        {
            return Err(RuntimeConvergenceStoreError::AttestationConflict);
        }
        let record = AttestationRecordV1 {
            live: live.clone(),
            runtime_build_revision: request.metadata.runtime_build_revision.clone(),
            panel_report_digest: request.metadata.panel_report_digest.clone(),
            gateway_shard_id: request.metadata.gateway_shard_id.clone(),
            controller_fencing_token: request.fencing_token,
            deployment_revision: snapshot.revision,
        };
        let attestation_id =
            AttestationIdV1::from(live_attestation_digest(&record).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidInput("Live attestation serialization")
            })?);
        if persisted.live_attestation_id.as_ref() != Some(&attestation_id) {
            return Err(RuntimeConvergenceStoreError::AttestationConflict);
        }
        let attestation =
            Self::load_attestation(transaction, &request.scope, &attestation_id, false)
                .await?
                .ok_or(RuntimeConvergenceStoreError::AttestationConflict)?;
        if attestation.record != record {
            return Err(RuntimeConvergenceStoreError::AttestationConflict);
        }
        let exact_serving = serving.tenant_id == request.scope.tenant_id.as_str()
            && serving.installation_id == request.scope.installation_id.as_str()
            && serving.deployment_id == request.scope.deployment_id.as_str()
            && serving.attestation_id == attestation_id.as_str()
            && serving.process_instance_id == live.process_instance_id.as_str()
            && serving.runtime_generation == runtime_i64(live.runtime_generation.get())?
            && serving.connected
            && serving.serving
            && serving.expires_at > now;
        if !exact_serving {
            return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
        }
        Ok((
            MutationReceiptV1 {
                outcome: TransitionOutcomeV1::Replayed {
                    revision: snapshot.revision,
                },
                snapshot,
            },
            serving_receipt(&request.scope, serving)?,
        ))
    }

    pub(super) async fn load_attestation(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        scope: &crate::RuntimeDeploymentScopeV1,
        attestation_id: &AttestationIdV1,
        lock: bool,
    ) -> Result<Option<crate::row::PersistedAttestation>, RuntimeConvergenceStoreError> {
        let suffix = if lock { " FOR KEY SHARE" } else { "" };
        let query = format!(
            "SELECT {ATTESTATION_COLUMNS} FROM public.runtime_attestations \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 \
               AND attestation_id = $4{suffix}"
        );
        sqlx::query_as::<_, AttestationRow>(&query)
            .bind(scope.tenant_id.as_str())
            .bind(scope.installation_id.as_str())
            .bind(scope.deployment_id.as_str())
            .bind(attestation_id.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(database)?
            .map(AttestationRow::decode)
            .transpose()
    }

    async fn insert_attestation(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        scope: &crate::RuntimeDeploymentScopeV1,
        attestation_id: &AttestationIdV1,
        record: &AttestationRecordV1,
        snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    ) -> Result<(), RuntimeConvergenceStoreError> {
        let live = &record.live;
        sqlx::query(
            "INSERT INTO public.runtime_attestations (attestation_id, attestation_digest, deployment_id, \
             deployment_revision, tenant_id, installation_id, promotion_id, activation_request_id, \
             guild_id, ruleset_key, target_version, target_content_hash, binding_revision, \
             binding_fingerprint, runtime_generation, controller_fencing_token, \
             process_instance_id, runtime_build_revision, panel_certificate_id, \
             panel_report_digest, gateway_shard_id, gateway_ready_kind, gateway_ready_at, \
             certified_at, record_format_version, record, created_at) \
             VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
                     $15, $16, $17, $18, $19, $20, $21, $22, $23, 1, $24, $23)",
        )
        .bind(attestation_id.as_str())
        .bind(scope.deployment_id.as_str())
        .bind(runtime_i64(record.deployment_revision.get())?)
        .bind(scope.tenant_id.as_str())
        .bind(scope.installation_id.as_str())
        .bind(snapshot.identity.promotion_id.as_str())
        .bind(snapshot.identity.activation_request_id.as_str())
        .bind(live.target.guild_id.to_string())
        .bind(live.target.ruleset_key.as_str())
        .bind(i64::from(live.target.version.get()))
        .bind(live.target.content_hash.to_hex())
        .bind(runtime_i64(live.target.binding_revision.get())?)
        .bind(live.target.binding_fingerprint.as_str())
        .bind(runtime_i64(live.runtime_generation.get())?)
        .bind(runtime_i64(record.controller_fencing_token.get())?)
        .bind(live.process_instance_id.as_str())
        .bind(record.runtime_build_revision.as_str())
        .bind(live.panel_certificate.certificate_id.as_str())
        .bind(record.panel_report_digest.as_str())
        .bind(record.gateway_shard_id.as_str())
        .bind(gateway_ready_kind_name(live.gateway_ready.kind))
        .bind(live.gateway_ready.ready_at)
        .bind(live.certified_at)
        .bind(Json(serde_json::to_value(record).map_err(|_| {
            RuntimeConvergenceStoreError::InvalidInput("Live attestation serialization")
        })?))
        .execute(&mut **transaction)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                RuntimeConvergenceStoreError::AttestationConflict
            } else {
                database(error)
            }
        })?;
        Ok(())
    }

    async fn load_serving_lease_for_update(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        guild_id: String,
        ruleset_key: &str,
    ) -> Result<Option<ServingLeaseRow>, RuntimeConvergenceStoreError> {
        sqlx::query_as::<_, ServingLeaseRow>(&format!(
            "SELECT {SERVING_LEASE_COLUMNS} FROM public.runtime_serving_leases \
             WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE"
        ))
        .bind(guild_id)
        .bind(ruleset_key)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database)
    }
}

#[derive(sqlx::FromRow)]
struct StaleLiveCandidateRow {
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    deployment_revision: i64,
    attestation_id: String,
    process_instance_id: String,
    lease_epoch: i64,
    serving_revision: i64,
}

impl StaleLiveCandidateRow {
    fn recovery_request(self) -> Result<RecoverStaleLiveV1, RuntimeConvergenceStoreError> {
        let scope = crate::RuntimeDeploymentScopeV1 {
            tenant_id: automation_runtime_convergence::TenantId::parse(self.tenant_id).map_err(
                |_| RuntimeConvergenceStoreError::InvalidPersistedState("stale Live tenant"),
            )?,
            installation_id: automation_runtime_convergence::InstallationId::parse(
                self.installation_id,
            )
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("stale Live installation")
            })?,
            deployment_id: automation_runtime_convergence::DeploymentId::parse(self.deployment_id)
                .map_err(|_| {
                    RuntimeConvergenceStoreError::InvalidPersistedState("stale Live deployment")
                })?,
        };
        Ok(RecoverStaleLiveV1 {
            identity: ServingLeaseIdentityV1 {
                scope,
                attestation_id: AttestationIdV1::parse(self.attestation_id).map_err(|_| {
                    RuntimeConvergenceStoreError::InvalidPersistedState("stale Live attestation")
                })?,
                process_instance_id: automation_runtime_convergence::ProcessInstanceId::parse(
                    self.process_instance_id,
                )
                .map_err(|_| {
                    RuntimeConvergenceStoreError::InvalidPersistedState("stale Live process")
                })?,
                lease_epoch: crate::row::positive_u64(self.lease_epoch, "stale Live lease epoch")?,
                expected_revision: crate::row::positive_u64(
                    self.serving_revision,
                    "stale Live serving revision",
                )?,
            },
            expected_deployment_revision: automation_runtime_convergence::DeploymentRevision::new(
                crate::row::positive_u64(
                    self.deployment_revision,
                    "stale Live deployment revision",
                )?,
            )
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "stale Live deployment revision",
                )
            })?,
        })
    }
}

fn validate_serving_identity(
    row: &ServingLeaseRow,
    identity: &ServingLeaseIdentityV1,
) -> Result<(), RuntimeConvergenceStoreError> {
    validate_serving_identity_except_revision(row, identity)?;
    if row.checked_revision()? != identity.expected_revision {
        return Err(RuntimeConvergenceStoreError::RevisionConflict);
    }
    Ok(())
}

fn validate_serving_identity_except_revision(
    row: &ServingLeaseRow,
    identity: &ServingLeaseIdentityV1,
) -> Result<(), RuntimeConvergenceStoreError> {
    if row.tenant_id != identity.scope.tenant_id.as_str()
        || row.installation_id != identity.scope.installation_id.as_str()
        || row.deployment_id != identity.scope.deployment_id.as_str()
        || row.attestation_id != identity.attestation_id.as_str()
        || row.process_instance_id != identity.process_instance_id.as_str()
        || row.checked_epoch()? != identity.lease_epoch
    {
        return Err(RuntimeConvergenceStoreError::ServingLeaseConflict);
    }
    Ok(())
}

pub(crate) fn serving_receipt(
    scope: &crate::RuntimeDeploymentScopeV1,
    row: &ServingLeaseRow,
) -> Result<ServingLeaseReceiptV1, RuntimeConvergenceStoreError> {
    let process_instance_id =
        automation_runtime_convergence::ProcessInstanceId::parse(row.process_instance_id.clone())
            .map_err(|_| {
            RuntimeConvergenceStoreError::InvalidPersistedState("serving process identity")
        })?;
    Ok(ServingLeaseReceiptV1 {
        identity: ServingLeaseIdentityV1 {
            scope: scope.clone(),
            attestation_id: AttestationIdV1::parse(row.attestation_id.clone()).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("serving attestation identity")
            })?,
            process_instance_id,
            lease_epoch: row.checked_epoch()?,
            expected_revision: row.checked_revision()?,
        },
        acquired_at: row.acquired_at,
        last_heartbeat_at: row.last_heartbeat_at,
        expires_at: row.expires_at,
        connected: row.connected,
        serving: row.serving,
    })
}

fn map_serving_database(error: sqlx::Error) -> RuntimeConvergenceStoreError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "55006")
    {
        RuntimeConvergenceStoreError::ServingLeaseConflict
    } else {
        database(error)
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23505")
}
