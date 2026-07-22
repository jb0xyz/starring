async fn certify_live_with_concurrent_active_drift(
    pool: &PgPool,
    adapter: &PostgresRuntimeConvergence,
    request: SubmitLiveAttestationV1,
) -> (
    automation_runtime_convergence_postgres::MutationReceiptV1,
    automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions (guild_id, ruleset_key, version, \
         schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, 1, \
          pg_catalog.jsonb_build_object('version', 2, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(NEXT_CONTENT_HASH)
    .bind(PRINCIPAL)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE FUNCTION public.runtime_test_block_deployment_update() RETURNS TRIGGER \
         LANGUAGE plpgsql AS $function$ BEGIN PERFORM pg_catalog.pg_advisory_xact_lock(9200101); \
         RETURN NEW; END; $function$",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER zz_runtime_test_block_deployment_update \
         BEFORE UPDATE ON public.runtime_deployments FOR EACH ROW \
         EXECUTE FUNCTION public.runtime_test_block_deployment_update()",
    )
    .execute(pool)
    .await
    .unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(9200101)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let certifying_adapter = adapter.clone();
    let certifying = tokio::spawn(async move { certifying_adapter.certify_live(request).await });
    wait_for_ungranted_locks(pool, 1).await;
    let drift_pool = pool.clone();
    let drifting =
        tokio::spawn(async move { corrupt_active_pointer_for_test(&drift_pool, 2).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!drifting.is_finished());
    let released_at = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    let certified = certifying.await.unwrap().unwrap();
    let drifted_at = drifting.await.unwrap().unwrap();
    assert!(drifted_at >= released_at);
    sqlx::query(
        "DROP TRIGGER zz_runtime_test_block_deployment_update ON public.runtime_deployments",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION public.runtime_test_block_deployment_update()")
        .execute(pool)
        .await
        .unwrap();
    let superseded = adapter.status(&scope()).await.unwrap();
    assert_eq!(
        superseded.availability,
        DeploymentAvailabilityV1::Superseded
    );
    assert_eq!(superseded.reason_code, "active_target_changed");
    sqlx::query(
        "UPDATE public.automation_ruleset_activations SET active_version = 1 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(pool)
    .await
    .unwrap();
    certified
}

async fn corrupt_active_pointer_for_test(
    pool: &PgPool,
    version: i64,
) -> Result<DateTime<Utc>, sqlx::Error> {
    let mut corruption = pool.begin().await?;
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *corruption)
        .await?;
    let changed_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        "UPDATE public.automation_ruleset_activations SET active_version = $3 \
         WHERE guild_id = $1 AND ruleset_key = $2 \
         RETURNING pg_catalog.clock_timestamp()",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(version)
    .fetch_one(&mut *corruption)
    .await?;
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *corruption)
        .await?;
    corruption.commit().await?;
    Ok(changed_at)
}

async fn wait_for_ungranted_locks(pool: &PgPool, minimum: i64) {
    for _ in 0..100 {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pg_catalog.pg_stat_activity \
             WHERE datname = pg_catalog.current_database() \
               AND state = 'active' AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if count >= minimum {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected at least {minimum} waiting PostgreSQL locks");
}

async fn assert_concurrent_tenant_suspension(
    pool: &PgPool,
    adapter: &PostgresRuntimeConvergence,
    serving: &automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    let mut serving_blocker = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT deployment_id FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *serving_blocker)
    .await
    .unwrap();
    let status_adapter = adapter.clone();
    let concurrent_status = tokio::spawn(async move { status_adapter.status(&scope()).await });
    wait_for_ungranted_locks(pool, 1).await;
    let suspension_pool = pool.clone();
    let suspending = tokio::spawn(async move {
        sqlx::query(
            "UPDATE public.product_tenants SET lifecycle_state = 'suspended', \
             updated_at = GREATEST(pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
             WHERE tenant_id = $1",
        )
        .bind(TENANT)
        .execute(&suspension_pool)
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!suspending.is_finished());
    serving_blocker.commit().await.unwrap();
    let status_before_suspension = concurrent_status.await.unwrap().unwrap();
    assert_eq!(
        status_before_suspension.availability,
        DeploymentAvailabilityV1::Live
    );
    suspending.await.unwrap().unwrap();
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Blocked);
    assert_eq!(status.reason_code, "product_authority_inactive");
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: serving.identity.clone(),
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ProductAuthorityInactive
    ));
    sqlx::query(
        "UPDATE public.product_tenants SET lifecycle_state = 'active', \
         updated_at = GREATEST(pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
         WHERE tenant_id = $1",
    )
    .bind(TENANT)
    .execute(pool)
    .await
    .unwrap();
}

async fn assert_recovery_and_newer_certification_do_not_deadlock(
    pool: &PgPool,
    adapter: &PostgresRuntimeConvergence,
    current_live: &automation_runtime_convergence_postgres::MutationReceiptV1,
    current_serving: &automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    let next_generation = RuntimeGeneration::new(2).unwrap();
    let previous_runtime = RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        process_instance_id: current_serving.identity.process_instance_id.clone(),
    };
    let next_request = EnqueueDeploymentV1 {
        identity: RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse(NEXT_DEPLOYMENT).unwrap(),
            tenant_id: TenantId::parse(TENANT).unwrap(),
            installation_id: InstallationId::parse(INSTALLATION).unwrap(),
            promotion_id: PromotionId::parse(NEXT_PROMOTION).unwrap(),
            activation_request_id: ActivationRequestId::parse(NEXT_ACTIVATION).unwrap(),
        },
        target: target(),
        runtime_generation: next_generation,
        previous_runtime: Some(previous_runtime.clone()),
        installation_authority_revision: 1,
    };
    seed_next_product_journal(pool, &next_request).await;
    let next = match adapter.enqueue(next_request).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("atomically seeded newer deployment must replay exactly: {outcome:?}"),
    };
    let controller = ControllerId::parse("runtime-pg-controller-next").unwrap();
    let claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: next_scope(),
            expected_revision: next.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let mut revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        claim.snapshot.revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: next_generation,
            observed_runtime: Some(previous_runtime.clone()),
            checked_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: current_serving.identity.clone(),
        })
        .await
        .unwrap();
    let transition_at = disconnected.last_heartbeat_at;
    revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: Some(previous_runtime),
            target_runtime_generation: next_generation,
            drained_at: transition_at,
        }),
    )
    .await;
    revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(NEXT_ACTIVATION).unwrap(),
            target: target(),
            runtime_generation: next_generation,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: transition_at,
        }),
    )
    .await;
    revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    let next_process = ProcessInstanceId::parse("runtime-pg-process-next").unwrap();
    revision = mutate_scoped(
        adapter,
        next_scope(),
        next_generation,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("runtime-pg-panel-next").unwrap(),
            report_digest: automation_runtime_convergence::PanelReportDigestV1::parse(
                "4".repeat(64),
            )
            .unwrap(),
            target: target(),
            runtime_generation: next_generation,
            process_instance_id: next_process.clone(),
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: transition_at,
        }),
    )
    .await;
    let certification = SubmitLiveAttestationV1 {
        scope: next_scope(),
        expected_revision: revision,
        controller_id: controller,
        fencing_token: claim.fencing_token,
        runtime_generation: next_generation,
        gateway_ready: GatewayReadyAttestationV1 {
            target: target(),
            runtime_generation: next_generation,
            process_instance_id: next_process,
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: transition_at,
        },
        metadata: LiveMetadataV1 {
            runtime_build_revision: RuntimeBuildRevisionV1::parse("test-build-next").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
        serving_lease_for: Duration::from_secs(45),
    };
    let recovery = RecoverStaleLiveV1 {
        identity: disconnected.identity,
        expected_deployment_revision: current_live.snapshot.revision,
    };
    let mut serving_blocker = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT deployment_id FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *serving_blocker)
    .await
    .unwrap();
    let recovering_adapter = adapter.clone();
    let recovering =
        tokio::spawn(async move { recovering_adapter.recover_stale_live(recovery).await });
    wait_for_ungranted_locks(pool, 1).await;
    let certifying_adapter = adapter.clone();
    let certifying =
        tokio::spawn(async move { certifying_adapter.certify_live(certification).await });
    wait_for_ungranted_locks(pool, 2).await;
    serving_blocker.commit().await.unwrap();
    let (recovery_result, certification_result) =
        tokio::time::timeout(Duration::from_secs(4), async {
            tokio::join!(recovering, certifying)
        })
        .await
        .expect("recovery and certification concurrency must terminate");
    assert!(matches!(
        recovery_result.unwrap().unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    certification_result.unwrap().unwrap();
    let status = adapter.status(&next_scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
}

struct AuthorityRotation<'a> {
    revision: i64,
    binding_revision: i64,
    resource_bindings: &'a Value,
    binding_fingerprint: &'a str,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
}

async fn rotate_authority(pool: &PgPool, rotation: AuthorityRotation<'_>) {
    let AuthorityRotation {
        revision,
        binding_revision,
        resource_bindings,
        binding_fingerprint,
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
    } = rotation;
    let authority_payload_digest = format!("{:x}", revision + 3).repeat(64);
    let request_digest = format!("{:x}", revision + 8).repeat(64);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions (installation_id, \
         revision, tenant_id, binding_revision, resource_bindings, binding_fingerprint, \
         policy_revision, required_approvals, activation_ttl_seconds, \
         authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(INSTALLATION)
    .bind(revision)
    .bind(TENANT)
    .bind(binding_revision)
    .bind(Json(resource_bindings))
    .bind(binding_fingerprint)
    .bind(policy_revision)
    .bind(required_approvals)
    .bind(activation_ttl_seconds)
    .bind(authority_payload_digest)
    .bind(PRINCIPAL)
    .bind(request_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.automation_installations SET current_authority_revision = $3, \
         updated_at = GREATEST(pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
         WHERE tenant_id = $1 AND installation_id = $2 \
           AND current_authority_revision = $3 - 1",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(revision)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn converge_claimed(
    adapter: &PostgresRuntimeConvergence,
    claim: automation_runtime_convergence_postgres::ClaimReceiptV1,
    process_instance_id: ProcessInstanceId,
) -> (
    automation_runtime_convergence_postgres::MutationReceiptV1,
    automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    converge_claimed_with_lease(
        adapter,
        claim,
        process_instance_id,
        Duration::from_secs(45),
    )
    .await
}

async fn converge_claimed_with_lease(
    adapter: &PostgresRuntimeConvergence,
    claim: automation_runtime_convergence_postgres::ClaimReceiptV1,
    process_instance_id: ProcessInstanceId,
    serving_lease_for: Duration,
) -> (
    automation_runtime_convergence_postgres::MutationReceiptV1,
    automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    let controller_id = claim.controller_id.clone();
    let fencing_token = claim.fencing_token;
    let mut revision = mutate(
        adapter,
        claim.snapshot.revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            observed_runtime: None,
            checked_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::FIRST,
            drained_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("runtime-policy-panel").unwrap(),
            report_digest: automation_runtime_convergence::PanelReportDigestV1::parse(
                "7".repeat(64),
            )
            .unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process_instance_id.clone(),
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: claim.acquired_at,
        }),
    )
    .await;
    adapter
        .certify_live(SubmitLiveAttestationV1 {
            scope: scope(),
            expected_revision: revision,
            controller_id,
            fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            gateway_ready: GatewayReadyAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id,
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: claim.acquired_at,
            },
            metadata: LiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse("policy-build").unwrap(),
                panel_report_digest: PanelReportDigestV1::parse("7".repeat(64)).unwrap(),
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            },
            serving_lease_for,
        })
        .await
        .unwrap()
}

