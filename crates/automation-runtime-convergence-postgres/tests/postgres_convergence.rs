use std::num::NonZeroU32;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_ruleset::{RuleSetContentHash, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    ControllerId, DeploymentId, DrainAttestationV1, GatewayReadyAttestationV1, GatewayReadyKindV1,
    InstallationId, PanelCertificateId, PanelCertificateV1, PreflightAttestationV1,
    ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, ClaimDeploymentV1, ClaimNextDeploymentV1,
    DeploymentAvailabilityV1, DeploymentMutationV1, EnqueueDeploymentOutcomeV1,
    EnqueueDeploymentV1, GatewayShardIdV1, HeartbeatServingLeaseV1, LiveMetadataV1,
    MarkServingDisconnectedV1, PanelReportDigestV1, PostgresRuntimeConvergence,
    PostgresRuntimeConvergenceConfigV1, RecoverStaleLiveV1, RuntimeBuildRevisionV1,
    RuntimeConvergenceStoreError, RuntimeDeploymentScopeV1, SubmitDeploymentMutationV1,
    SubmitLiveAttestationV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{Connection, PgPool};

const TENANT: &str = "runtime-pg-tenant";
const INSTALLATION: &str = "runtime-pg-installation";
const PRINCIPAL: &str = "runtime-pg-principal";
const GUILD: GuildId = GuildId(9200101);
const RULESET: &str = "runtime_pg_ruleset";
const PROMOTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTIVATION: &str = "runtime_pg_activation";
const DEPLOYMENT: &str = "runtime-pg-deployment";
const NEXT_PROMOTION: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const NEXT_ACTIVATION: &str = "runtime_pg_activation_next";
const NEXT_DEPLOYMENT: &str = "runtime-pg-deployment-next";
const CONTENT_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BINDING_FINGERPRINT: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROTATED_BINDING_FINGERPRINT: &str =
    "7777777777777777777777777777777777777777777777777777777777777777";

struct RuntimeMigrationDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

async fn isolated_runtime_migration_database() -> RuntimeMigrationDatabase {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let base = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let configured_database = base
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        configured_database.starts_with("starring_")
            && configured_database
                .split('_')
                .any(|segment| segment == "test")
            && configured_database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to create a database outside the strict Starring test namespace"
    );
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("starring_runtime_acl_test_{suffix}");
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    RuntimeMigrationDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_runtime_migration_database(database: RuntimeMigrationDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
}

async fn runtime_authority_function_acl(pool: &PgPool) -> (i64, i64, String, bool, bool, bool) {
    sqlx::query_as(
        "SELECT \
          routine.oid::BIGINT, \
          routine.proowner::BIGINT, \
          owner.rolname, \
          EXISTS (\
           SELECT 1 FROM pg_catalog.aclexplode(routine.proacl) AS privilege \
           INNER JOIN pg_catalog.pg_roles AS grantee ON grantee.oid = privilege.grantee \
           WHERE grantee.rolname = 'pg_read_all_data' \
            AND privilege.privilege_type = 'EXECUTE'), \
          EXISTS (\
           SELECT 1 FROM pg_catalog.aclexplode(routine.proacl) AS privilege \
           INNER JOIN pg_catalog.pg_roles AS grantee ON grantee.oid = privilege.grantee \
           WHERE grantee.rolname = 'pg_read_all_data' \
            AND privilege.privilege_type = 'EXECUTE' \
            AND privilege.is_grantable), \
          EXISTS (\
           SELECT 1 FROM pg_catalog.aclexplode(routine.proacl) AS privilege \
           WHERE privilege.grantee = 0 \
            AND privilege.privilege_type = 'EXECUTE') \
         FROM pg_catalog.pg_proc AS routine \
         INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = routine.proowner \
         WHERE routine.oid = pg_catalog.to_regprocedure(\
          'public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)')",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn authority_lock_upgrade_preserves_owner_and_explicit_execute_acl() {
    let database = isolated_runtime_migration_database().await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version < 202_607_190_011)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        sqlx::raw_sql(
            "GRANT EXECUTE ON FUNCTION public.starring_runtime_lock_current_authority(\
             text,text,text,text,bigint,text,text,bigint,text,bigint,text) \
             TO pg_read_all_data WITH GRANT OPTION",
        )
        .execute(&database.pool)
        .await?;
        let before = runtime_authority_function_acl(&database.pool).await;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_011)
            .expect("runtime authority upgrade migration must exist");
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await?;
        let after = runtime_authority_function_acl(&database.pool).await;
        Ok::<_, sqlx::Error>((before, after))
    }
    .await;
    drop_runtime_migration_database(database).await;
    let (before, after) = outcome.unwrap();
    assert_eq!(before.0, after.0);
    assert_eq!(before.1, after.1);
    assert_eq!(before.2, after.2);
    assert_eq!((before.3, before.4, before.5), (true, true, false));
    assert_eq!((after.3, after.4, after.5), (true, true, false));
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn runtime_authority_tracks_binding_identity_across_policy_rotation() {
    let pool = test_pool().await;
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let created = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("atomically seeded deployment must replay exactly: {outcome:?}"),
    };

    let unchanged_bindings = json!({});
    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 2,
            binding_revision: 1,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 2,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;

    let controller = ControllerId::parse("runtime-policy-controller").unwrap();
    let claim = adapter
        .claim_next(ClaimNextDeploymentV1 {
            controller_id: controller,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap()
        .expect("policy-only authority rotation must leave the deployment claimable");
    assert_eq!(
        claim.snapshot.identity.deployment_id,
        created.identity.deployment_id
    );
    let (live, serving) = converge_claimed(
        &adapter,
        claim,
        ProcessInstanceId::parse("runtime-policy-process").unwrap(),
    )
    .await;
    assert!(live.snapshot.live.is_some());
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
    assert_eq!(status.reason_code, "live");

    let heartbeat = adapter
        .heartbeat_serving(HeartbeatServingLeaseV1 {
            identity: serving.identity,
            lease_for: Duration::from_secs(45),
        })
        .await
        .unwrap();
    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: heartbeat.identity,
        })
        .await
        .unwrap();
    assert!(!disconnected.connected);
    let spoofed_bindings = json!({
        "channel_bindings": {"community_hub": "9200401"},
        "role_bindings": {}
    });
    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 3,
            binding_revision: 1,
            resource_bindings: &spoofed_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 3,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;
    assert_eq!(
        adapter.status(&scope()).await.unwrap().availability,
        DeploymentAvailabilityV1::Superseded
    );
    assert!(adapter.recover_next_stale_live().await.unwrap().is_none());
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: disconnected.identity.clone(),
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::BindingAuthorityMismatch
    ));
    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 4,
            binding_revision: 1,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 4,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;
    let recovered = adapter
        .recover_next_stale_live()
        .await
        .unwrap()
        .expect("policy-only authority rotation must leave stale Live recovery eligible");
    let recovered_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: recovered.snapshot.revision,
            controller_id: ControllerId::parse("runtime-policy-controller-recovered").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let (_, recovered_serving) = converge_recovered(
        &adapter,
        recovered_claim,
        ProcessInstanceId::parse("runtime-policy-process-recovered").unwrap(),
        "policy-build-recovered",
        "7",
        Duration::from_secs(45),
    )
    .await;
    assert_eq!(
        adapter.status(&scope()).await.unwrap().availability,
        DeploymentAvailabilityV1::Live
    );

    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 5,
            binding_revision: 2,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: ROTATED_BINDING_FINGERPRINT,
            policy_revision: 5,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;

    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Superseded);
    assert_eq!(status.reason_code, "binding_authority_changed");
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: recovered_serving.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::BindingAuthorityMismatch
    ));
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_live_status_and_fencing_survive_postgres() {
    let pool = test_pool().await;
    seed_product_target(&pool).await;
    assert_search_path_shadow_resistance(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let request = enqueue_request();
    let mut cross_tenant = request.clone();
    cross_tenant.identity.deployment_id = DeploymentId::parse("runtime-pg-cross-tenant").unwrap();
    cross_tenant.identity.tenant_id = TenantId::parse("runtime-pg-other-tenant").unwrap();
    assert!(matches!(
        adapter.enqueue(cross_tenant).await.unwrap_err(),
        RuntimeConvergenceStoreError::ScopeMismatch
    ));
    let mut cross_promotion = request.clone();
    cross_promotion.identity.deployment_id =
        DeploymentId::parse("runtime-pg-cross-promotion").unwrap();
    cross_promotion.identity.promotion_id = PromotionId::parse("9".repeat(64)).unwrap();
    assert!(matches!(
        adapter.enqueue(cross_promotion).await.unwrap_err(),
        RuntimeConvergenceStoreError::ScopeMismatch
    ));
    let mut wrong_activation = request.clone();
    wrong_activation.identity.deployment_id =
        DeploymentId::parse("runtime-pg-wrong-activation").unwrap();
    wrong_activation.identity.activation_request_id =
        ActivationRequestId::parse("runtime_pg_wrong_activation").unwrap();
    assert!(matches!(
        adapter.enqueue(wrong_activation).await.unwrap_err(),
        RuntimeConvergenceStoreError::ScopeMismatch
    ));
    let (first_enqueue, second_enqueue) = tokio::join!(
        adapter.enqueue(request.clone()),
        adapter.enqueue(request.clone())
    );
    let (created, replayed) = match (first_enqueue.unwrap(), second_enqueue.unwrap()) {
        (
            EnqueueDeploymentOutcomeV1::ExactReplay(created),
            EnqueueDeploymentOutcomeV1::ExactReplay(replayed),
        ) => (created, replayed),
        outcome => panic!("atomically seeded deployment must replay exactly: {outcome:?}"),
    };
    assert_eq!(created, replayed);
    let initial = created;
    assert_adapter_search_path_resistance().await;
    assert!(matches!(
        adapter.enqueue(request).await.unwrap(),
        EnqueueDeploymentOutcomeV1::ExactReplay(_)
    ));
    let controller = ControllerId::parse("runtime-pg-controller").unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM runtime_deployments WHERE deployment_id = $1 FOR UPDATE")
        .bind(DEPLOYMENT)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let timeout_adapter = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(200),
            lock_timeout: Duration::from_millis(50),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let timeout_error = timeout_adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        &timeout_error,
        RuntimeConvergenceStoreError::DatabaseTimeout
    ));
    assert!(timeout_error.is_retryable());
    let claiming_adapter = adapter.clone();
    let claim_request = ClaimDeploymentV1 {
        scope: scope(),
        expected_revision: initial.revision,
        controller_id: controller.clone(),
        lease_for: Duration::from_secs(90),
    };
    let claiming = tokio::spawn(async move { claiming_adapter.claim(claim_request).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    let released_at = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    let claim = claiming.await.unwrap().unwrap();
    assert!(claim.acquired_at >= released_at);
    assert_eq!(claim.expires_at - claim.acquired_at, TimeDelta::seconds(90));
    let replayed_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert_eq!(replayed_claim.fencing_token, claim.fencing_token);
    assert_eq!(replayed_claim.snapshot, claim.snapshot);
    let process = ProcessInstanceId::parse("runtime-pg-process").unwrap();
    let mut revision = claim.snapshot.revision;
    let preflight = PreflightAttestationV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        observed_runtime: None,
        checked_at: claim.acquired_at,
    };
    let preflight_expected = revision;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptPreflight(preflight.clone()),
    )
    .await;
    let replayed_preflight = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: preflight_expected,
            controller_id: controller.clone(),
            fencing_token: claim.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::AcceptPreflight(preflight),
        })
        .await
        .unwrap();
    assert_eq!(replayed_preflight.snapshot.revision, revision);
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::FIRST,
            drained_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
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
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("runtime-pg-panel-certificate").unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process.clone(),
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
    let stale_ready = adapter
        .certify_live(SubmitLiveAttestationV1 {
            scope: scope(),
            expected_revision: revision,
            controller_id: controller.clone(),
            fencing_token: claim.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            gateway_ready: GatewayReadyAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: process.clone(),
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: claim.acquired_at - TimeDelta::minutes(5),
            },
            metadata: LiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse("test-build-1").unwrap(),
                panel_report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            },
            serving_lease_for: Duration::from_secs(45),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        stale_ready,
        RuntimeConvergenceStoreError::InvalidInput("gateway Ready evidence is stale")
    ));
    let live_request = SubmitLiveAttestationV1 {
        scope: scope(),
        expected_revision: revision,
        controller_id: controller,
        fencing_token: claim.fencing_token,
        runtime_generation: RuntimeGeneration::FIRST,
        gateway_ready: GatewayReadyAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process,
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: claim.acquired_at,
        },
        metadata: LiveMetadataV1 {
            runtime_build_revision: RuntimeBuildRevisionV1::parse("test-build-1").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
        serving_lease_for: Duration::from_secs(45),
    };
    let drift_pool = pool.clone();
    let drift_adapter = adapter.clone();
    let drift_request = live_request.clone();
    let (live_transition, serving) = tokio::spawn(async move {
        certify_live_with_concurrent_active_drift(&drift_pool, &drift_adapter, drift_request).await
    })
    .await
    .unwrap();
    assert!(live_transition.snapshot.live.is_some());
    let (replayed_live, replayed_serving) = adapter.certify_live(live_request).await.unwrap();
    assert!(matches!(
        replayed_live.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed_serving, serving);
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
    let first_adapter = adapter.clone();
    let second_adapter = adapter.clone();
    let first_identity = serving.identity.clone();
    let second_identity = serving.identity;
    let (first, second) = tokio::join!(
        first_adapter.heartbeat_serving(HeartbeatServingLeaseV1 {
            identity: first_identity,
            lease_for: Duration::from_secs(45),
        }),
        second_adapter.heartbeat_serving(HeartbeatServingLeaseV1 {
            identity: second_identity,
            lease_for: Duration::from_secs(45),
        })
    );
    let heartbeat = match (first, second) {
        (Ok(receipt), Err(RuntimeConvergenceStoreError::RevisionConflict))
        | (Err(RuntimeConvergenceStoreError::RevisionConflict), Ok(receipt)) => receipt,
        outcome => panic!("exactly one fenced heartbeat must win: {outcome:?}"),
    };
    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: heartbeat.identity.clone(),
        })
        .await
        .unwrap();
    assert!(!disconnected.connected);
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(
        status.availability,
        DeploymentAvailabilityV1::RuntimePending
    );
    assert_eq!(status.reason_code, "gateway_not_serving");
    let disconnected_recovery = RecoverStaleLiveV1 {
        identity: disconnected.identity.clone(),
        expected_deployment_revision: live_transition.snapshot.revision,
    };
    let recovered = adapter
        .recover_stale_live(disconnected_recovery.clone())
        .await
        .unwrap();
    assert!(recovered.snapshot.last_live_recovery.is_some());
    assert!(recovered.snapshot.live.is_none());
    let replayed_recovery = adapter
        .recover_stale_live(disconnected_recovery)
        .await
        .unwrap();
    assert!(matches!(
        replayed_recovery.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: disconnected.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    let second_controller = ControllerId::parse("runtime-pg-controller-2").unwrap();
    let second_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: recovered.snapshot.revision,
            controller_id: second_controller,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert!(second_claim.fencing_token > claim.fencing_token);
    let short_lease_adapter = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(100),
            lock_timeout: Duration::from_millis(50),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let (second_live, second_serving) = converge_recovered(
        &short_lease_adapter,
        second_claim,
        ProcessInstanceId::parse("runtime-pg-process-2").unwrap(),
        "test-build-2",
        "e",
        Duration::from_millis(500),
    )
    .await;
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: heartbeat.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    let mut status_blocker = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE runtime_serving_leases IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *status_blocker)
        .await
        .unwrap();
    let status_adapter = adapter.clone();
    let delayed_status = tokio::spawn(async move { status_adapter.status(&scope()).await });
    tokio::time::sleep(Duration::from_millis(650)).await;
    status_blocker.commit().await.unwrap();
    let status = delayed_status.await.unwrap().unwrap();
    assert_eq!(
        status.availability,
        DeploymentAvailabilityV1::RuntimePending
    );
    assert_eq!(status.reason_code, "serving_lease_expired");
    sqlx::query(
        "UPDATE automation_ruleset_activations SET active_version = 2 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&pool)
    .await
    .unwrap();
    let superseded_status = adapter.status(&scope()).await.unwrap();
    assert_eq!(
        superseded_status.availability,
        DeploymentAvailabilityV1::Superseded
    );
    assert_eq!(superseded_status.reason_code, "active_target_changed");
    assert!(adapter.recover_next_stale_live().await.unwrap().is_none());
    sqlx::query(
        "UPDATE automation_ruleset_activations SET active_version = 1 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&pool)
    .await
    .unwrap();
    let expired_recovery = adapter
        .recover_next_stale_live()
        .await
        .unwrap()
        .expect("expired Live deployment is recoverable");
    assert!(expired_recovery.snapshot.live.is_none());
    let third_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: expired_recovery.snapshot.revision,
            controller_id: ControllerId::parse("runtime-pg-controller-3").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert!(third_claim.fencing_token > second_claim_fencing(&second_live));
    let (third_live, third_serving) = converge_recovered(
        &adapter,
        third_claim,
        ProcessInstanceId::parse("runtime-pg-process-3").unwrap(),
        "test-build-3",
        "6",
        Duration::from_secs(45),
    )
    .await;
    assert!(third_serving.identity.lease_epoch > second_serving.identity.lease_epoch);
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: second_serving.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
    assert_eq!(
        status.live.unwrap().process_instance_id,
        ProcessInstanceId::parse("runtime-pg-process-3").unwrap()
    );
    let suspension_pool = pool.clone();
    let suspension_adapter = adapter.clone();
    let suspension_serving = third_serving.clone();
    tokio::spawn(async move {
        assert_concurrent_tenant_suspension(
            &suspension_pool,
            &suspension_adapter,
            &suspension_serving,
        )
        .await;
    })
    .await
    .unwrap();
    let recovery_pool = pool.clone();
    let recovery_adapter = adapter.clone();
    tokio::spawn(async move {
        assert_recovery_and_newer_certification_do_not_deadlock(
            &recovery_pool,
            &recovery_adapter,
            &third_live,
            &third_serving,
        )
        .await;
    })
    .await
    .unwrap();
}

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
         VALUES ($1, $2, 2, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind("7".repeat(64))
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
    let drifting = tokio::spawn(async move {
        sqlx::query_scalar::<_, DateTime<Utc>>(
            "UPDATE public.automation_ruleset_activations SET active_version = 2 \
             WHERE guild_id = $1 AND ruleset_key = $2 \
             RETURNING pg_catalog.clock_timestamp()",
        )
        .bind(GUILD.to_string())
        .bind(RULESET)
        .fetch_one(&drift_pool)
        .await
    });
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
            drained_at: claim.acquired_at,
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
            activated_at: claim.acquired_at,
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
            reconciled_at: claim.acquired_at,
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
            ready_at: claim.acquired_at,
        },
        metadata: LiveMetadataV1 {
            runtime_build_revision: RuntimeBuildRevisionV1::parse("test-build-next").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
        serving_lease_for: Duration::from_secs(45),
    };
    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: current_serving.identity.clone(),
        })
        .await
        .unwrap();
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
            serving_lease_for: Duration::from_secs(45),
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

