fn fixture_operational_deployment_query(
    fixture: &Fixture,
) -> authoring_application::RuntimeDeploymentQueryV1 {
    authoring_application::RuntimeDeploymentQueryV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
    }
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct RawOperationalDeploymentStatusEnvelopeShape {
    request_outcome: String,
    payload_is_empty: bool,
    database_now_is_present: bool,
}

async fn read_raw_operational_deployment_status(
    connection: &mut PgConnection,
    request: &RawDeploymentStatusRequest,
) -> Vec<RawOperationalDeploymentStatusEnvelopeShape> {
    sqlx::query_as::<_, RawOperationalDeploymentStatusEnvelopeShape>(
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
                AND serving_projection IS NULL \
                AND deployment_convergence_attempt_no IS NULL \
                AND deployment_last_failure_attempt_no IS NULL \
                AND attestation_convergence_attempt_no IS NULL \
                AND attestation_record_format_version IS NULL \
                AND attestation_serving_lease_duration_nanos IS NULL \
                AND deployment_last_controller_id IS NULL \
                AND v2_evidence_state IS NULL \
                AND v2_operation_id IS NULL \
                AND v2_intent_fingerprint IS NULL \
                AND v2_certification_intent_bytes IS NULL \
                AND v2_request_digest IS NULL \
                AND v2_request_bytes IS NULL \
                AND v2_live_attestation_bytes IS NULL \
                AND v2_must_commit_before IS NULL \
                AND v2_route_admission IS NULL \
                AND v2_certified_snapshot IS NULL AS payload_is_empty, \
            database_now IS NOT NULL AS database_now_is_present \
         FROM public.starring_product_operational_deployment_status_read_v3(\
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

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_reader_redacts_mismatch_and_denies_scope_enumeration() {
    let pool = pool().await;
    let (fixture, exact) = applied_status_reader_fixture(&pool).await;
    let request = RawDeploymentStatusRequest::exact(&fixture, &exact);
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        read_raw_operational_deployment_status(&mut connection, &request).await,
        vec![RawOperationalDeploymentStatusEnvelopeShape {
            request_outcome: "exact".to_string(),
            payload_is_empty: false,
            database_now_is_present: true,
        }]
    );

    let mut wrong_promotion = request.clone();
    wrong_promotion.promotion_id = sha256_hex(&format!("wrong-v2-promotion:{}", suffix()));
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
            read_raw_operational_deployment_status(&mut connection, &mismatch).await,
            vec![RawOperationalDeploymentStatusEnvelopeShape {
                request_outcome: "request_mismatch".to_string(),
                payload_is_empty: true,
                database_now_is_present: true,
            }]
        );
    }

    let mut wrong_deployment = request.clone();
    wrong_deployment.deployment_id = format!("missing-v2-deployment-{}", suffix());
    let mut wrong_tenant = request.clone();
    wrong_tenant.tenant_id = format!("missing-v2-tenant-{}", suffix());
    let mut wrong_installation = request.clone();
    wrong_installation.installation_id = format!("missing-v2-installation-{}", suffix());
    let mut wrong_principal = request.clone();
    wrong_principal.principal_id = format!("missing-v2-principal-{}", suffix());
    let mut wrong_user = request.clone();
    wrong_user.acting_discord_user_id = fixture
        .approver_user
        .0
        .checked_add(20_000)
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
        assert!(
            read_raw_operational_deployment_status(&mut connection, &unauthorized)
                .await
                .is_empty()
        );
    }
}

struct OperationalLiveSeed {
    fixture: Fixture,
    exact: ExactDeploymentSelectorV1,
    convergence_attempt: NonZeroU32,
    deployment_revision: NonZeroU64,
    last_heartbeat_at: SystemTime,
    lease_expires_at: SystemTime,
}

async fn seed_operational_live(
    pool: &PgPool,
    runtime: &PostgresRuntimeConvergence,
    serving_lease_for: Duration,
) -> OperationalLiveSeed {
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
            &ProductRequestIdV1::parse(&format!("apply.operational.seed.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-operational-seed-{}", suffix())),
        )
        .await
        .unwrap();
    let exact = applied.exact_deployment().clone();
    let scope = product_runtime_scope(&fixture, &exact);
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!("operational-seed-{}", suffix())).unwrap(),
            lease_for: Duration::from_secs(10),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(runtime, &scope, &claim).await;
    let (live, serving) = certify_product_runtime_live(
        runtime,
        &scope,
        &claim,
        ready_revision,
        serving_lease_for,
    )
    .await;
    OperationalLiveSeed {
        fixture,
        exact,
        convergence_attempt: claim.convergence_attempt,
        deployment_revision: NonZeroU64::new(live.snapshot.revision.get()).unwrap(),
        last_heartbeat_at: serving.last_heartbeat_at.into(),
        lease_expires_at: serving.expires_at.into(),
    }
}

