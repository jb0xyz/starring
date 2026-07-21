#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn deployment_status_redacts_controller_failure_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.failure.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-failure-{}", suffix())),
        )
        .await
        .unwrap();
    let runtime_scope = RuntimeDeploymentScopeV1 {
        tenant_id: RuntimeTenantId::parse(fixture.tenant_id.as_str()).unwrap(),
        installation_id: RuntimeInstallationId::parse(fixture.installation_id.as_str()).unwrap(),
        deployment_id: DeploymentId::parse(applied.exact_deployment().deployment_reference())
            .unwrap(),
    };
    let requested = runtime.status(&runtime_scope).await.unwrap();
    let controller = ControllerId::parse(format!("controller-{}", suffix())).unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: runtime_scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &runtime_scope, &claim).await;
    let private_code = sha256_hex(&format!("private-runtime-code:{}", suffix()));
    runtime
        .mutate(SubmitDeploymentMutationV1 {
            scope: runtime_scope,
            expected_revision: ready_revision,
            controller_id: controller,
            fencing_token: claim.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse(format!("failure-{}", suffix())).unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: private_code.clone(),
                message: "private runtime diagnostic".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_secs(1),
            },
        })
        .await
        .unwrap();
    let status = application
        .get_deployment_status(
            &fixture.credential,
            &selector(&fixture),
            authoring_application::RuntimeDeploymentQueryV1 {
                promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        status,
        DeploymentStatusV1::Failed {
            retryable: true,
            failure_code: "gateway_start_failed".to_string(),
        }
    );
    assert!(!format!("{status:?}").contains(&private_code));
}
#[derive(Clone)]
struct RecordingDeploymentStatuses {
    inner: PostgresProductDeploymentStatuses,
    authority_windows: Arc<Mutex<Vec<(CapabilityV1, i64)>>>,
}

impl RecordingDeploymentStatuses {
    fn new(inner: PostgresProductDeploymentStatuses) -> Self {
        Self {
            inner,
            authority_windows: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn authority_windows(&self) -> Vec<(CapabilityV1, i64)> {
        self.authority_windows.lock().unwrap().clone()
    }
}

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for RecordingDeploymentStatuses {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        let evidence = request.evidence();
        self.authority_windows.lock().unwrap().push((
            evidence.capability(),
            (evidence.expires_at() - evidence.observed_at()).num_milliseconds(),
        ));
        self.inner.load_exact_deployment_status(request).await
    }
}

impl DeploymentStatusObservationPort<FreshDiscordAuthorityEvidenceV1>
    for RecordingDeploymentStatuses
{
    async fn load_exact_deployment_observation(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError> {
        let evidence = request.evidence();
        self.authority_windows.lock().unwrap().push((
            evidence.capability(),
            (evidence.expires_at() - evidence.observed_at()).num_milliseconds(),
        ));
        self.inner.load_exact_deployment_observation(request).await
    }
}

#[derive(Clone)]
struct ProjectedDecision {
    projection: ProductDecisionProjectionV1,
}

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for ProjectedDecision {
    async fn load_approval_preview(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        Err(ProductControlPortError::InvalidState)
    }

    async fn load_product_status(
        &self,
        _request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        Ok(self.projection.clone())
    }
}

fn product_runtime_scope(
    fixture: &Fixture,
    exact: &ExactDeploymentSelectorV1,
) -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: RuntimeTenantId::parse(fixture.tenant_id.as_str()).unwrap(),
        installation_id: RuntimeInstallationId::parse(fixture.installation_id.as_str()).unwrap(),
        deployment_id: DeploymentId::parse(exact.deployment_reference()).unwrap(),
    }
}