async fn test_pool() -> PgPool {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn seed_product_target(pool: &PgPool) {
    let now = Utc::now();
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
         VALUES ($1, $2, 1, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(PRINCIPAL)
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
    sqlx::query(
        "INSERT INTO authoring_promotions (id, record_format_version, revision, stage, \
         request_digest, tenant_id, principal_id, record) \
         VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5)",
    )
    .bind(PROMOTION)
    .bind(&request_digest)
    .bind(TENANT)
    .bind(PRINCIPAL)
    .bind(Json(promotion_record))
    .execute(&mut *transaction)
    .await
    .unwrap();
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

async fn seed_next_product_journal(pool: &PgPool, request: &EnqueueDeploymentV1) {
    let now = Utc::now();
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
    sqlx::query(
        "INSERT INTO public.authoring_promotions (id, record_format_version, revision, stage, \
         request_digest, tenant_id, principal_id, record) \
         VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5)",
    )
    .bind(NEXT_PROMOTION)
    .bind(&request_digest)
    .bind(TENANT)
    .bind(PRINCIPAL)
    .bind(Json(record))
    .execute(&mut *transaction)
    .await
    .unwrap();
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

async fn assert_adapter_search_path_resistance() {
    let url = std::env::var("STARRING_TEST_DATABASE_URL").unwrap();
    let setup_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
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
        .connect(&url)
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
                "created_at": created_at,
                "expires_at": expires_at,
                "request_state_at_journal": "pending",
                "approval_context": approval_context
            }
        }
    })
}