async fn converge_recovered(
    adapter: &PostgresRuntimeConvergence,
    claim: automation_runtime_convergence_postgres::ClaimReceiptV1,
    process_instance_id: ProcessInstanceId,
    build_revision: &str,
    report_digest_character: &str,
    serving_lease_for: Duration,
) -> (
    automation_runtime_convergence_postgres::MutationReceiptV1,
    automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    let controller_id = claim.controller_id.clone();
    let fencing_token = claim.fencing_token;
    let mut revision = mutate(
        adapter,
        claim.snapshot.revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        &controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse(format!(
                "runtime-pg-panel-{}",
                process_instance_id.as_str()
            ))
            .unwrap(),
            report_digest: automation_runtime_convergence::PanelReportDigestV1::parse(
                report_digest_character.repeat(64),
            )
            .unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process_instance_id.clone(),
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: claim.acquired_at,
        }),
    )
    .await;
    adapter
        .certify_live(SubmitLiveAttestationV1 {
            scope: scope(),
            expected_revision: revision,
            controller_id,
            fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            gateway_ready: GatewayReadyAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id,
                kind: GatewayReadyKindV1::DiscordResumed,
                ready_at: claim.acquired_at,
            },
            metadata: LiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse(build_revision).unwrap(),
                panel_report_digest: PanelReportDigestV1::parse(report_digest_character.repeat(64))
                    .unwrap(),
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            },
            serving_lease_for,
        })
        .await
        .unwrap()
}