async fn mutate_product_runtime(
    runtime: &PostgresRuntimeConvergence,
    scope: &RuntimeDeploymentScopeV1,
    expected_revision: DeploymentRevision,
    controller_id: &ControllerId,
    fencing_token: FencingToken,
    runtime_generation: RuntimeGeneration,
    mutation: DeploymentMutationV1,
) -> DeploymentRevision {
    runtime
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope.clone(),
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

async fn advance_product_runtime_to_ready(
    runtime: &PostgresRuntimeConvergence,
    scope: &RuntimeDeploymentScopeV1,
    claim: &automation_runtime_convergence_postgres::ClaimReceiptV1,
) -> DeploymentRevision {
    let target = claim.snapshot.target.clone();
    let generation = claim.snapshot.runtime_generation;
    let mut revision = mutate_product_runtime(
        runtime,
        scope,
        claim.snapshot.revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target.clone(),
            runtime_generation: generation,
            observed_runtime: None,
            checked_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: generation,
            drained_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(
                claim.snapshot.identity.activation_request_id.as_str(),
            )
            .unwrap(),
            target,
            runtime_generation: generation,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: claim.acquired_at,
        }),
    )
    .await
}

async fn certify_product_runtime_live(
    runtime: &PostgresRuntimeConvergence,
    scope: &RuntimeDeploymentScopeV1,
    claim: &automation_runtime_convergence_postgres::ClaimReceiptV1,
    ready_revision: DeploymentRevision,
    serving_lease_for: Duration,
) -> (
    automation_runtime_convergence_postgres::MutationReceiptV1,
    automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    let target = claim.snapshot.target.clone();
    let generation = claim.snapshot.runtime_generation;
    let process_instance_id =
        ProcessInstanceId::parse(format!("product-live-process-{}", suffix())).unwrap();
    let revision = mutate_product_runtime(
        runtime,
        scope,
        ready_revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    let revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse(format!(
                "product-live-certificate-{}",
                suffix()
            ))
            .unwrap(),
            report_digest: automation_runtime_convergence::PanelReportDigestV1::parse(sha256_hex(
                &format!("product-panel-report:{}", suffix()),
            ))
            .unwrap(),
            target: target.clone(),
            runtime_generation: generation,
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
    runtime
        .certify_live(SubmitLiveAttestationV1 {
            scope: scope.clone(),
            expected_revision: revision,
            controller_id: claim.controller_id.clone(),
            fencing_token: claim.fencing_token,
            runtime_generation: generation,
            gateway_ready: GatewayReadyAttestationV1 {
                target,
                runtime_generation: generation,
                process_instance_id,
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: claim.acquired_at,
            },
            metadata: LiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse("product-status-e2e")
                    .unwrap(),
                panel_report_digest: PanelReportDigestV1::parse(sha256_hex(&format!(
                    "product-panel-report:{}",
                    suffix()
                )))
                .unwrap(),
                gateway_shard_id: GatewayShardIdV1::parse("shard:product-status").unwrap(),
            },
            serving_lease_for,
        })
        .await
        .unwrap()
}

fn applied_projection(
    fixture: &Fixture,
    exact: ExactDeploymentSelectorV1,
) -> ProductDecisionProjectionV1 {
    ProductDecisionProjectionV1::from_server_projection(
        fixture.tenant_id.clone(),
        fixture.installation_id.clone(),
        fixture.guild_id,
        fixture.promotion_id.clone(),
        ProductRevisionV1::new(4).unwrap(),
        ProductDecisionPhaseV1::Applied {
            exact_deployment: exact,
        },
    )
}

#[derive(Clone)]
struct RawDeploymentStatusRequest {
    deployment_id: String,
    promotion_id: String,
    desired_target_digest: String,
    tenant_id: String,
    installation_id: String,
    guild_id: String,
    principal_id: String,
    acting_discord_user_id: String,
    product_session_digest: [u8; 32],
}