async fn load_operational_status(
    pool: &PgPool,
    fixture: &Fixture,
) -> authoring_application::ProductDeploymentOperationalStatusV2 {
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let decisions = product_decisions(pool);
    let deployments =
        authoring_application_postgres::PostgresProductDeploymentOperationalStatusesV2::new(
            pool.clone(),
        );
    ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(fixture),
            fixture_operational_deployment_query(fixture),
        )
        .await
        .unwrap()
}

fn assert_live_attestation(
    observation: &authoring_application::DeploymentOperationalObservationV2,
    seed: &OperationalLiveSeed,
) {
    assert_eq!(
        observation.phase(),
        authoring_application::DeploymentConvergencePhaseV2::Live
    );
    assert_eq!(
        observation.attestation(),
        Some(authoring_application::DeploymentAttestationObservationV2::new(
            seed.deployment_revision,
            seed.convergence_attempt,
        ))
    );
}

impl authoring_application::ProductDecisionObservationPort<FreshDiscordAuthorityEvidenceV1>
    for ProjectedDecision
{
    async fn load_approval_preview_observation(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<authoring_application::ProductApprovalPreviewObservationV1, ProductControlPortError>
    {
        Err(ProductControlPortError::InvalidState)
    }

    async fn load_product_status_observation(
        &self,
        _request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<authoring_application::ProductDecisionObservationV1, ProductControlPortError> {
        Ok(
            authoring_application::ProductDecisionObservationV1::from_server_projection(
                self.projection.clone(),
                UNIX_EPOCH,
            ),
        )
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_tracks_pristine_retry_blocked_and_recovered_attempts() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let setup_deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let setup_application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &setup_deployments);
    let applied = setup_application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.operational.attempt.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-operational-attempt-{}", suffix())),
        )
        .await
        .unwrap();
    let deployments =
        authoring_application_postgres::PostgresProductDeploymentOperationalStatusesV2::new(
            pool.clone(),
        );
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let pristine = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    assert_eq!(pristine.status(), &DeploymentStatusV1::Pending);
    let pristine_runtime = pristine.deployment().unwrap();
    assert_eq!(
        pristine_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::Requested
    );
    assert_eq!(pristine_runtime.current_attempt(), 0);
    assert_eq!(pristine_runtime.last_failure_attempt(), None);
    assert_eq!(pristine_runtime.retry(), None);
    assert_eq!(pristine_runtime.operator_action(), None);

    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let scope = product_runtime_scope(&fixture, applied.exact_deployment());
    let requested = runtime.status(&scope).await.unwrap();
    let first_controller = ControllerId::parse(format!("operational-first-{}", suffix())).unwrap();
    let first_claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: first_controller,
            lease_for: Duration::from_secs(10),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &scope, &first_claim).await;
    let private_retry_code = sha256_hex(&format!("operational-private-retry:{}", suffix()));
    let retry_failure_id = RuntimeFailureId::parse(format!("operational-retry-{}", suffix())).unwrap();
    let retry_failure = runtime
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope.clone(),
            expected_revision: ready_revision,
            controller_id: first_claim.controller_id.clone(),
            fencing_token: first_claim.fencing_token,
            convergence_attempt: first_claim.convergence_attempt,
            runtime_generation: first_claim.snapshot.runtime_generation,
            mutation: DeploymentMutationV1::RecordRetryableFailure {
                failure_id: retry_failure_id,
                kind: RuntimeFailureKindV1::GatewayReadyTimeout,
                code: private_retry_code.clone(),
                message: "private retry diagnostic".to_string(),
                attempt: first_claim.convergence_attempt,
                retry_after: Duration::from_millis(500),
            },
        })
        .await
        .unwrap();
    let waiting = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    assert_eq!(
        waiting.status(),
        &DeploymentStatusV1::Failed {
            retryable: true,
            failure_code: "gateway_ready_timeout".to_string(),
        }
    );
    let waiting_runtime = waiting.deployment().unwrap();
    assert_eq!(
        waiting_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::RetryWaiting
    );
    assert_eq!(waiting_runtime.current_attempt(), 1);
    assert_eq!(waiting_runtime.last_failure_attempt(), Some(NonZeroU32::MIN));
    assert!(matches!(
        waiting_runtime.retry(),
        Some(authoring_application::DeploymentRetryObservationV2::Waiting {
            failure_attempt,
            retry_not_before,
        }) if failure_attempt == NonZeroU32::MIN && retry_not_before > waiting_runtime.observed_at()
    ));
    assert!(!format!("{waiting:?}").contains(&private_retry_code));

    tokio::time::sleep(Duration::from_millis(650)).await;
    let due = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    let due_runtime = due.deployment().unwrap();
    assert_eq!(
        due_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::RetryDue
    );
    assert!(matches!(
        due_runtime.retry(),
        Some(authoring_application::DeploymentRetryObservationV2::Due {
            failure_attempt,
            retry_not_before,
        }) if failure_attempt == NonZeroU32::MIN && retry_not_before <= due_runtime.observed_at()
    ));

    let second_controller = ControllerId::parse(format!("operational-second-{}", suffix())).unwrap();
    let second_claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: retry_failure.snapshot.revision,
            controller_id: second_controller.clone(),
            lease_for: Duration::from_secs(10),
        })
        .await
        .unwrap();
    assert_eq!(second_claim.convergence_attempt.get(), 2);
    let mutation_guard = ProductRuntimeMutationGuard::from_claim(&scope, &second_claim);
    let resumed_revision = mutate_product_runtime(
        &runtime,
        &mutation_guard,
        second_claim.snapshot.revision,
        DeploymentMutationV1::ResumeRuntimePending,
    )
    .await;
    let panels_revision = mutate_product_runtime(
        &runtime,
        &mutation_guard,
        resumed_revision,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    let blocked_failure_id =
        RuntimeFailureId::parse(format!("operational-blocked-{}", suffix())).unwrap();
    let private_blocked_code = sha256_hex(&format!("operational-private-blocked:{}", suffix()));
    let blocked = runtime
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope.clone(),
            expected_revision: panels_revision,
            controller_id: second_controller,
            fencing_token: second_claim.fencing_token,
            convergence_attempt: second_claim.convergence_attempt,
            runtime_generation: second_claim.snapshot.runtime_generation,
            mutation: DeploymentMutationV1::RecordBlockedFailure {
                failure_id: blocked_failure_id.clone(),
                kind: RuntimeFailureKindV1::InvariantViolation,
                code: private_blocked_code.clone(),
                message: "private blocked diagnostic".to_string(),
            },
        })
        .await
        .unwrap();
    let blocked_status = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    let blocked_runtime = blocked_status.deployment().unwrap();
    assert_eq!(
        blocked_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::OperatorBlocked
    );
    assert_eq!(blocked_runtime.current_attempt(), 2);
    assert_eq!(
        blocked_runtime.last_failure_attempt(),
        NonZeroU32::new(2)
    );
    assert_eq!(
        blocked_runtime.operator_action(),
        Some(authoring_application::DeploymentOperatorActionV2::RecoverBlockedDeployment)
    );
    assert!(!format!("{blocked_status:?}").contains(&private_blocked_code));

    let recovered = runtime
        .recover_blocked_for_operator(
            automation_runtime_convergence_postgres::RecoverBlockedDeploymentV1 {
                scope,
                expected_revision: blocked.snapshot.revision,
                expected_failure_id: blocked_failure_id,
                expected_failure_attempt: NonZeroU32::new(2).unwrap(),
                controller_id: ControllerId::parse(format!(
                    "operational-recovery-{}",
                    suffix()
                ))
                .unwrap(),
                lease_for: Duration::from_secs(10),
            },
        )
        .await
        .unwrap();
    assert_eq!(recovered.convergence_attempt.get(), 3);
    let recovered_status = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    let recovered_runtime = recovered_status.deployment().unwrap();
    assert_eq!(
        recovered_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::RuntimeReady
    );
    assert_eq!(recovered_runtime.current_attempt(), 3);
    assert_eq!(
        recovered_runtime.last_failure_attempt(),
        NonZeroU32::new(2)
    );
    assert_eq!(recovered_runtime.operator_action(), None);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_binds_live_attempt_and_reports_exact_disconnect() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let setup_deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let setup_application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &setup_deployments);
    let applied = setup_application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.operational.live.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-operational-live-{}", suffix())),
        )
        .await
        .unwrap();
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let scope = product_runtime_scope(&fixture, applied.exact_deployment());
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!("operational-live-{}", suffix())).unwrap(),
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
        Duration::from_secs(45),
    )
    .await;
    let deployments =
        authoring_application_postgres::PostgresProductDeploymentOperationalStatusesV2::new(
            pool.clone(),
        );
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let live_status = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    assert_eq!(
        live_status.status(),
        &DeploymentStatusV1::Live {
            attestation_revision: NonZeroU64::new(live.snapshot.revision.get()).unwrap(),
        }
    );
    let live_runtime = live_status.deployment().unwrap();
    assert_eq!(
        live_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::Live
    );
    assert_eq!(live_runtime.current_attempt(), claim.convergence_attempt.get());
    let attestation = live_runtime.attestation().unwrap();
    assert_eq!(
        attestation.deployment_revision().get(),
        live.snapshot.revision.get()
    );
    assert_eq!(attestation.convergence_attempt(), claim.convergence_attempt);
    assert_eq!(
        live_runtime.serving(),
        authoring_application::DeploymentServingFreshnessV2::Fresh {
            last_heartbeat_at: serving.last_heartbeat_at.into(),
            lease_expires_at: serving.expires_at.into(),
        }
    );

    runtime
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: serving.identity,
        })
        .await
        .unwrap();
    let disconnected = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    assert_eq!(disconnected.status(), &DeploymentStatusV1::Pending);
    let disconnected_runtime = disconnected.deployment().unwrap();
    assert_eq!(
        disconnected_runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::Live
    );
    assert_eq!(disconnected_runtime.attestation(), Some(attestation));
    match disconnected_runtime.serving() {
        authoring_application::DeploymentServingFreshnessV2::Disconnected {
            last_heartbeat_at,
            lease_expires_at,
        } => {
            assert_eq!(last_heartbeat_at, lease_expires_at);
            assert!(last_heartbeat_at >= SystemTime::from(serving.last_heartbeat_at));
            assert!(last_heartbeat_at <= disconnected_runtime.observed_at());
        }
        freshness => panic!("expected disconnected serving freshness, got {freshness:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_reports_naturally_expired_serving_lease() {
    let pool = pool().await;
    let runtime = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_secs(1),
            lock_timeout: Duration::from_millis(250),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let seed = seed_operational_live(&pool, &runtime, Duration::from_millis(1500)).await;
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let status = load_operational_status(&pool, &seed.fixture).await;
    assert_eq!(status.status(), &DeploymentStatusV1::Pending);
    let observation = status.deployment().unwrap();
    assert_live_attestation(observation, &seed);
    assert_eq!(
        observation.serving(),
        authoring_application::DeploymentServingFreshnessV2::Expired {
            last_heartbeat_at: seed.last_heartbeat_at,
            lease_expires_at: seed.lease_expires_at,
        }
    );
    assert!(seed.lease_expires_at <= observation.observed_at());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_reports_missing_serving_lease_without_timestamps() {
    let database = isolated_product_control_database("op_lease_missing").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let runtime = PostgresRuntimeConvergence::new(database.pool.clone());
        let seed =
            seed_operational_live(&database.pool, &runtime, Duration::from_secs(45)).await;
        let mut corruption = database.pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *corruption)
            .await
            .unwrap();
        let deleted = sqlx::query(
            "DELETE FROM public.runtime_serving_leases \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3",
        )
        .bind(seed.fixture.tenant_id.as_str())
        .bind(seed.fixture.installation_id.as_str())
        .bind(seed.exact.deployment_reference())
        .execute(&mut *corruption)
        .await
        .unwrap();
        assert_eq!(deleted.rows_affected(), 1);
        corruption.commit().await.unwrap();
        let status = load_operational_status(&database.pool, &seed.fixture).await;
        assert_eq!(status.status(), &DeploymentStatusV1::Pending);
        let observation = status.deployment().unwrap();
        assert_live_attestation(observation, &seed);
        assert_eq!(
            observation.serving(),
            authoring_application::DeploymentServingFreshnessV2::LeaseMissing
        );
        assert_eq!(observation.base().last_heartbeat_at(), None);
        assert_eq!(observation.base().lease_expires_at(), None);
    }
    drop_isolated_product_control_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_reports_missing_attestation_without_serving_evidence() {
    let database = isolated_product_control_database("op_attest_missing").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let runtime = PostgresRuntimeConvergence::new(database.pool.clone());
        let seed =
            seed_operational_live(&database.pool, &runtime, Duration::from_secs(45)).await;
        let mut corruption = database.pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *corruption)
            .await
            .unwrap();
        let deleted = sqlx::query(
            "DELETE FROM public.runtime_attestations \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3",
        )
        .bind(seed.fixture.tenant_id.as_str())
        .bind(seed.fixture.installation_id.as_str())
        .bind(seed.exact.deployment_reference())
        .execute(&mut *corruption)
        .await
        .unwrap();
        assert_eq!(deleted.rows_affected(), 1);
        corruption.commit().await.unwrap();
        let status = load_operational_status(&database.pool, &seed.fixture).await;
        assert_eq!(status.status(), &DeploymentStatusV1::Pending);
        let observation = status.deployment().unwrap();
        assert_eq!(
            observation.phase(),
            authoring_application::DeploymentConvergencePhaseV2::Live
        );
        assert_eq!(observation.attestation(), None);
        assert_eq!(
            observation.serving(),
            authoring_application::DeploymentServingFreshnessV2::AttestationMissing
        );
        assert_eq!(observation.base().last_heartbeat_at(), None);
        assert_eq!(observation.base().lease_expires_at(), None);
    }
    drop_isolated_product_control_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_reads_coherent_identity_during_concurrent_change() {
    let database = isolated_product_control_database("op_identity_mvcc").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let runtime = PostgresRuntimeConvergence::new(database.pool.clone());
        let seed =
            seed_operational_live(&database.pool, &runtime, Duration::from_secs(45)).await;
        let mut corruption = database.pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *corruption)
            .await
            .unwrap();
        let changed = sqlx::query(
            "UPDATE public.runtime_serving_leases \
             SET process_instance_id = $4 \
             WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3",
        )
        .bind(seed.fixture.tenant_id.as_str())
        .bind(seed.fixture.installation_id.as_str())
        .bind(seed.exact.deployment_reference())
        .bind(format!("operational-mismatch-{}", suffix()))
        .execute(&mut *corruption)
        .await
        .unwrap();
        assert_eq!(changed.rows_affected(), 1);
        let before_commit = load_operational_status(&database.pool, &seed.fixture).await;
        assert_eq!(
            before_commit.status(),
            &DeploymentStatusV1::Live {
                attestation_revision: seed.deployment_revision,
            }
        );
        let before_observation = before_commit.deployment().unwrap();
        assert_live_attestation(before_observation, &seed);
        assert_eq!(
            before_observation.serving(),
            authoring_application::DeploymentServingFreshnessV2::Fresh {
                last_heartbeat_at: seed.last_heartbeat_at,
                lease_expires_at: seed.lease_expires_at,
            }
        );
        corruption.commit().await.unwrap();
        let after_commit = load_operational_status(&database.pool, &seed.fixture).await;
        assert_eq!(after_commit.status(), &DeploymentStatusV1::Pending);
        let after_observation = after_commit.deployment().unwrap();
        assert_live_attestation(after_observation, &seed);
        assert_eq!(
            after_observation.serving(),
            authoring_application::DeploymentServingFreshnessV2::IdentityMismatch
        );
        assert_eq!(after_observation.base().last_heartbeat_at(), None);
        assert_eq!(after_observation.base().lease_expires_at(), None);
    }
    drop_isolated_product_control_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn operational_status_classifies_product_authority_drift_without_runtime_authority() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let setup_deployments = PostgresProductDeploymentStatuses::new(pool.clone());
    let setup_application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &setup_deployments);
    let applied = setup_application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.operational.authority.{}", suffix()))
                .unwrap(),
            &selector(&fixture),
            apply_command(
                &fixture,
                &format!("apply-operational-authority-{}", suffix()),
            ),
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.product_tenants SET lifecycle_state = 'suspended', \
         updated_at = GREATEST(pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
         WHERE tenant_id = $1",
    )
    .bind(fixture.tenant_id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    let deployments =
        authoring_application_postgres::PostgresProductDeploymentOperationalStatusesV2::new(
            pool.clone(),
        );
    let projected = ProjectedDecision {
        projection: applied_projection(&fixture, applied.exact_deployment().clone()),
    };
    let application =
        ProductControlApplication::new(&authentication, &authority, &projected, &deployments);
    let status = application
        .get_deployment_operational_status_v2(
            &fixture.credential,
            &selector(&fixture),
            fixture_operational_deployment_query(&fixture),
        )
        .await
        .unwrap();
    assert_eq!(
        status.status(),
        &DeploymentStatusV1::Failed {
            retryable: false,
            failure_code: "product_authority_inactive".to_string(),
        }
    );
    let runtime = status.deployment().unwrap();
    assert_eq!(
        runtime.phase(),
        authoring_application::DeploymentConvergencePhaseV2::AuthorityBlocked
    );
    assert_eq!(runtime.current_attempt(), 0);
    assert_eq!(
        runtime.operator_action(),
        Some(authoring_application::DeploymentOperatorActionV2::RestoreProductAuthority)
    );
    assert_eq!(
        runtime.serving(),
        authoring_application::DeploymentServingFreshnessV2::NotExpected
    );
}
