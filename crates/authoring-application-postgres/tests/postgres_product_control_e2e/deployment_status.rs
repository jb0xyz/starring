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
    let deployments = PostgresProductDeploymentStatuses::new(runtime.clone());
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
        RecordingDeploymentStatuses::new(PostgresProductDeploymentStatuses::new(runtime.clone()));
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
    let deployments = PostgresProductDeploymentStatuses::new(runtime.clone());
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
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let deployments = PostgresProductDeploymentStatuses::new(runtime);
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