fn second_claim_fencing(
    second_live: &automation_runtime_convergence_postgres::MutationReceiptV1,
) -> automation_runtime_convergence::FencingToken {
    second_live
        .snapshot
        .last_fencing_token
        .expect("second convergence fencing token")
}

async fn mutate(
    adapter: &PostgresRuntimeConvergence,
    expected_revision: automation_runtime_convergence::DeploymentRevision,
    controller_id: &ControllerId,
    fencing_token: automation_runtime_convergence::FencingToken,
    mutation: DeploymentMutationV1,
) -> automation_runtime_convergence::DeploymentRevision {
    mutate_scoped(
        adapter,
        scope(),
        RuntimeGeneration::FIRST,
        expected_revision,
        controller_id,
        fencing_token,
        mutation,
    )
    .await
}

async fn mutate_scoped(
    adapter: &PostgresRuntimeConvergence,
    scope: RuntimeDeploymentScopeV1,
    runtime_generation: RuntimeGeneration,
    expected_revision: automation_runtime_convergence::DeploymentRevision,
    controller_id: &ControllerId,
    fencing_token: automation_runtime_convergence::FencingToken,
    mutation: DeploymentMutationV1,
) -> automation_runtime_convergence::DeploymentRevision {
    adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope,
            expected_revision,
            controller_id: controller_id.clone(),
            fencing_token,
            runtime_generation,
            mutation,
        })
        .await
        .unwrap()
        .snapshot
        .revision
}