impl RawDeploymentStatusRequest {
    fn exact(fixture: &Fixture, exact: &ExactDeploymentSelectorV1) -> Self {
        Self {
            deployment_id: exact.deployment_reference().to_string(),
            promotion_id: exact.promotion_id().as_str().to_string(),
            desired_target_digest: exact.target_digest().to_string(),
            tenant_id: fixture.tenant_id.as_str().to_string(),
            installation_id: fixture.installation_id.as_str().to_string(),
            guild_id: fixture.guild_id.to_string(),
            principal_id: fixture.approver_principal.as_str().to_string(),
            acting_discord_user_id: fixture.approver_user.to_string(),
            product_session_digest: fixture.session_digest,
        }
    }
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct RawDeploymentStatusEnvelopeShape {
    request_outcome: String,
    payload_is_empty: bool,
    database_now_is_present: bool,
}

async fn read_raw_deployment_status(
    connection: &mut PgConnection,
    request: &RawDeploymentStatusRequest,
) -> Vec<RawDeploymentStatusEnvelopeShape> {
    sqlx::query_as::<_, RawDeploymentStatusEnvelopeShape>(
        "SELECT request_outcome, \
            deployment_projection IS NULL \
                AND activation_projection IS NULL \
                AND promotion_projection IS NULL \
                AND tenant_lifecycle_state IS NULL \
                AND installation_projection IS NULL \
                AND historical_authority_projection IS NULL \
                AND current_authority_projection IS NULL \
                AND active_target_version IS NULL \
                AND artifact_projection IS NULL \
                AND attestation_projection IS NULL \
                AND serving_projection IS NULL AS payload_is_empty, \
            database_now IS NOT NULL AS database_now_is_present \
         FROM public.starring_product_deployment_status_read_v1(\
            $1, $2, $3, $4, $5, $6, $7, $8, $9) \
         LIMIT 2",
    )
    .bind(&request.deployment_id)
    .bind(&request.promotion_id)
    .bind(&request.desired_target_digest)
    .bind(&request.tenant_id)
    .bind(&request.installation_id)
    .bind(&request.guild_id)
    .bind(&request.principal_id)
    .bind(&request.acting_discord_user_id)
    .bind(request.product_session_digest.as_slice())
    .fetch_all(connection)
    .await
    .unwrap()
}

async fn applied_status_reader_fixture(pool: &PgPool) -> (Fixture, ExactDeploymentSelectorV1) {
    let fixture = seed_fixture(pool).await;
    let decisions = product_decisions(pool);
    approve_fixture(pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.raw-status.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-raw-status-{}", suffix())),
        )
        .await
        .unwrap();
    (fixture, applied.exact_deployment().clone())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_requires_exact_attestation_and_connected_unexpired_serving() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(500),
            lock_timeout: Duration::from_millis(250),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let deployments =
        RecordingDeploymentStatuses::new(PostgresProductDeploymentStatuses::new(pool.clone()));
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let apply_key = format!("apply-live-{}", suffix());
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.live.first.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &apply_key),
        )
        .await
        .unwrap();
    assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
    assert_eq!(
        application
            .apply(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("apply.live.replay.{}", suffix())).unwrap(),
                &selector(&fixture),
                apply_command(&fixture, &apply_key),
            )
            .await
            .unwrap()
            .status(),
        ProductStatusV1::RuntimePending
    );
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );
    let scope = product_runtime_scope(&fixture, applied.exact_deployment());
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!("product-live-controller-{}", suffix()))
                .unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &scope, &claim).await;
    let (live, serving) = certify_product_runtime_live(
        &runtime,
        &scope,
        &claim,
        ready_revision,
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Live {
            attestation_revision: NonZeroU64::new(live.snapshot.revision.get()).unwrap(),
        }
    );
    let live_observation = application
        .get_deployment_status_observation(
            &fixture.credential,
            &selector(&fixture),
            authoring_application::RuntimeDeploymentQueryV1 {
                promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        live_observation.status(),
        &DeploymentStatusV1::Live {
            attestation_revision: NonZeroU64::new(live.snapshot.revision.get()).unwrap(),
        }
    );
    assert_eq!(live_observation.decision().revision().get(), 4);
    assert!(matches!(
        live_observation.decision().phase(),
        ProductDecisionPhaseV1::Applied { .. }
    ));
    let runtime_observation = live_observation.deployment().unwrap();
    assert_eq!(
        runtime_observation.last_heartbeat_at(),
        Some(serving.last_heartbeat_at.into())
    );
    assert_eq!(
        runtime_observation.lease_expires_at(),
        Some(serving.expires_at.into())
    );
    assert!(runtime_observation.observed_at() >= live_observation.decision_observed_at());
    assert!(runtime_observation.observed_at() < SystemTime::from(serving.expires_at));
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );
    let recovered = runtime
        .recover_stale_live(RecoverStaleLiveV1 {
            identity: serving.identity,
            expected_deployment_revision: live.snapshot.revision,
        })
        .await
        .unwrap();
    let recovered_claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: recovered.snapshot.revision,
            controller_id: ControllerId::parse(format!(
                "product-live-recovery-controller-{}",
                suffix()
            ))
            .unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let (recovered_live, recovered_serving) = certify_product_runtime_live(
        &runtime,
        &scope,
        &recovered_claim,
        recovered_claim.snapshot.revision,
        Duration::from_secs(45),
    )
    .await;
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Live {
            attestation_revision: NonZeroU64::new(recovered_live.snapshot.revision.get()).unwrap(),
        }
    );
    runtime
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: recovered_serving.identity,
        })
        .await
        .unwrap();
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );
    let authority_windows = deployments.authority_windows();
    assert!(authority_windows.contains(&(CapabilityV1::Apply, 5_000)));
    assert!(authority_windows.contains(&(CapabilityV1::Read, 30_000)));
    assert!(authority_windows.iter().all(|(capability, lifetime)| {
        matches!(
            (capability, lifetime),
            (CapabilityV1::Apply, 5_000) | (CapabilityV1::Read, 30_000)
        )
    }));
    assert!(DiscordAuthorityConfigV1::new(
        Duration::from_secs(2),
        Duration::from_millis(5_001),
        Duration::from_secs(30),
    )
    .is_err());
    assert!(DiscordAuthorityConfigV1::new(
        Duration::from_secs(2),
        Duration::from_secs(5),
        Duration::from_millis(30_001),
    )
    .is_err());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_maps_blocked_failure_to_stable_public_code() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.blocked.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-blocked-{}", suffix())),
        )
        .await
        .unwrap();
    let scope = product_runtime_scope(&fixture, applied.exact_deployment());
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!("product-blocked-{}", suffix())).unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &scope, &claim).await;
    let private_code = sha256_hex(&format!("private-blocked-code:{}", suffix()));
    mutate_product_runtime(
        &runtime,
        &scope,
        ready_revision,
        &claim.controller_id,
        claim.fencing_token,
        claim.snapshot.runtime_generation,
        DeploymentMutationV1::RecordBlockedFailure {
            failure_id: RuntimeFailureId::parse(format!("blocked-{}", suffix())).unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: private_code.clone(),
            message: "private blocked diagnostic".to_string(),
        },
    )
    .await;
    let status = application
        .get_deployment_status(
            &fixture.credential,
            &selector(&fixture),
            authoring_application::RuntimeDeploymentQueryV1 {
                promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        status,
        DeploymentStatusV1::Failed {
            retryable: false,
            failure_code: "runtime_invariant_violation".to_string(),
        }
    );
    assert!(!format!("{status:?}").contains(&private_code));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_fails_closed_for_exact_identity_digest_and_scope_mismatch() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.mismatch.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-mismatch-{}", suffix())),
        )
        .await
        .unwrap();
    let exact = applied.exact_deployment().clone();
    let query = authoring_application::RuntimeDeploymentQueryV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
    };

    let wrong_deployment = ExactDeploymentSelectorV1::from_server_projection(
        fixture.installation_id.clone(),
        fixture.promotion_id.clone(),
        format!("missing-deployment-{}", suffix()),
        exact.target_digest(),
    )
    .unwrap();
    let wrong_deployment_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_deployment),
    };
    let wrong_deployment_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_deployment_decision,
        &deployments,
    );
    assert_eq!(
        wrong_deployment_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query.clone())
            .await
            .unwrap_err(),
        ProductApplicationError::Deployment(DeploymentStatusPortError::NotFound)
    );

    let wrong_digest = ExactDeploymentSelectorV1::from_server_projection(
        fixture.installation_id.clone(),
        fixture.promotion_id.clone(),
        exact.deployment_reference(),
        if exact.target_digest() == "0".repeat(64) {
            "1".repeat(64)
        } else {
            "0".repeat(64)
        },
    )
    .unwrap();
    let wrong_digest_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_digest),
    };
    let wrong_digest_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_digest_decision,
        &deployments,
    );
    assert_eq!(
        wrong_digest_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query.clone())
            .await
            .unwrap_err(),
        ProductApplicationError::Deployment(DeploymentStatusPortError::Indeterminate(
            "runtime deployment status projection is inconsistent".to_string(),
        ))
    );

    let wrong_promotion = ExactDeploymentSelectorV1::from_server_projection(
        fixture.installation_id.clone(),
        PromotionId::parse(&sha256_hex(&format!("wrong-promotion:{}", suffix()))).unwrap(),
        exact.deployment_reference(),
        exact.target_digest(),
    )
    .unwrap();
    let wrong_promotion_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_promotion),
    };
    let wrong_promotion_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_promotion_decision,
        &deployments,
    );
    assert_eq!(
        wrong_promotion_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query.clone())
            .await
            .unwrap_err(),
        ProductApplicationError::InvalidProjection
    );

    let wrong_installation = ExactDeploymentSelectorV1::from_server_projection(
        AutomationInstallationId::parse(&format!("wrong-installation-{}", suffix())).unwrap(),
        fixture.promotion_id.clone(),
        exact.deployment_reference(),
        exact.target_digest(),
    )
    .unwrap();
    let wrong_installation_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_installation),
    };
    let wrong_installation_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_installation_decision,
        &deployments,
    );
    assert_eq!(
        wrong_installation_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query)
            .await
            .unwrap_err(),
        ProductApplicationError::InvalidProjection
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_reader_request_mismatch_is_payload_free() {
    let pool = pool().await;
    let (fixture, exact) = applied_status_reader_fixture(&pool).await;
    let request = RawDeploymentStatusRequest::exact(&fixture, &exact);
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        read_raw_deployment_status(&mut connection, &request).await,
        vec![RawDeploymentStatusEnvelopeShape {
            request_outcome: "exact".to_string(),
            payload_is_empty: false,
            database_now_is_present: true,
        }]
    );

    let mut wrong_promotion = request.clone();
    wrong_promotion.promotion_id = sha256_hex(&format!("wrong-raw-promotion:{}", suffix()));
    let mut wrong_digest = request.clone();
    wrong_digest.desired_target_digest = if request.desired_target_digest == "0".repeat(64) {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    };
    let mut wrong_guild = request.clone();
    wrong_guild.guild_id = fixture.guild_id.0.checked_add(10_000).unwrap().to_string();

    for mismatch in [wrong_promotion, wrong_digest, wrong_guild] {
        assert_eq!(
            read_raw_deployment_status(&mut connection, &mismatch).await,
            vec![RawDeploymentStatusEnvelopeShape {
                request_outcome: "request_mismatch".to_string(),
                payload_is_empty: true,
                database_now_is_present: true,
            }]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_reader_denies_enumeration_and_inactive_session_state() {
    let pool = pool().await;
    let (fixture, exact) = applied_status_reader_fixture(&pool).await;
    let request = RawDeploymentStatusRequest::exact(&fixture, &exact);
    let mut connection = pool.acquire().await.unwrap();

    let mut wrong_deployment = request.clone();
    wrong_deployment.deployment_id = format!("missing-deployment-{}", suffix());
    let mut wrong_tenant = request.clone();
    wrong_tenant.tenant_id = format!("missing-tenant-{}", suffix());
    let mut wrong_installation = request.clone();
    wrong_installation.installation_id = format!("missing-installation-{}", suffix());
    let mut wrong_principal = request.clone();
    wrong_principal.principal_id = format!("missing-principal-{}", suffix());
    let mut wrong_user = request.clone();
    wrong_user.acting_discord_user_id = fixture
        .approver_user
        .0
        .checked_add(10_000)
        .unwrap()
        .to_string();
    let mut wrong_session = request.clone();
    wrong_session.product_session_digest[0] ^= 0xff;

    for unauthorized in [
        wrong_deployment,
        wrong_tenant,
        wrong_installation,
        wrong_principal,
        wrong_user,
        wrong_session,
    ] {
        assert!(read_raw_deployment_status(&mut connection, &unauthorized)
            .await
            .is_empty());
    }
    drop(connection);

    let mut disabled = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE public.product_principals \
         SET disabled = TRUE, \
             identity_revision = identity_revision + 1, \
             updated_at = GREATEST(\
                pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond'\
             ) \
         WHERE principal_id = $1",
    )
    .bind(fixture.approver_principal.as_str())
    .execute(&mut *disabled)
    .await
    .unwrap();
    assert!(read_raw_deployment_status(&mut disabled, &request)
        .await
        .is_empty());
    disabled.rollback().await.unwrap();

    let mut revoked = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE public.product_auth_sessions \
         SET revoked_at = GREATEST(pg_catalog.clock_timestamp(), last_seen_at), \
             revocation_reason = 'status_security_test' \
         WHERE session_digest = $1",
    )
    .bind(request.product_session_digest.as_slice())
    .execute(&mut *revoked)
    .await
    .unwrap();
    assert!(read_raw_deployment_status(&mut revoked, &request)
        .await
        .is_empty());
    revoked.rollback().await.unwrap();

    let mut expired = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *expired)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.product_auth_sessions \
         SET authenticated_at = observation.captured_at - INTERVAL '2 hours', \
             created_at = observation.captured_at - INTERVAL '2 hours', \
             last_seen_at = observation.captured_at - INTERVAL '90 minutes', \
             idle_expires_at = observation.captured_at - INTERVAL '80 minutes', \
             absolute_expires_at = observation.captured_at - INTERVAL '60 minutes' \
         FROM (SELECT pg_catalog.clock_timestamp() AS captured_at) AS observation \
         WHERE session_digest = $1",
    )
    .bind(request.product_session_digest.as_slice())
    .execute(&mut *expired)
    .await
    .unwrap();
    assert!(read_raw_deployment_status(&mut expired, &request)
        .await
        .is_empty());
    expired.rollback().await.unwrap();

    let mut oauth_unbound = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *oauth_unbound)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.product_auth_sessions \
         SET oauth_state_digest = NULL, \
             revoked_at = GREATEST(pg_catalog.clock_timestamp(), last_seen_at), \
             revocation_reason = 'oauth_binding_removed' \
         WHERE session_digest = $1",
    )
    .bind(request.product_session_digest.as_slice())
    .execute(&mut *oauth_unbound)
    .await
    .unwrap();
    assert!(read_raw_deployment_status(&mut oauth_unbound, &request)
        .await
        .is_empty());
    oauth_unbound.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_reader_direct_login_is_ready_and_returns_pending_without_relation_access() {
    let mut database = isolated_product_control_database("status_reader").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let fixture = seed_fixture(&database.pool).await;
    let decisions = product_decisions(&database.pool);
    approve_fixture(&database.pool, &fixture, &decisions).await;
    let setup_authentication = PostgresAuthentication::new(database.pool.clone());
    let setup_authority = authority_adapter(fixture.clone());
    let setup_deployments = PostgresProductDeploymentStatuses::new(database.pool.clone());
    let setup_application = ProductControlApplication::new(
        &setup_authentication,
        &setup_authority,
        &decisions,
        &setup_deployments,
    );
    let applied = setup_application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.restricted.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-restricted-{}", suffix())),
        )
        .await
        .unwrap();
    assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
    let role_suffix = suffix();
    let owner_role = format!("starring_status_owner_{role_suffix}");
    let reader_v1_role = format!("starring_status_reader_v1_{role_suffix}");
    let reader_v2_role = format!("starring_status_reader_v2_{role_suffix}");
    let reader_v1_password = database_role_password();
    let reader_v2_password = database_role_password();
    for role in [&owner_role, &reader_v1_role, &reader_v2_role] {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
    }
    sqlx::query(&format!(
        "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for (role, password) in [
        (&reader_v1_role, &reader_v1_password),
        (&reader_v2_role, &reader_v2_password),
    ] {
        let password_literal =
            sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
                .bind(password)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {role} LOGIN PASSWORD {password_literal} \
             NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
             NOBYPASSRLS CONNECTION LIMIT 4"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for relation in [
        "product_control_plane_identity",
        "product_principals",
        "product_auth_sessions",
        "runtime_deployments",
        "activation_requests",
        "authoring_promotions",
        "product_tenants",
        "automation_installations",
        "automation_installation_authority_versions",
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "runtime_attestations",
        "runtime_serving_leases",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for function in [
        "public.starring_product_deployment_status_reader_database_identity_v1()",
        "public.starring_product_deployment_status_read_v1(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BYTEA)",
        "public.starring_product_deployment_status_reader_database_identity_v2()",
        "public.starring_product_deployment_status_read_core_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BYTEA)",
        "public.starring_product_deployment_status_read_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BYTEA)",
        "public.validate_runtime_deployment_projection()",
        "public.validate_runtime_convergence_attempt_projection()",
        "public.enforce_runtime_deployment_policy_shadow()",
        "public.guard_runtime_ruleset_artifact_transition()",
        "public.reject_runtime_deployment_delete()",
        "public.validate_runtime_attestation_projection()",
        "public.validate_runtime_attestation_attempt_projection()",
        "public.reject_immutable_product_row()",
        "public.validate_runtime_serving_lease_transition()",
        "public.reject_runtime_serving_lease_delete()",
        "public.reject_ruleset_artifact_mutation()",
        "public.starring_canonical_json_v1(JSONB)",
        "public.starring_ruleset_content_hash_v1(BIGINT,JSONB)",
    ] {
        sqlx::query(&format!(
            "ALTER FUNCTION {function} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "REVOKE ALL ON DATABASE {} FROM PUBLIC",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
        .execute(&database.pool)
        .await
        .unwrap();
    for role in [&reader_v1_role, &reader_v2_role] {
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {} TO {role}",
            database.name
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner_role}, {reader_v1_role}, {reader_v2_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION \
         public.starring_product_deployment_status_reader_database_identity_v1(), \
         public.starring_product_deployment_status_read_v1( \
          TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BYTEA) TO {reader_v1_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION \
         public.starring_product_deployment_status_reader_database_identity_v2(), \
         public.starring_product_deployment_status_read_v2( \
          TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BYTEA) TO {reader_v2_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let reader_v1_pool =
        database_role_login_pool(&database.name, &reader_v1_role, &reader_v1_password).await;
    let reader_v2_pool =
        database_role_login_pool(&database.name, &reader_v2_role, &reader_v2_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let deployments = PostgresProductDeploymentStatuses::new(reader_v1_pool.clone());
        deployments.verify_readiness().await.unwrap();
        let operational_deployments = authoring_application_postgres::
            PostgresProductDeploymentOperationalStatusesV2::new(reader_v2_pool.clone());
        operational_deployments.verify_readiness().await.unwrap();
        let authentication = ClaimsAuthentication {
            claims: authoring_application::AuthenticationClaimsV1::from_authentication(
                fixture.approver_principal.clone(),
                authoring_application::AuthenticatedSessionFingerprintV1::from_sha256_digest(
                    fixture.session_digest,
                ),
            ),
        };
        let authority = authority_adapter(fixture.clone());
        let projected = ProjectedDecision {
            projection: applied_projection(&fixture, applied.exact_deployment().clone()),
        };
        let application =
            ProductControlApplication::new(&authentication, &authority, &projected, &deployments);
        let operational_application = ProductControlApplication::new(
            &authentication,
            &authority,
            &projected,
            &operational_deployments,
        );
        let mut writer_lock = database.pool.begin().await.unwrap();
        sqlx::query(&format!("SET LOCAL ROLE {owner_role}"))
            .execute(&mut *writer_lock)
            .await
            .unwrap();
        sqlx::query(
            "SELECT deployment_id FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE",
        )
        .bind(applied.exact_deployment().deployment_reference())
        .execute(&mut *writer_lock)
        .await
        .unwrap();
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(1),
                application.get_deployment_status(
                    &fixture.credential,
                    &selector(&fixture),
                    authoring_application::RuntimeDeploymentQueryV1 {
                        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                    },
                ),
            )
            .await
            .expect("status reader must not wait for a runtime row lock")
            .unwrap(),
            DeploymentStatusV1::Pending
        );
        let operational = tokio::time::timeout(
            Duration::from_secs(1),
            operational_application.get_deployment_operational_status_v2(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            ),
        )
        .await
        .expect("operational status reader must not wait for a runtime row lock")
        .unwrap();
        assert_eq!(operational.status(), &DeploymentStatusV1::Pending);
        let operational_observation = operational.deployment().unwrap();
        assert_eq!(
            operational_observation.phase(),
            authoring_application::DeploymentConvergencePhaseV2::Requested
        );
        assert_eq!(operational_observation.current_attempt(), 0);
        assert_eq!(operational_observation.last_failure_attempt(), None);
        assert_eq!(operational_observation.retry(), None);
        assert_eq!(operational_observation.operator_action(), None);
        assert_eq!(operational_observation.attestation(), None);
        assert_eq!(
            operational_observation.serving(),
            authoring_application::DeploymentServingFreshnessV2::NotExpected
        );
        writer_lock.rollback().await.unwrap();
        let raw_request = RawDeploymentStatusRequest::exact(&fixture, applied.exact_deployment());
        let mut open_status_read = reader_v1_pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY")
            .execute(&mut *open_status_read)
            .await
            .unwrap();
        let row_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM \
             public.starring_product_deployment_status_read_v1(\
                $1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&raw_request.deployment_id)
        .bind(&raw_request.promotion_id)
        .bind(&raw_request.desired_target_digest)
        .bind(&raw_request.tenant_id)
        .bind(&raw_request.installation_id)
        .bind(&raw_request.guild_id)
        .bind(&raw_request.principal_id)
        .bind(&raw_request.acting_discord_user_id)
        .bind(raw_request.product_session_digest.as_slice())
        .fetch_one(&mut *open_status_read)
        .await
        .unwrap();
        assert_eq!(row_count, 1);
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut writer = database.pool.begin().await.unwrap();
            sqlx::query(&format!("SET LOCAL ROLE {owner_role}"))
                .execute(&mut *writer)
                .await
                .unwrap();
            sqlx::query(
                "SELECT deployment_id FROM public.runtime_deployments \
                 WHERE deployment_id = $1 FOR UPDATE",
            )
            .bind(applied.exact_deployment().deployment_reference())
            .execute(&mut *writer)
            .await
            .unwrap();
            writer.rollback().await.unwrap();
        })
        .await
        .expect("an open status snapshot must not block a runtime row lock");
        open_status_read.rollback().await.unwrap();
        for statement in [
            "SELECT deployment_id FROM public.runtime_deployments LIMIT 1",
            "INSERT INTO public.runtime_deployments DEFAULT VALUES",
            "UPDATE public.runtime_deployments SET phase = phase WHERE FALSE",
            "DELETE FROM public.runtime_deployments WHERE FALSE",
            "TRUNCATE TABLE public.runtime_deployments",
            "CREATE TABLE public.forbidden_status_table (value INTEGER)",
            "CREATE TEMPORARY TABLE forbidden_status_temp (value INTEGER)",
            "CREATE SCHEMA forbidden_status_schema",
            "SELECT public.starring_product_apply_executor_database_identity_v1()",
            "SELECT public.starring_product_deployment_status_reader_database_identity_v2()",
        ] {
            assert_database_permission_denied(&reader_v1_pool, statement).await;
        }
        for statement in [
            "SELECT deployment_id FROM public.runtime_deployments LIMIT 1",
            "UPDATE public.runtime_deployments SET phase = phase WHERE FALSE",
            "CREATE TABLE public.forbidden_operational_status_table (value INTEGER)",
            "SELECT public.starring_product_deployment_status_reader_database_identity_v1()",
            "SELECT * FROM public.starring_product_deployment_status_read_core_v2(\
                'x','x','x','x','x','x','x','x','x'::BYTEA)",
        ] {
            assert_database_permission_denied(&reader_v2_pool, statement).await;
        }
    })
    .catch_unwind()
    .await;
    reader_v1_pool.close().await;
    reader_v2_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    for role in [&reader_v1_role, &reader_v2_role, &owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