fn scope() -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(TENANT).unwrap(),
        installation_id: InstallationId::parse(INSTALLATION).unwrap(),
        deployment_id: DeploymentId::parse(DEPLOYMENT).unwrap(),
    }
}

fn next_scope() -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(TENANT).unwrap(),
        installation_id: InstallationId::parse(INSTALLATION).unwrap(),
        deployment_id: DeploymentId::parse(NEXT_DEPLOYMENT).unwrap(),
    }
}

fn target() -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GUILD,
        ruleset_key: RULESET.parse().unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(CONTENT_HASH).unwrap(),
        binding_revision: BindingRevision::FIRST,
        binding_fingerprint: ResourceBindingFingerprint::parse(BINDING_FINGERPRINT).unwrap(),
    }
}

fn enqueue_request() -> EnqueueDeploymentV1 {
    EnqueueDeploymentV1 {
        identity: RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse(DEPLOYMENT).unwrap(),
            tenant_id: TenantId::parse(TENANT).unwrap(),
            installation_id: InstallationId::parse(INSTALLATION).unwrap(),
            promotion_id: PromotionId::parse(PROMOTION).unwrap(),
            activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
        },
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        previous_runtime: None,
        installation_authority_revision: 1,
    }
}

async fn seed_product_target(pool: &PgPool) {
    let now = database_now(pool).await;
    let expires_at = now + TimeDelta::hours(1);
    let linked_at = now + TimeDelta::seconds(1);
    let request_digest = "e".repeat(64);
    let approval_payload_digest = "f".repeat(64);
    let approval_context_digest = "1".repeat(64);
    let approval_context = json!({
        "promotion_id": PROMOTION,
        "promotion_request_digest": request_digest,
        "approval_payload_digest": approval_payload_digest,
        "approval_context_digest": approval_context_digest,
        "binding": {
            "revision": 1,
            "required_bindings": [],
            "fingerprint": BINDING_FINGERPRINT
        },
        "baseline": { "state": "absent" },
        "policy": {
            "revision": 1,
            "required_approvals": 1,
            "ttl_seconds": 3600,
            "digest": "2".repeat(64)
        }
    });
    let promotion_record = promotion_record(
        PROMOTION,
        ACTIVATION,
        "9200401",
        now,
        expires_at,
        &request_digest,
        &approval_context,
    );
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("INSERT INTO product_principals (principal_id, discord_user_id) VALUES ($1, $2)")
        .bind(PRINCIPAL)
        .bind("9200201")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO product_tenants (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', 'Runtime PostgreSQL Test')",
    )
    .bind(TENANT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_installations (installation_id, tenant_id, \
         discord_application_id, discord_guild_id, ruleset_key, lifecycle_state, \
         current_authority_revision) VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(INSTALLATION)
    .bind(TENANT)
    .bind("9200301")
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_installation_authority_versions (installation_id, revision, \
         tenant_id, binding_revision, resource_bindings, binding_fingerprint, policy_revision, \
         required_approvals, activation_ttl_seconds, authority_payload_digest, \
         created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, '{}'::JSONB, $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(INSTALLATION)
    .bind(TENANT)
    .bind(BINDING_FINGERPRINT)
    .bind("3".repeat(64))
    .bind(PRINCIPAL)
    .bind("4".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_ruleset_heads (guild_id, ruleset_key, next_version) \
         VALUES ($1, $2, 2)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_ruleset_versions (guild_id, ruleset_key, version, \
         schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, \
          pg_catalog.jsonb_build_object('version', 1, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind("9200201")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_ruleset_activations (guild_id, ruleset_key, active_version) \
         VALUES ($1, $2, 1)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    insert_activation_pending_promotion(
        &mut transaction,
        PROMOTION,
        &request_digest,
        &promotion_record,
    )
    .await;
    sqlx::query(
        "INSERT INTO activation_requests (id, guild_id, ruleset_key, target_version, \
         target_content_hash, requester_id, required_approvals, state, created_at, expires_at, \
         authority_kind, link_state_name, approval_context, link_state, promotion_id, \
         promotion_request_digest, approval_payload_digest, approval_context_digest) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, 'product_authoring', \
                 'unlinked', $8, '{\"state\":\"unlinked\"}'::JSONB, $9, $10, $11, $12)",
    )
    .bind(ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind("9200401")
    .bind(now)
    .bind(expires_at)
    .bind(Json(json!({
        "authority": "product_authoring",
        "context": approval_context
    })))
    .bind(PROMOTION)
    .bind(&request_digest)
    .bind(&approval_payload_digest)
    .bind(&approval_context_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE activation_requests SET link_state_name = 'linked', \
         link_state = $2, linked_at = $3 WHERE id = $1",
    )
    .bind(ACTIVATION)
    .bind(Json(json!({ "state": "linked", "linked_at": linked_at })))
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE activation_requests SET state = 'applied', applied_at = $2, applied_by = $3, \
         completion_kind = 'already_active', activation_notices = '[]'::JSONB WHERE id = $1",
    )
    .bind(ACTIVATION)
    .bind(linked_at)
    .bind("9200501")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let requested_at =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    let prepared = prepare_requested_deployment_v1(enqueue_request(), requested_at).unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(requested_at.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_deployments (deployment_id, tenant_id, installation_id, \
         promotion_id, activation_request_id, installation_authority_revision, guild_id, \
         ruleset_key, target_version, target_content_hash, binding_revision, \
         binding_fingerprint, desired_target_digest, runtime_generation, requested_at, \
         snapshot_format_version, snapshot, revision, phase, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, 1, $8, 1, $9, $10, 1, $11, \
                 1, $12, 1, 'requested', $11, $11)",
    )
    .bind(DEPLOYMENT)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(PROMOTION)
    .bind(ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(prepared.desired_target_digest())
    .bind(requested_at)
    .bind(Json(prepared.snapshot_json().clone()))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', '', TRUE)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn corrupt_seeded_ruleset_artifact(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DISABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DROP CONSTRAINT arv_content_integrity",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE public.automation_ruleset_versions \
         SET definition = pg_catalog.jsonb_build_object(\
          'version', 2, 'panels', '[]'::JSONB, 'modals', '[]'::JSONB, 'rules', '[]'::JSONB) \
         WHERE guild_id = $1 AND ruleset_key = $2 AND version = 1",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ADD CONSTRAINT arv_content_integrity CHECK (canonical_content_hash IS NOT NULL \
          AND canonical_content_hash = content_hash) NOT VALID",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ENABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn replace_seeded_target_with_future_schema(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    for table in [
        "automation_ruleset_versions",
        "activation_requests",
        "authoring_promotions",
        "runtime_deployments",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} DISABLE TRIGGER USER"))
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    let future_hash = sqlx::query_scalar::<_, String>(
        "UPDATE public.automation_ruleset_versions \
         SET schema_version = 2, \
          content_hash = public.starring_ruleset_content_hash_v1(2, definition) \
         WHERE guild_id = $1 AND ruleset_key = $2 AND version = 1 \
         RETURNING content_hash",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET target_content_hash = $2 WHERE id = $1",
    )
    .bind(ACTIVATION)
    .bind(&future_hash)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET record = pg_catalog.jsonb_set(\
          record, '{stage,activation,target,content_hash}', pg_catalog.to_jsonb($2::TEXT), FALSE) \
         WHERE id = $1",
    )
    .bind(PROMOTION)
    .bind(&future_hash)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET target_content_hash = $2, \
          snapshot = pg_catalog.jsonb_set(\
           snapshot, '{target,content_hash}', pg_catalog.to_jsonb($2::TEXT), FALSE) \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(&future_hash)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for table in [
        "runtime_deployments",
        "authoring_promotions",
        "activation_requests",
        "automation_ruleset_versions",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} ENABLE TRIGGER USER"))
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn seed_next_product_journal(pool: &PgPool, request: &EnqueueDeploymentV1) {
    let now = database_now(pool).await;
    let expires_at = now + TimeDelta::hours(1);
    let linked_at = now + TimeDelta::seconds(1);
    let request_digest = "8".repeat(64);
    let approval_payload_digest = "9".repeat(64);
    let approval_context_digest = "0".repeat(64);
    let requester_id = "9200402";
    let approval_context = json!({
        "promotion_id": NEXT_PROMOTION,
        "promotion_request_digest": request_digest,
        "approval_payload_digest": approval_payload_digest,
        "approval_context_digest": approval_context_digest,
        "binding": {
            "revision": 1,
            "required_bindings": [],
            "fingerprint": BINDING_FINGERPRINT
        },
        "baseline": { "state": "absent" },
        "policy": {
            "revision": 1,
            "required_approvals": 1,
            "ttl_seconds": 3600,
            "digest": "5".repeat(64)
        }
    });
    let record = promotion_record(
        NEXT_PROMOTION,
        NEXT_ACTIVATION,
        requester_id,
        now,
        expires_at,
        &request_digest,
        &approval_context,
    );
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    insert_activation_pending_promotion(
        &mut transaction,
        NEXT_PROMOTION,
        &request_digest,
        &record,
    )
    .await;
    sqlx::query(
        "INSERT INTO public.activation_requests (id, guild_id, ruleset_key, target_version, \
         target_content_hash, requester_id, required_approvals, state, created_at, expires_at, \
         authority_kind, link_state_name, approval_context, link_state, promotion_id, \
         promotion_request_digest, approval_payload_digest, approval_context_digest) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, 'product_authoring', \
                 'unlinked', $8, '{\"state\":\"unlinked\"}'::JSONB, $9, $10, $11, $12)",
    )
    .bind(NEXT_ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(requester_id)
    .bind(now)
    .bind(expires_at)
    .bind(Json(json!({
        "authority": "product_authoring",
        "context": approval_context
    })))
    .bind(NEXT_PROMOTION)
    .bind(&request_digest)
    .bind(&approval_payload_digest)
    .bind(&approval_context_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET link_state_name = 'linked', \
         link_state = $2, linked_at = $3 WHERE id = $1",
    )
    .bind(NEXT_ACTIVATION)
    .bind(Json(json!({ "state": "linked", "linked_at": linked_at })))
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'applied', applied_at = $2, \
         applied_by = $3, completion_kind = 'already_active', \
         activation_notices = '[]'::JSONB WHERE id = $1",
    )
    .bind(NEXT_ACTIVATION)
    .bind(linked_at)
    .bind("9200502")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let requested_at =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    let prepared = prepare_requested_deployment_v1(request.clone(), requested_at).unwrap();
    let previous_runtime = prepared.previous_runtime_json().cloned().map(Json);
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(requested_at.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_deployments (deployment_id, tenant_id, installation_id, \
         promotion_id, activation_request_id, installation_authority_revision, guild_id, \
         ruleset_key, target_version, target_content_hash, binding_revision, \
         binding_fingerprint, desired_target_digest, runtime_generation, previous_runtime, \
         requested_at, snapshot_format_version, snapshot, revision, phase, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, 1, $8, 1, $9, $10, 2, $11, $12, \
                 1, $13, 1, 'requested', $12, $12)",
    )
    .bind(NEXT_DEPLOYMENT)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(NEXT_PROMOTION)
    .bind(NEXT_ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(prepared.desired_target_digest())
    .bind(previous_runtime)
    .bind(requested_at)
    .bind(Json(prepared.snapshot_json().clone()))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', '', TRUE)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_search_path_shadow_resistance(pool: &PgPool) {
    sqlx::query("CREATE SCHEMA runtime_shadow")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE FUNCTION runtime_shadow.starring_runtime_lock_current_authority(\
             TEXT, TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, BIGINT, TEXT, BIGINT, TEXT) \
         RETURNS TEXT LANGUAGE SQL AS 'SELECT ''active_mismatch''::TEXT'",
    )
    .execute(pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL search_path = runtime_shadow, public, pg_catalog")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let shadowed = authority_lock_result(&mut transaction, false).await;
    let hardened = authority_lock_result(&mut transaction, true).await;
    assert_eq!(shadowed, "active_mismatch");
    assert_eq!(hardened, "exact");
    transaction.commit().await.unwrap();
}

async fn authority_lock_result(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    qualified: bool,
) -> String {
    let function = if qualified {
        "public.starring_runtime_lock_current_authority"
    } else {
        "starring_runtime_lock_current_authority"
    };
    sqlx::query_scalar::<_, String>(&format!(
        "SELECT {function}($1, $2, $3, $4, 1, $5, $6, 1, $7, 1, $8)"
    ))
    .bind(ACTIVATION)
    .bind(PROMOTION)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .fetch_one(&mut **transaction)
    .await
    .unwrap()
}

async fn assert_adapter_search_path_resistance(connect_options: &PgConnectOptions) {
    let setup_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options.clone())
        .await
        .unwrap();
    sqlx::query(
        "CREATE VIEW runtime_shadow.runtime_deployments AS \
         SELECT * FROM public.runtime_deployments WHERE FALSE",
    )
    .execute(&setup_pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE FUNCTION runtime_shadow.clock_timestamp() RETURNS TIMESTAMPTZ \
         LANGUAGE SQL AS 'SELECT ''2000-01-01T00:00:00Z''::TIMESTAMPTZ'",
    )
    .execute(&setup_pool)
    .await
    .unwrap();
    setup_pool.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET search_path = runtime_shadow, pg_catalog")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(connect_options.clone())
        .await
        .unwrap();
    let adapter = PostgresRuntimeConvergence::new(pool);
    assert!(matches!(
        adapter.enqueue(enqueue_request()).await.unwrap(),
        EnqueueDeploymentOutcomeV1::ExactReplay(_)
    ));
    let status = adapter.status(&scope()).await.unwrap();
    assert!(status.observed_at > Utc::now() - TimeDelta::minutes(1));
}

fn promotion_record(
    promotion_id: &str,
    activation_request_id: &str,
    requester_id: &str,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    request_digest: &str,
    approval_context: &Value,
) -> Value {
    json!({
        "id": promotion_id,
        "request_digest": request_digest,
        "revision": 3,
        "intent": {
            "authority": {
                "tenant_id": TENANT,
                "principal_id": PRINCIPAL,
                "installation_id": INSTALLATION,
                "guild_id": GUILD.to_string(),
                "ruleset_key": RULESET,
                "binding_revision": 1
            },
            "evidence": {
                "context_fingerprint": BINDING_FINGERPRINT
            }
        },
        "stage": {
            "state": "activation_pending",
            "publication": {
                "version": 1,
                "schema_version": 1,
                "content_hash": CONTENT_HASH,
                "disposition": "created",
                "registry_created_by": requester_id
            },
            "activation": {
                "request_id": activation_request_id,
                "target": {
                    "guild_id": GUILD.to_string(),
                    "ruleset_key": RULESET,
                    "version": 1,
                    "content_hash": CONTENT_HASH
                },
                "requester": requester_id,
                "required_approvals": NonZeroU32::new(1).unwrap(),
                "observed_active": null,
                "created_at": created_at,
                "expires_at": expires_at,
                "disposition": "created",
                "request_state_at_journal": "pending",
                "approval_context": approval_context
            }
        },
        "created_at": created_at,
        "updated_at": created_at
    })
}

async fn insert_activation_pending_promotion(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
    request_digest: &str,
    record: &Value,
) {
    let mut prepared = record.clone();
    prepared["revision"] = json!(1);
    prepared["stage"] = json!({"state": "prepared"});
    let mut published = record.clone();
    published["revision"] = json!(2);
    published["stage"] = json!({
        "state": "published",
        "publication": record["stage"]["publication"].clone()
    });
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, installation_id, \
          principal_id, record) VALUES ($1, 1, 1, 'prepared', $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(request_digest)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(PRINCIPAL)
    .bind(Json(&prepared))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 2, stage = 'published', record = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(&published))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 3, stage = 'activation_pending', record = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(record))
    .execute(&mut **transaction)
    .await
    .unwrap();
}

async fn database_now(pool: &PgPool) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap()
}
