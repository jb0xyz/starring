use std::borrow::Cow;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceRegistrarV1, InstanceResources,
    InstanceRouteReaderV1, InstanceRuleSetVersion, InstanceStatus, InstanceStoreError,
    InstanceTeardownClaimOutcomeV1, InstanceTeardownMarkOutcomeV1,
    InstanceTeardownRetryScanCursorV2, InstanceTeardownRetryScannerV2, InstanceTeardownStoreV1,
    MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1, MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2,
};
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_ruleset_dispatch::{PinnedInstanceResolverErrorV1, PinnedInstanceResolverV1};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, FencingToken, InstallationId, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    build_interaction_token_authenticated_data_v1, DiscordApplicationIdV1, DiscordInteractionIdV1,
    EncryptedInteractionTokenV1, InteractionActionPlanDigestV1, InteractionExpectedRouteV1,
    InteractionGatewayShardIdentityV1, InteractionProductScopeV1,
    InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1, InteractionRequestDigestV1,
    InteractionRouteIncarnationV1, InteractionRuntimeBuildRevisionV1,
    InteractionTokenAuthenticatedDataInputV1, InteractionTokenEnvelopeTimeV1,
    XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1,
    XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1,
};
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1,
    RuntimeInteractionReceiptClaimLeaseV1, RuntimeInteractionReceiptClaimOutcomeV1,
    RuntimeInteractionReceiptClaimRequestV1,
    RuntimeInteractionReceiptInitialResponseIntentDispositionV1,
    RuntimeInteractionReceiptInitialResponseIntentV1,
    RuntimeInteractionReceiptInitialResponseKindV1,
    RuntimeInteractionReceiptInitialResponseResultKindV1,
    RuntimeInteractionReceiptInitialResponseResultV1,
    RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionReceiptOpaqueDigestV1,
    RuntimeInteractionReceiptRecoveryDeferredReasonV1,
    RuntimeInteractionReceiptRecoveryObservationKindV1, RuntimeInteractionReceiptRecoveryOutcomeV1,
    RuntimeInteractionReceiptRecoveryRequestV1, RuntimeInteractionReceiptRecoveryRequiredReasonV1,
    RuntimeInteractionReceiptRecoveryScanCursorV1, RuntimeInteractionReceiptRequestKindV1,
    RuntimeInteractionReceiptRouteV1, RuntimeInteractionReceiptTerminalOutcomeV1,
    RuntimeInteractionReceiptTerminalStateV1,
    RuntimeInteractionReceiptTerminalizeExpiredDispositionV1,
    RuntimeInteractionReceiptTerminalizeExpiredRequestV1,
    RuntimeInteractionReceiptTokenExpiryDispositionV1,
    RuntimeInteractionReceiptTokenExpiryRequestV1, RuntimeInteractionRouteTimeoutV1, MIGRATOR,
};
use chrono::Utc;
use discord_model::{ChannelId, GuildId, RoleId, UserId};
use resource_resolution::ResourceBindingFingerprint;
use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::{Connection, Executor, PgPool};

const READINESS_FUNCTION: &str = "public.starring_runtime_interaction_database_readiness_v1()";
const IDENTITY_FUNCTION: &str = "public.starring_runtime_interaction_database_identity_v1()";
const ROUTE_FUNCTION: &str = "public.starring_runtime_interaction_route_read_v1(TEXT,TEXT)";
const PINNED_FUNCTION: &str = "public.starring_runtime_interaction_pinned_read_v1(TEXT,TEXT)";
const REGISTER_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_register_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,JSONB)";
const TEARDOWN_GET_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_get_for_teardown_v1(TEXT,TEXT)";
const TEARDOWN_CLAIM_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_claim_deleting_v1(TEXT,TEXT)";
const TEARDOWN_MARK_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_mark_deleted_v1(TEXT,TEXT)";
const TEARDOWN_RETRY_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_list_retryable_v1(TEXT,BIGINT)";
const TEARDOWN_RETRY_SCAN_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_scan_retryable_v2(TEXT,TEXT,TEXT,TEXT,BIGINT)";
const RECEIPT_AUTHORITY_OBSERVE_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_authority_observe_v1(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,TEXT,TEXT,TEXT)";
const RECEIPT_CLAIM_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_claim_v1(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,BYTEA,BIGINT,TEXT,SMALLINT,TEXT,BYTEA,BYTEA,BYTEA,TIMESTAMPTZ,TIMESTAMPTZ)";
const RECEIPT_PLAN_BIND_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_plan_bind_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA)";
const RECEIPT_ACKNOWLEDGEMENT_INTEND_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_acknowledgement_intend_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,BYTEA)";
const RECEIPT_ACKNOWLEDGEMENT_FINISH_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA,TEXT,BYTEA)";
const RECEIPT_EXECUTION_INTEND_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_execution_intend_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA)";
const RECEIPT_FINISH_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_finish_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA,TEXT,TEXT,BYTEA)";
const RECEIPT_RECOVERY_SCAN_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_scan_recoverable_v1(TIMESTAMPTZ,TEXT,TEXT,TIMESTAMPTZ,TEXT,TEXT,BIGINT)";
const RECEIPT_RECOVER_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_recover_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BYTEA,BIGINT)";
const RECEIPT_TOKEN_EXPIRE_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_token_expire_v1(TEXT,TEXT,BIGINT,BIGINT,BYTEA)";
const RECEIPT_TERMINALIZE_EXPIRED_FUNCTION: &str =
    "public.starring_runtime_interaction_receipt_terminalize_expired_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,BYTEA)";

static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const RECEIPT_APPLICATION_ID: u64 = 9_100_001;
const RECEIPT_GUILD_ID: u64 = 9_100_002;
const RECEIPT_CHANNEL_ID: u64 = 9_100_003;
const RECEIPT_ACTOR_ID: u64 = 9_100_004;
const RECEIPT_TENANT_ID: &str = "tenant-receipt";
const RECEIPT_INSTALLATION_ID: &str = "installation-receipt";
const RECEIPT_DEPLOYMENT_ID: &str = "deployment-receipt";
const RECEIPT_RULESET_KEY: &str = "receipt";
const RECEIPT_PROCESS_ID: &str = "process-receipt";
const RECEIPT_BUILD_REVISION: &str = "build-receipt";
const RECEIPT_GATEWAY_SHARD: &str = "shard:0";
const RECEIPT_BINDING_FINGERPRINT: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct IsolatedDatabase {
    name: String,
    role: String,
    administrator: PgConnection,
    owner_pool: PgPool,
    executor_pool: PgPool,
    deadline_pool: PgPool,
    cross_role: String,
    cross_pool: PgPool,
}

fn function_grant(function: &str, role: &str) -> String {
    format!("GRANT EXECUTE ON FUNCTION {function} TO {role}")
}

async fn isolated_database() -> IsolatedDatabase {
    isolated_database_with_upgrade_boundary(None).await
}

async fn isolated_database_with_upgrade_boundary(
    upgrade_boundary: Option<i64>,
) -> IsolatedDatabase {
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
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let suffix = format!("{:x}_{timestamp:x}_{sequence:x}", std::process::id());
    let name = format!("starring_ri_test_{suffix}");
    let role = format!("starring_ri_executor_{suffix}");
    let cross_role = format!("starring_ri_cross_{suffix}");
    let password = format!("ri_test_password_{suffix}");
    let cross_password = format!("ri_cross_password_{suffix}");
    for identifier in [&name, &role, &cross_role] {
        assert!(
            identifier.len() <= 63
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
    }

    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    administrator
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .unwrap();
    administrator
        .execute(
            format!(
                "CREATE ROLE {cross_role} LOGIN PASSWORD '{cross_password}' NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 1"
            )
            .as_str(),
        )
        .await
        .unwrap();
    administrator
        .execute(
            format!(
                "CREATE ROLE {role} LOGIN PASSWORD '{password}' NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
            )
            .as_str(),
        )
        .await
        .unwrap();

    let owner_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.clone().database(&name))
        .await
        .unwrap();
    if let Some(boundary) = upgrade_boundary {
        let partial = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| migration.version <= boundary)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        };
        partial.run(&owner_pool).await.unwrap();
        assert!(
            sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.to_regprocedure(\
                    'public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)'\
                 ) IS NULL",
            )
            .fetch_one(&owner_pool)
            .await
            .unwrap()
        );
        MIGRATOR.run(&owner_pool).await.unwrap();
    } else {
        MIGRATOR.run(&owner_pool).await.unwrap();
    }
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON DATABASE {name} FROM PUBLIC"),
        "REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC".to_string(),
        format!("GRANT CONNECT ON DATABASE {name} TO {role}"),
        format!("GRANT USAGE ON SCHEMA public TO {role}"),
        function_grant(IDENTITY_FUNCTION, &role),
        function_grant(READINESS_FUNCTION, &role),
        function_grant(ROUTE_FUNCTION, &role),
        function_grant(PINNED_FUNCTION, &role),
        function_grant(REGISTER_FUNCTION, &role),
        function_grant(TEARDOWN_GET_FUNCTION, &role),
        function_grant(TEARDOWN_CLAIM_FUNCTION, &role),
        function_grant(TEARDOWN_MARK_FUNCTION, &role),
        function_grant(TEARDOWN_RETRY_FUNCTION, &role),
        function_grant(TEARDOWN_RETRY_SCAN_FUNCTION, &role),
        function_grant(RECEIPT_AUTHORITY_OBSERVE_FUNCTION, &role),
        function_grant(RECEIPT_CLAIM_FUNCTION, &role),
        function_grant(RECEIPT_PLAN_BIND_FUNCTION, &role),
        function_grant(RECEIPT_ACKNOWLEDGEMENT_INTEND_FUNCTION, &role),
        function_grant(RECEIPT_ACKNOWLEDGEMENT_FINISH_FUNCTION, &role),
        function_grant(RECEIPT_EXECUTION_INTEND_FUNCTION, &role),
        function_grant(RECEIPT_FINISH_FUNCTION, &role),
        function_grant(RECEIPT_RECOVERY_SCAN_FUNCTION, &role),
        function_grant(RECEIPT_RECOVER_FUNCTION, &role),
        function_grant(RECEIPT_TOKEN_EXPIRE_FUNCTION, &role),
        function_grant(RECEIPT_TERMINALIZE_EXPIRED_FUNCTION, &role),
        format!("GRANT CONNECT ON DATABASE {name} TO {cross_role}"),
        format!("GRANT USAGE ON SCHEMA public TO {cross_role}"),
    ] {
        owner_pool.execute(statement.as_str()).await.unwrap();
    }

    let executor_options = base
        .clone()
        .database(&name)
        .username(&role)
        .password(&password);
    let executor_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(executor_options.clone())
        .await
        .unwrap();
    let deadline_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(executor_options)
        .await
        .unwrap();
    let cross_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            base.database(&name)
                .username(&cross_role)
                .password(&cross_password),
        )
        .await
        .unwrap();
    IsolatedDatabase {
        name,
        role,
        administrator,
        owner_pool,
        executor_pool,
        deadline_pool,
        cross_role,
        cross_pool,
    }
}

async fn cleanup(mut database: IsolatedDatabase) {
    database.cross_pool.close().await;
    database.deadline_pool.close().await;
    database.executor_pool.close().await;
    database.owner_pool.close().await;
    database
        .administrator
        .execute(format!("DROP DATABASE {} WITH (FORCE)", database.name).as_str())
        .await
        .unwrap();
    database
        .administrator
        .execute(format!("DROP ROLE {}", database.role).as_str())
        .await
        .unwrap();
    database
        .administrator
        .execute(format!("DROP ROLE {}", database.cross_role).as_str())
        .await
        .unwrap();
}

fn instance(kind: &str) -> AutomationInstance {
    let mut roles = BTreeMap::new();
    roles.insert("member".to_string(), RoleId(9));
    AutomationInstance {
        id: InstanceId::parse("room").unwrap(),
        guild_id: GuildId(7),
        ruleset_key: "study".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        kind: InstanceKind(kind.to_string()),
        created_by: UserId(3),
        resources: InstanceResources {
            roles,
            channels: BTreeMap::new(),
            messages: BTreeMap::new(),
        },
        status: InstanceStatus::Active,
    }
}

fn sqlstate(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
}

async fn seed_receipt_authority(pool: &PgPool) -> String {
    for table in [
        "automation_installations",
        "runtime_deployments",
        "runtime_attestations",
        "runtime_serving_leases",
        "runtime_gateway_owners",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} DISABLE TRIGGER ALL"))
            .execute(pool)
            .await
            .unwrap();
    }
    let definition = r#"{"version":1,"panels":[],"modals":[],"rules":[]}"#;
    let content_hash: String = sqlx::query_scalar(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         SELECT $1, $2, 1, 1, $3::JSONB, \
                public.starring_ruleset_content_hash_v1(1, $3::JSONB), $4 \
         RETURNING content_hash",
    )
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .bind(definition)
    .bind(RECEIPT_ACTOR_ID.to_string())
    .fetch_one(pool)
    .await
    .unwrap();
    let request_bytes = br#"{"fixture":"durable-receipt-contract"}"#.to_vec();
    let (request_digest, live_bytes, attestation_id): (String, Vec<u8>, String) = sqlx::query_as(
        "WITH request AS ( \
                 SELECT $1::BYTEA AS bytes, \
                        starring_runtime_private_v2.starring_runtime_framed_digest_v2( \
                            pg_catalog.convert_to( \
                                'starring.runtime.certification_request.v2', 'UTF8' \
                            ) || pg_catalog.decode('00', 'hex'), \
                            $1::BYTEA \
                        ) AS digest \
             ), live AS ( \
                 SELECT request.digest, \
                        pg_catalog.convert_to( \
                            '{\"format_version\":2,\"request_digest\":\"' \
                                || request.digest || '\",\"request\":', \
                            'UTF8' \
                        ) || request.bytes || pg_catalog.convert_to('}', 'UTF8') AS bytes \
                 FROM request \
             ) \
             SELECT live.digest, live.bytes, \
                    starring_runtime_private_v2.starring_runtime_framed_digest_v2( \
                        pg_catalog.convert_to( \
                            'starring.runtime.live_attestation.v2', 'UTF8' \
                        ) || pg_catalog.decode('00', 'hex'), \
                        live.bytes \
                    ) \
             FROM live",
    )
    .bind(request_bytes.clone())
    .fetch_one(pool)
    .await
    .unwrap();
    let now = Utc::now();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, \
          ruleset_key, lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(RECEIPT_INSTALLATION_ID)
    .bind(RECEIPT_TENANT_ID)
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_deployments \
         (deployment_id, tenant_id, installation_id, promotion_id, activation_request_id, \
          installation_authority_revision, guild_id, ruleset_key, target_version, \
          target_content_hash, binding_revision, binding_fingerprint, desired_target_digest, \
          runtime_generation, requested_at, snapshot_format_version, snapshot, revision, phase, \
          live_attestation_id, live_at, created_at, updated_at, policy_revision, \
          desired_target_digest_version, convergence_attempt_no) \
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, 1, $8, 1, $9, $10, 1, $11, 1, \
                 $12::JSONB, 1, 'live', $13, $11, $11, $11, 1, 1, 1)",
    )
    .bind(RECEIPT_DEPLOYMENT_ID)
    .bind(RECEIPT_TENANT_ID)
    .bind(RECEIPT_INSTALLATION_ID)
    .bind("c".repeat(64))
    .bind("receipt_activation")
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .bind(&content_hash)
    .bind(RECEIPT_BINDING_FINGERPRINT)
    .bind("d".repeat(64))
    .bind(now)
    .bind(r#"{"fixture":"durable receipt deployment snapshot"}"#)
    .bind(&attestation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_attestations \
         (attestation_id, attestation_digest, deployment_id, deployment_revision, tenant_id, \
          installation_id, promotion_id, activation_request_id, guild_id, ruleset_key, \
          target_version, target_content_hash, binding_revision, binding_fingerprint, \
          runtime_generation, controller_fencing_token, process_instance_id, \
          runtime_build_revision, panel_certificate_id, panel_report_digest, gateway_shard_id, \
          gateway_ready_kind, gateway_ready_at, certified_at, record_format_version, record, \
          created_at, convergence_attempt_no, serving_lease_duration_nanos, v2_operation_id, \
          v2_intent_fingerprint, v2_request_digest, v2_request_bytes, \
          v2_live_attestation_bytes, v2_must_commit_before, v2_route_admission, \
          v2_route_incarnation, v2_route_activation_sequence, v2_initial_lease_epoch, \
          v2_initial_serving_revision, v2_prepared_snapshot, v2_certified_snapshot) \
         VALUES ($1, $1, $2, 1, $3, $4, $5, $6, $7, $8, 1, $9, 1, $10, 1, 1, $11, $12, \
                 $13, $14, $15, 'discord_ready', $16, $16, 2, $17::JSONB, $16, 1, \
                 300000000000, $18, $19, $20, $21, $22, $23, $24::JSONB, 1, 1, 1, 1, \
                 $25::JSONB, $26::JSONB)",
    )
    .bind(&attestation_id)
    .bind(RECEIPT_DEPLOYMENT_ID)
    .bind(RECEIPT_TENANT_ID)
    .bind(RECEIPT_INSTALLATION_ID)
    .bind("c".repeat(64))
    .bind("receipt_activation")
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .bind(&content_hash)
    .bind(RECEIPT_BINDING_FINGERPRINT)
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_BUILD_REVISION)
    .bind("receipt-panel")
    .bind("e".repeat(64))
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(now)
    .bind(r#"{"fixture":"durable receipt attestation record"}"#)
    .bind("1234567890abcdef1234567890abcdef")
    .bind("f".repeat(64))
    .bind(&request_digest)
    .bind(request_bytes)
    .bind(live_bytes)
    .bind(now + chrono::TimeDelta::seconds(60))
    .bind(r#"{"gateway_owner_lease_id":{"lease_epoch":1},"fixture":"receipt"}"#)
    .bind(r#"{"fixture":"durable receipt prepared snapshot"}"#)
    .bind(r#"{"fixture":"durable receipt certified snapshot"}"#)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_serving_leases \
         (guild_id, ruleset_key, tenant_id, installation_id, deployment_id, attestation_id, \
          process_instance_id, runtime_generation, target_version, target_content_hash, \
          binding_revision, binding_fingerprint, lease_epoch, revision, connected, serving, \
          acquired_at, last_heartbeat_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 1, 1, $8, 1, $9, 1, 1, TRUE, TRUE, \
                 $10, $10, $11)",
    )
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .bind(RECEIPT_TENANT_ID)
    .bind(RECEIPT_INSTALLATION_ID)
    .bind(RECEIPT_DEPLOYMENT_ID)
    .bind(&attestation_id)
    .bind(RECEIPT_PROCESS_ID)
    .bind(&content_hash)
    .bind(RECEIPT_BINDING_FINGERPRINT)
    .bind(now)
    .bind(now + chrono::TimeDelta::seconds(300))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_gateway_owners \
         (gateway_shard_id, process_instance_id, lease_epoch, expected_build_revision, \
          owner_revision, expires_at) VALUES ($1, $2, 1, $3, 1, $4)",
    )
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(now + chrono::TimeDelta::seconds(300))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    for table in [
        "automation_installations",
        "runtime_deployments",
        "runtime_attestations",
        "runtime_serving_leases",
        "runtime_gateway_owners",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} ENABLE TRIGGER ALL"))
            .execute(pool)
            .await
            .unwrap();
    }
    content_hash
}

fn receipt_expected_route(content_hash: &str) -> InteractionExpectedRouteV1 {
    receipt_expected_route_for(content_hash, RECEIPT_PROCESS_ID, RECEIPT_BUILD_REVISION)
}

fn receipt_expected_route_for(
    content_hash: &str,
    process_instance_id: &str,
    runtime_build_revision: &str,
) -> InteractionExpectedRouteV1 {
    let target = RuntimeDeploymentTargetV1 {
        guild_id: GuildId(RECEIPT_GUILD_ID),
        ruleset_key: RuleSetKey::parse(RECEIPT_RULESET_KEY).unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(content_hash).unwrap(),
        binding_revision: BindingRevision::new(1).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(RECEIPT_BINDING_FINGERPRINT)
            .unwrap(),
    };
    InteractionExpectedRouteV1::new(
        InteractionProductScopeV1::new(
            TenantId::parse(RECEIPT_TENANT_ID).unwrap(),
            InstallationId::parse(RECEIPT_INSTALLATION_ID).unwrap(),
            DeploymentId::parse(RECEIPT_DEPLOYMENT_ID).unwrap(),
        ),
        RuntimeProcessIdentityV1 {
            target,
            runtime_generation: RuntimeGeneration::new(1).unwrap(),
            process_instance_id: ProcessInstanceId::parse(process_instance_id).unwrap(),
        },
        InteractionGatewayShardIdentityV1::parse(RECEIPT_GATEWAY_SHARD).unwrap(),
        InteractionRuntimeBuildRevisionV1::parse(runtime_build_revision).unwrap(),
        FencingToken::new(1).unwrap(),
        InteractionRouteIncarnationV1::new(1).unwrap(),
    )
    .unwrap()
}

async fn acquire_test_receipt(
    store: &PostgresRuntimeInteractionV1,
    content_hash: &str,
    interaction_id: u64,
    route_key: &str,
    lease: Duration,
) -> automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1 {
    let request = test_receipt_request(store, content_hash, interaction_id, route_key, lease).await;
    match store.claim_interaction_receipt_v1(request).await.unwrap() {
        RuntimeInteractionReceiptClaimOutcomeV1::Acquired(claim) => *claim,
        _ => panic!("test receipt claim was not acquired"),
    }
}

async fn test_receipt_request(
    store: &PostgresRuntimeInteractionV1,
    content_hash: &str,
    interaction_id: u64,
    route_key: &str,
    lease: Duration,
) -> RuntimeInteractionReceiptClaimRequestV1 {
    test_receipt_request_with_digest(
        store,
        content_hash,
        interaction_id,
        route_key,
        lease,
        format!("{interaction_id:064x}"),
    )
    .await
}

async fn test_receipt_request_with_digest(
    store: &PostgresRuntimeInteractionV1,
    content_hash: &str,
    interaction_id: u64,
    route_key: &str,
    lease: Duration,
    request_digest: String,
) -> RuntimeInteractionReceiptClaimRequestV1 {
    let identity = InteractionReceiptIdentityV1::new(
        DiscordApplicationIdV1::new(RECEIPT_APPLICATION_ID).unwrap(),
        DiscordInteractionIdV1::new(interaction_id).unwrap(),
    );
    let candidate = InteractionReceiptClaimCandidateV1::new(
        identity,
        receipt_expected_route(content_hash),
        InteractionRequestDigestV1::parse(request_digest).unwrap(),
    );
    let authority = store
        .observe_interaction_receipt_authority_v1(
            candidate,
            RuntimeInteractionReceiptRouteV1::static_route(route_key).unwrap(),
        )
        .await
        .unwrap();
    let now = u64::try_from(Utc::now().timestamp_millis()).unwrap();
    let time = InteractionTokenEnvelopeTimeV1::new(now - 1_000, now + 30_000).unwrap();
    let authenticated_data =
        build_interaction_token_authenticated_data_v1(InteractionTokenAuthenticatedDataInputV1 {
            claim_root: authority.claim_root(),
            encryption_key_id: "receipt-test-key",
            encryption_suite: XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1,
            encryption_suite_version: XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1,
            time,
        })
        .unwrap();
    let token = EncryptedInteractionTokenV1::from_persisted_parts(
        vec![7; 17],
        vec![8; 24],
        "receipt-test-key".to_string(),
        XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1.to_string(),
        XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1,
        time,
        authenticated_data.digest().clone(),
    )
    .unwrap();
    RuntimeInteractionReceiptClaimRequestV1::new(
        authority,
        ChannelId(RECEIPT_CHANNEL_ID),
        UserId(RECEIPT_ACTOR_ID),
        RuntimeInteractionReceiptRequestKindV1::MessageComponent,
        token,
        RuntimeInteractionReceiptClaimLeaseV1::new(lease).unwrap(),
    )
    .unwrap()
}

async fn replace_with_expired_token(pool: &PgPool, interaction_id: u64) {
    sqlx::query(
        "DELETE FROM public.runtime_interaction_receipt_token_secrets_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_receipt_token_secrets_v1 \
         (application_id, interaction_id, encryption_suite, suite_version, key_id, nonce, \
          ciphertext, aad_digest, issued_at, expires_at) \
         VALUES ($1, $2, 'xchacha20_poly1305', 1, 'expired-test-key', $3, $4, $5, \
                 pg_catalog.clock_timestamp() - INTERVAL '2 minutes', \
                 pg_catalog.clock_timestamp() - INTERVAL '1 minute')",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(vec![1_u8; 24])
    .bind(vec![2_u8; 17])
    .bind(vec![3_u8; 32])
    .execute(pool)
    .await
    .unwrap();
}

async fn remove_interaction_token(pool: &PgPool, interaction_id: u64) {
    sqlx::query(
        "DELETE FROM public.runtime_interaction_receipt_token_secrets_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn interaction_migration_follows_convergence_exactness() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let convergence = versions
        .iter()
        .position(|version| *version == 202_607_220_026)
        .unwrap();
    let interaction = versions
        .iter()
        .position(|version| *version == 202_607_220_027)
        .unwrap();
    assert_eq!(interaction, convergence + 1);
}

#[test]
fn durable_receipt_migration_preserves_expiry_and_recovery_contracts() {
    let migration =
        include_str!("../../../migrations/202607310022_add_runtime_interaction_receipts_v1.sql");
    for required in [
        "root_route_key TEXT",
        "root_route_key := root_row.route_key",
        "proposed_terminal_outcome_code = 'exact_replay'",
        "SELECT artifact.version, artifact.content_hash, NULL::TEXT AS manifest_digest",
        "public.starring_runtime_interaction_receipt_token_expire_v1(text,text,bigint,bigint,bytea)",
        "WHEN secret_found THEN 'interaction_token_expired'",
        "ELSE 'interaction_token_unavailable'",
        "public.starring_runtime_interaction_receipt_terminalize_expired_v1(text,text,bigint,bigint,text,text,bytea)",
        "outcome_name := 'route_authority_stale'",
        "'expired_pristine_claim_abandoned'",
        "outcome_name := 'claim_renewed'",
        "outcome_name := 'revision_race'",
        "outcome_name := 'successor_process_recovery_deferred'",
        "root_row.origin_process_instance_id",
        "root_row.origin_serving_revision\n                > expected_serving_revision",
        "root_row.origin_gateway_owner_revision\n                > expected_gateway_owner_revision",
        "derived_serving_revision := expected_serving_revision",
        "derived_gateway_owner_revision :=\n            expected_gateway_owner_revision",
    ] {
        assert!(migration.contains(required), "missing contract: {required}");
    }
    let token_expiry = migration
        .split("CREATE FUNCTION public.starring_runtime_interaction_receipt_token_expire_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    assert!(!token_expiry.contains("expected_process_instance_id"));
    assert!(!token_expiry.contains("receipt_authority_observe_v1"));
    assert!(!migration.contains(
        "public.starring_runtime_interaction_receipt_token_expire_v1(text,text,bigint,bigint,text,bytea)"
    ));
    let terminalize = migration
        .split(
            "CREATE FUNCTION public.starring_runtime_interaction_receipt_terminalize_expired_v1(",
        )
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    assert!(terminalize.contains("root_row.origin_gateway_shard_id"));
    assert!(terminalize.contains("head.head_revision = expected_head_revision"));
    assert!(terminalize.contains("head.claim_revision = expected_claim_revision"));
    assert!(terminalize.contains("WHEN SQLSTATE 'RI004'"));
    assert!(!terminalize.contains("token_ciphertext"));
    assert!(!terminalize.contains("root_route_key"));
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn teardown_migration_is_ordered_idempotent_bounded_and_private() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let certification = versions
        .iter()
        .position(|version| *version == 202_607_300_003)
        .unwrap();
    let teardown = versions
        .iter()
        .position(|version| *version == 202_607_300_004)
        .unwrap();
    assert_eq!(teardown, certification + 1);

    let migration =
        include_str!("../../../migrations/202607300004_add_runtime_interaction_teardown_v1.sql");
    for function in [
        "starring_runtime_interaction_instance_get_for_teardown_v1",
        "starring_runtime_interaction_instance_claim_deleting_v1",
        "starring_runtime_interaction_instance_mark_deleted_v1",
        "starring_runtime_interaction_instance_list_retryable_v1",
    ] {
        assert_eq!(
            migration
                .matches(&format!("CREATE FUNCTION public.{function}("))
                .count(),
            1
        );
    }
    for required in [
        "CREATE OR REPLACE FUNCTION public.starring_runtime_interaction_database_readiness_v1()",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "expected_limit NOT BETWEEN 1 AND 256",
        "ROWS 256",
        "FOR UPDATE",
        "RETURN 'claimed'",
        "RETURN 'already_deleting'",
        "RETURN 'already_deleted'",
        "RETURN 'marked_deleted'",
        "RETURN 'conflict'",
        "RETURN 'not_found'",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE",
        "invalid_relation_acl_count",
        "invalid_attribute_count",
        "pg_get_function_arguments",
        "pg_get_function_result",
        "ORDER BY instance.instance_id COLLATE \"C\"",
        "ORDER BY route.instance_id COLLATE \"C\"",
    ] {
        assert!(migration.contains(required), "missing contract: {required}");
    }
    for forbidden in [
        "GRANT EXECUTE",
        "GRANT SELECT",
        "GRANT INSERT",
        "GRANT UPDATE",
        "GRANT DELETE",
        "COMMENT ON",
    ] {
        assert!(!migration.contains(forbidden), "{forbidden}");
    }
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn teardown_retry_scan_migration_is_ordered_key_only_bounded_and_private() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let teardown = versions
        .iter()
        .position(|version| *version == 202_607_300_004)
        .unwrap();
    let retry_scan = versions
        .iter()
        .position(|version| *version == 202_607_300_005)
        .unwrap();
    assert_eq!(retry_scan, teardown + 1);

    let migration = include_str!(
        "../../../migrations/202607300005_add_runtime_interaction_teardown_retry_scan_v2.sql"
    );
    for required in [
        "CREATE INDEX automation_instances_deleting_retry_scan_v2_idx",
        "guild_id COLLATE \"C\"",
        "instance_id COLLATE \"C\"",
        "WHERE status = 'deleting'",
        "CREATE FUNCTION public.starring_runtime_interaction_instance_scan_retryable_v2(",
        "through_guild_id TEXT",
        "through_instance_id TEXT",
        "expected_limit NOT BETWEEN 1 AND 256",
        "ROWS 256",
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "ORDER BY\n            instance.guild_id COLLATE \"C\" DESC",
        "ORDER BY\n        instance.guild_id COLLATE \"C\"",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE",
        "starring_runtime_interaction_schema_manifest_v1",
        "starring_runtime_interaction_database_readiness_v1",
        "pg_get_function_arguments",
        "pg_get_function_result",
    ] {
        assert!(migration.contains(required), "missing contract: {required}");
    }
    for forbidden in [
        "ruleset_key TEXT",
        "resources JSONB",
        "GRANT EXECUTE",
        "GRANT SELECT",
        "GRANT INSERT",
        "GRANT UPDATE",
        "GRANT DELETE",
        "COMMENT ON",
    ] {
        assert!(!migration.contains(forbidden), "{forbidden}");
    }
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn interaction_migration_is_private_bounded_and_comment_free() {
    let migration =
        include_str!("../../../migrations/202607220027_scope_runtime_interaction_database.sql");
    for function in [
        "starring_runtime_interaction_database_identity_v1",
        "starring_runtime_interaction_database_readiness_v1",
        "starring_runtime_interaction_route_read_v1",
        "starring_runtime_interaction_pinned_read_v1",
        "starring_runtime_interaction_instance_register_v1",
    ] {
        assert_eq!(
            migration
                .matches(&format!("CREATE FUNCTION public.{function}("))
                .count(),
            1
        );
    }
    for required in [
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "ROWS 1",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE",
        "REVOKE ALL PRIVILEGES ON TABLE %s FROM PUBLIC CASCADE",
        "REVOKE ALL PRIVILEGES (%I) ON TABLE %s FROM %I CASCADE",
        "pg_parameter_acl",
        "pg_largeobject_metadata",
        "privilege.is_grantable",
        "trigger_row.tgtype",
        "runtime_interaction_instance_identity_mutation_rejected",
        "runtime_interaction_instance_destructive_mutation_rejected",
        "RETURN 'created'",
        "RETURN 'exact_replay'",
        "RETURN 'conflict'",
    ] {
        assert!(migration.contains(required), "missing contract: {required}");
    }
    for forbidden in ["CREATE ROLE", "GRANT EXECUTE", "COMMENT ON"] {
        assert!(!migration.contains(forbidden));
    }
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[tokio::test]
#[ignore]
async fn teardown_retry_scan_upgrades_cleanly_from_teardown_v1() {
    let database = isolated_database_with_upgrade_boundary(Some(202_607_300_004)).await;
    let applied: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public._sqlx_migrations \
         WHERE version = 202607300005 AND success",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(applied, 1);
    let executable: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_function_privilege( \
             $1, pg_catalog.to_regprocedure($2), 'EXECUTE' \
         )",
    )
    .bind(&database.role)
    .bind(TEARDOWN_RETRY_SCAN_FUNCTION)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert!(executable);
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn durable_receipt_recovery_scan_is_empty_and_private() {
    let database = isolated_database().await;
    let database_identity: String = sqlx::query_scalar(
        "SELECT database_identity::TEXT FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let expectation = RuntimeInteractionDatabaseExpectationV1::new(
        database_identity,
        database.name.clone(),
        database.role.clone(),
    )
    .unwrap();
    let store = PostgresRuntimeInteractionV1::connect_verified_default(
        database.executor_pool.clone(),
        expectation,
    )
    .await
    .unwrap();
    let page = store
        .scan_recoverable_interaction_receipts_v1(
            &RuntimeInteractionReceiptRecoveryScanCursorV1::default(),
            NonZeroUsize::new(1).unwrap(),
        )
        .await
        .unwrap();
    assert!(page.candidates().is_empty());
    assert!(page.through().is_none());
    assert!(page.observed_database_now().is_none());
    assert!(page.exhausted());
    let cross_error = sqlx::query(
        "SELECT * FROM public.starring_runtime_interaction_receipt_scan_recoverable_v1(\
            '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', \
            '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', 1\
         )",
    )
    .fetch_all(&database.cross_pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&cross_error).as_deref(), Some("42501"));
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn durable_receipt_restricted_role_runs_lifecycle_recovery_and_token_expiry() {
    let database = isolated_database().await;
    let content_hash = seed_receipt_authority(&database.owner_pool).await;
    let database_identity: String = sqlx::query_scalar(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let expectation = RuntimeInteractionDatabaseExpectationV1::new(
        database_identity,
        database.name.clone(),
        database.role.clone(),
    )
    .unwrap();
    let store = PostgresRuntimeInteractionV1::connect_verified_default(
        database.executor_pool.clone(),
        expectation,
    )
    .await
    .unwrap();

    let concurrent_id = 9_200_013;
    let concurrent_left = test_receipt_request(
        &store,
        &content_hash,
        concurrent_id,
        "button:concurrent",
        Duration::from_secs(30),
    )
    .await;
    let concurrent_right = test_receipt_request(
        &store,
        &content_hash,
        concurrent_id,
        "button:concurrent",
        Duration::from_secs(30),
    )
    .await;
    let (concurrent_left, concurrent_right) = tokio::join!(
        store.claim_interaction_receipt_v1(concurrent_left),
        store.claim_interaction_receipt_v1(concurrent_right),
    );
    let concurrent_outcomes = [concurrent_left.unwrap(), concurrent_right.unwrap()];
    assert_eq!(
        concurrent_outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    RuntimeInteractionReceiptClaimOutcomeV1::Acquired(_)
                )
            })
            .count(),
        1
    );
    assert_eq!(
        concurrent_outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    RuntimeInteractionReceiptClaimOutcomeV1::InFlightDuplicate(_)
                )
            })
            .count(),
        1
    );

    let renewal_id = 9_200_012;
    acquire_test_receipt(
        &store,
        &content_hash,
        renewal_id,
        "button:renewal",
        Duration::from_secs(30),
    )
    .await;
    for table in ["runtime_serving_leases", "runtime_gateway_owners"] {
        sqlx::query(&format!("ALTER TABLE public.{table} DISABLE TRIGGER ALL"))
            .execute(&database.owner_pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "UPDATE public.runtime_serving_leases \
         SET revision = revision + 1, \
             last_heartbeat_at = pg_catalog.clock_timestamp(), \
             expires_at = pg_catalog.clock_timestamp() + INTERVAL '5 minutes' \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .execute(&database.owner_pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_gateway_owners \
         SET owner_revision = owner_revision + 1, \
             expires_at = pg_catalog.clock_timestamp() + INTERVAL '5 minutes' \
         WHERE gateway_shard_id = $1",
    )
    .bind(RECEIPT_GATEWAY_SHARD)
    .execute(&database.owner_pool)
    .await
    .unwrap();
    for table in ["runtime_serving_leases", "runtime_gateway_owners"] {
        sqlx::query(&format!("ALTER TABLE public.{table} ENABLE TRIGGER ALL"))
            .execute(&database.owner_pool)
            .await
            .unwrap();
    }
    assert!(matches!(
        store
            .claim_interaction_receipt_v1(
                test_receipt_request(
                    &store,
                    &content_hash,
                    renewal_id,
                    "button:renewal",
                    Duration::from_secs(30),
                )
                .await,
            )
            .await
            .unwrap(),
        RuntimeInteractionReceiptClaimOutcomeV1::InFlightDuplicate(_)
    ));
    assert!(matches!(
        store
            .claim_interaction_receipt_v1(
                test_receipt_request_with_digest(
                    &store,
                    &content_hash,
                    renewal_id,
                    "button:renewal",
                    Duration::from_secs(30),
                    "f".repeat(64),
                )
                .await,
            )
            .await,
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    ));
    assert!(matches!(
        store
            .claim_interaction_receipt_v1(
                test_receipt_request(
                    &store,
                    &content_hash,
                    renewal_id,
                    "button:route-drift",
                    Duration::from_secs(30),
                )
                .await,
            )
            .await,
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    ));
    sqlx::query("ALTER TABLE public.runtime_serving_leases DISABLE TRIGGER ALL")
        .execute(&database.owner_pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_serving_leases SET lease_epoch = 2 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .execute(&database.owner_pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .claim_interaction_receipt_v1(
                test_receipt_request(
                    &store,
                    &content_hash,
                    renewal_id,
                    "button:renewal",
                    Duration::from_secs(30),
                )
                .await,
            )
            .await,
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    ));
    sqlx::query(
        "UPDATE public.runtime_serving_leases SET lease_epoch = 1 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .execute(&database.owner_pool)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_serving_leases ENABLE TRIGGER ALL")
        .execute(&database.owner_pool)
        .await
        .unwrap();

    let lifecycle_id = 9_200_001;
    let mut lifecycle = acquire_test_receipt(
        &store,
        &content_hash,
        lifecycle_id,
        "button:lifecycle",
        Duration::from_secs(30),
    )
    .await;
    let plan_digest = InteractionActionPlanDigestV1::parse("1".repeat(64)).unwrap();
    assert_eq!(
        store
            .bind_interaction_receipt_action_plan_v1(&mut lifecycle, plan_digest.clone())
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(
        store
            .bind_interaction_receipt_action_plan_v1(&mut lifecycle, plan_digest)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::ExactReplay
    );
    let intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([2; 32]);
    let intent = RuntimeInteractionReceiptInitialResponseIntentV1::new(
        RuntimeInteractionReceiptInitialResponseKindV1::RespondEphemeral,
        intent_digest.clone(),
    );
    assert_eq!(
        store
            .intend_interaction_receipt_initial_response_v1(&mut lifecycle, intent.clone())
            .await
            .unwrap(),
        RuntimeInteractionReceiptInitialResponseIntentDispositionV1::ExternalCallAuthorized
    );
    assert_eq!(
        store
            .intend_interaction_receipt_initial_response_v1(&mut lifecycle, intent)
            .await
            .unwrap(),
        RuntimeInteractionReceiptInitialResponseIntentDispositionV1::ExactReplaySuppressed
    );
    let response_result = RuntimeInteractionReceiptInitialResponseResultV1::new(
        intent_digest,
        RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
        RuntimeInteractionReceiptOpaqueDigestV1::new([3; 32]),
    );
    assert_eq!(
        store
            .finish_interaction_receipt_initial_response_v1(
                &mut lifecycle,
                response_result.clone(),
            )
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(
        store
            .finish_interaction_receipt_initial_response_v1(&mut lifecycle, response_result)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::ExactReplay
    );
    assert_eq!(
        store
            .intend_interaction_receipt_execution_v1(&mut lifecycle)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(
        store
            .intend_interaction_receipt_execution_v1(&mut lifecycle)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::ExactReplay
    );
    let early_expiry = RuntimeInteractionReceiptTokenExpiryRequestV1::new(
        lifecycle.claim_root().identity(),
        lifecycle.head_revision(),
        lifecycle.claim_revision(),
        RuntimeInteractionReceiptOpaqueDigestV1::new([5; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .expire_interaction_receipt_token_v1(early_expiry)
            .await
            .unwrap()
            .disposition(),
        RuntimeInteractionReceiptTokenExpiryDispositionV1::TokenNotExpired
    );
    let terminal = RuntimeInteractionReceiptTerminalOutcomeV1::new(
        RuntimeInteractionReceiptTerminalStateV1::Completed,
        "completed_test",
        RuntimeInteractionReceiptOpaqueDigestV1::new([4; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .finish_interaction_receipt_v1(&mut lifecycle, terminal.clone())
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(
        store
            .finish_interaction_receipt_v1(&mut lifecycle, terminal)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::ExactReplay
    );
    let absent_expiry = RuntimeInteractionReceiptTokenExpiryRequestV1::new(
        lifecycle.claim_root().identity(),
        lifecycle.head_revision(),
        lifecycle.claim_revision(),
        RuntimeInteractionReceiptOpaqueDigestV1::new([5; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .expire_interaction_receipt_token_v1(absent_expiry)
            .await
            .unwrap()
            .disposition(),
        RuntimeInteractionReceiptTokenExpiryDispositionV1::TokenAbsent
    );
    let terminal_noop = RuntimeInteractionReceiptTerminalizeExpiredRequestV1::new(
        lifecycle.claim_root().identity(),
        lifecycle.head_revision(),
        lifecycle.claim_revision(),
        ProcessInstanceId::parse(RECEIPT_PROCESS_ID).unwrap(),
        InteractionRuntimeBuildRevisionV1::parse(RECEIPT_BUILD_REVISION).unwrap(),
        RuntimeInteractionReceiptOpaqueDigestV1::new([12; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .terminalize_expired_interaction_receipt_v1(terminal_noop)
            .await
            .unwrap()
            .disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::TerminalReceipt
    );

    let renewed_id = 9_200_006;
    let renewed = acquire_test_receipt(
        &store,
        &content_hash,
        renewed_id,
        "button:renewed",
        Duration::from_secs(30),
    )
    .await;
    let renewed_noop = RuntimeInteractionReceiptTerminalizeExpiredRequestV1::new(
        renewed.claim_root().identity(),
        renewed.head_revision(),
        renewed.claim_revision(),
        ProcessInstanceId::parse(RECEIPT_PROCESS_ID).unwrap(),
        InteractionRuntimeBuildRevisionV1::parse(RECEIPT_BUILD_REVISION).unwrap(),
        RuntimeInteractionReceiptOpaqueDigestV1::new([13; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .terminalize_expired_interaction_receipt_v1(renewed_noop)
            .await
            .unwrap()
            .disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::ClaimRenewed
    );

    let unsafe_id = 9_200_002;
    let mut unsafe_claim = acquire_test_receipt(
        &store,
        &content_hash,
        unsafe_id,
        "button:unsafe",
        Duration::from_secs(1),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(
            &mut unsafe_claim,
            InteractionActionPlanDigestV1::parse("6".repeat(64)).unwrap(),
        )
        .await
        .unwrap();
    let recovered_id = 9_200_003;
    acquire_test_receipt(
        &store,
        &content_hash,
        recovered_id,
        "button:exact-recovered-route",
        Duration::from_secs(1),
    )
    .await;
    let expired_id = 9_200_004;
    acquire_test_receipt(
        &store,
        &content_hash,
        expired_id,
        "button:expired",
        Duration::from_secs(1),
    )
    .await;
    let unavailable_id = 9_200_005;
    acquire_test_receipt(
        &store,
        &content_hash,
        unavailable_id,
        "button:unavailable",
        Duration::from_secs(1),
    )
    .await;
    remove_interaction_token(&database.owner_pool, unavailable_id).await;
    let terminalize_id = 9_200_007;
    let mut terminalize_claim = acquire_test_receipt(
        &store,
        &content_hash,
        terminalize_id,
        "button:terminalize",
        Duration::from_secs(1),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(
            &mut terminalize_claim,
            InteractionActionPlanDigestV1::parse("7".repeat(64)).unwrap(),
        )
        .await
        .unwrap();
    let attempting_id = 9_200_008;
    let mut attempting_claim = acquire_test_receipt(
        &store,
        &content_hash,
        attempting_id,
        "button:attempting",
        Duration::from_secs(1),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(
            &mut attempting_claim,
            InteractionActionPlanDigestV1::parse("8".repeat(64)).unwrap(),
        )
        .await
        .unwrap();
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut attempting_claim,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::RespondEphemeral,
                RuntimeInteractionReceiptOpaqueDigestV1::new([14; 32]),
            ),
        )
        .await
        .unwrap();
    let pristine_id = 9_200_009;
    acquire_test_receipt(
        &store,
        &content_hash,
        pristine_id,
        "button:pristine",
        Duration::from_secs(1),
    )
    .await;
    let stale_id = 9_200_010;
    let mut stale_claim = acquire_test_receipt(
        &store,
        &content_hash,
        stale_id,
        "button:stale",
        Duration::from_secs(1),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(
            &mut stale_claim,
            InteractionActionPlanDigestV1::parse("9".repeat(64)).unwrap(),
        )
        .await
        .unwrap();
    let successor_id = 9_200_011;
    acquire_test_receipt(
        &store,
        &content_hash,
        successor_id,
        "button:successor",
        Duration::from_secs(1),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let page = store
        .scan_recoverable_interaction_receipts_v1(
            &RuntimeInteractionReceiptRecoveryScanCursorV1::default(),
            NonZeroUsize::new(16).unwrap(),
        )
        .await
        .unwrap();
    let candidate = |interaction_id: u64| {
        page.candidates()
            .iter()
            .find(|candidate| candidate.key().identity().interaction_id().get() == interaction_id)
            .cloned()
            .unwrap()
    };
    let successor_candidate = candidate(successor_id);
    let successor_recovery = store
        .recover_interaction_receipt_v1(
            RuntimeInteractionReceiptRecoveryRequestV1::new(
                successor_candidate.clone(),
                receipt_expected_route_for(&content_hash, "process-successor", "build-successor"),
                RuntimeInteractionReceiptRecoveryObservationKindV1::Unacknowledged,
                RuntimeInteractionReceiptOpaqueDigestV1::new([20; 32]),
                RuntimeInteractionReceiptClaimLeaseV1::default(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        successor_recovery,
        RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryDeferred {
            reason: RuntimeInteractionReceiptRecoveryDeferredReasonV1::SuccessorProcess,
            ..
        }
    ));
    let successor_terminalized = store
        .terminalize_expired_interaction_receipt_v1(
            RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
                &successor_candidate,
                &receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptOpaqueDigestV1::new([21; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        successor_terminalized.disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::PristineClaimAbandoned
    );
    let terminalize_candidate = candidate(terminalize_id);
    let revision_race = RuntimeInteractionReceiptTerminalizeExpiredRequestV1::new(
        terminalize_candidate.key().identity(),
        terminalize_candidate.head_revision() + 1,
        terminalize_candidate.claim_revision(),
        ProcessInstanceId::parse(RECEIPT_PROCESS_ID).unwrap(),
        InteractionRuntimeBuildRevisionV1::parse(RECEIPT_BUILD_REVISION).unwrap(),
        RuntimeInteractionReceiptOpaqueDigestV1::new([15; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .terminalize_expired_interaction_receipt_v1(revision_race)
            .await
            .unwrap()
            .disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RevisionRace
    );
    let terminalized = store
        .terminalize_expired_interaction_receipt_v1(
            RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
                &terminalize_candidate,
                &receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptOpaqueDigestV1::new([16; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        terminalized.disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RecoveryRequired
    );
    let attempting_terminalized = store
        .terminalize_expired_interaction_receipt_v1(
            RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
                &candidate(attempting_id),
                &receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptOpaqueDigestV1::new([17; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        attempting_terminalized.disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RecoveryRequired
    );
    let (acknowledgement_state, acknowledgement_result): (String, Option<String>) = sqlx::query_as(
        "SELECT acknowledgement_state, acknowledgement_result \
             FROM public.runtime_interaction_receipt_heads_v1 \
             WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(attempting_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(acknowledgement_state, "response_recovery_terminal");
    assert_eq!(acknowledgement_result.as_deref(), Some("indeterminate"));
    let pristine = store
        .terminalize_expired_interaction_receipt_v1(
            RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
                &candidate(pristine_id),
                &receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptOpaqueDigestV1::new([18; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        pristine.disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::PristineClaimAbandoned
    );
    let recovered = store
        .recover_interaction_receipt_v1(
            RuntimeInteractionReceiptRecoveryRequestV1::new(
                candidate(recovered_id),
                receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptRecoveryObservationKindV1::Unacknowledged,
                RuntimeInteractionReceiptOpaqueDigestV1::new([7; 32]),
                RuntimeInteractionReceiptClaimLeaseV1::default(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let RuntimeInteractionReceiptRecoveryOutcomeV1::Recovered(recovered) = recovered else {
        panic!("pristine receipt was not recovered")
    };
    assert_eq!(
        recovered.route().route_key(),
        "button:exact-recovered-route"
    );
    let unsafe_outcome = store
        .recover_interaction_receipt_v1(
            RuntimeInteractionReceiptRecoveryRequestV1::new(
                candidate(unsafe_id),
                receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptRecoveryObservationKindV1::MutationsReconciled,
                RuntimeInteractionReceiptOpaqueDigestV1::new([8; 32]),
                RuntimeInteractionReceiptClaimLeaseV1::default(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        unsafe_outcome,
        RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryRequired {
            reason: RuntimeInteractionReceiptRecoveryRequiredReasonV1::UnsafeToResume,
            ..
        }
    ));

    replace_with_expired_token(&database.owner_pool, lifecycle_id).await;
    replace_with_expired_token(&database.owner_pool, expired_id).await;
    sqlx::query("ALTER TABLE public.runtime_gateway_owners DISABLE TRIGGER ALL")
        .execute(&database.owner_pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_gateway_owners \
         SET expires_at = pg_catalog.clock_timestamp() - INTERVAL '1 second' \
         WHERE gateway_shard_id = $1",
    )
    .bind(RECEIPT_GATEWAY_SHARD)
    .execute(&database.owner_pool)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_gateway_owners ENABLE TRIGGER ALL")
        .execute(&database.owner_pool)
        .await
        .unwrap();
    let stale = store
        .terminalize_expired_interaction_receipt_v1(
            RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
                &candidate(stale_id),
                &receipt_expected_route(&content_hash),
                RuntimeInteractionReceiptOpaqueDigestV1::new([19; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        stale.disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RouteAuthorityStale
    );
    let expired = store
        .expire_interaction_receipt_token_v1(
            RuntimeInteractionReceiptTokenExpiryRequestV1::from_recovery_candidate(
                &candidate(expired_id),
                RuntimeInteractionReceiptOpaqueDigestV1::new([9; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        expired.disposition(),
        RuntimeInteractionReceiptTokenExpiryDispositionV1::RecoveryRequired
    );
    let unavailable = store
        .expire_interaction_receipt_token_v1(
            RuntimeInteractionReceiptTokenExpiryRequestV1::from_recovery_candidate(
                &candidate(unavailable_id),
                RuntimeInteractionReceiptOpaqueDigestV1::new([10; 32]),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        unavailable.disposition(),
        RuntimeInteractionReceiptTokenExpiryDispositionV1::RecoveryRequired
    );
    let terminal_expiry = RuntimeInteractionReceiptTokenExpiryRequestV1::new(
        lifecycle.claim_root().identity(),
        lifecycle.head_revision(),
        lifecycle.claim_revision(),
        RuntimeInteractionReceiptOpaqueDigestV1::new([11; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .expire_interaction_receipt_token_v1(terminal_expiry)
            .await
            .unwrap()
            .disposition(),
        RuntimeInteractionReceiptTokenExpiryDispositionV1::TerminalTokenDeleted
    );
    let remaining_secrets: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_interaction_receipt_token_secrets_v1 \
         WHERE application_id = $1 AND interaction_id IN ($2, $3, $4)",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(lifecycle_id.to_string())
    .bind(expired_id.to_string())
    .bind(unavailable_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(remaining_secrets, 0);
    let terminalized_secrets: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_interaction_receipt_token_secrets_v1 \
         WHERE application_id = $1 AND interaction_id IN ($2, $3, $4)",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(terminalize_id.to_string())
    .bind(attempting_id.to_string())
    .bind(successor_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(terminalized_secrets, 0);
    let unchanged_secrets: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_interaction_receipt_token_secrets_v1 \
         WHERE application_id = $1 AND interaction_id IN ($2, $3)",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(pristine_id.to_string())
    .bind(stale_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(unchanged_secrets, 1);
    let table_error =
        sqlx::query("SELECT * FROM public.runtime_interaction_receipt_token_secrets_v1")
            .fetch_all(&database.executor_pool)
            .await
            .unwrap_err();
    assert_eq!(sqlstate(&table_error).as_deref(), Some("42501"));
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn exact_capabilities_preserve_binding_inactivity_and_least_privilege() {
    let database = isolated_database().await;
    let owner_pool = database.owner_pool.clone();
    let executor_pool = database.executor_pool.clone();
    let deadline_pool = database.deadline_pool.clone();
    let database_name = database.name.clone();
    let executor_role = database.role.clone();
    let cross_role = database.cross_role.clone();
    let cross_pool = database.cross_pool.clone();
    let task = tokio::spawn(async move {
        let database_identity: String = sqlx::query_scalar(
            "SELECT database_identity::TEXT FROM public.product_control_plane_identity WHERE singleton",
        )
        .fetch_one(&owner_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.automation_ruleset_versions \
             (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
             SELECT '7', 'study', 1, 1, $1::JSONB, \
                    public.starring_ruleset_content_hash_v1(1, $1::JSONB), '4'",
        )
        .bind(r#"{"version":1,"panels":[],"modals":[],"rules":[]}"#)
        .execute(&owner_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.automation_instances \
             (guild_id, instance_id, ruleset_key, ruleset_version, kind, created_by, status, resources) \
             VALUES ('7', 'disabled_orphan', 'missing', 9, 'study_room', '3', 'disabled', '{}'::JSONB)",
        )
        .execute(&owner_pool)
        .await
        .unwrap();

        let expectation = RuntimeInteractionDatabaseExpectationV1::new(
            database_identity,
            database_name.clone(),
            executor_role.clone(),
        )
        .unwrap();
        let deadline_store =
            PostgresRuntimeInteractionV1::connect_verified_with_route_timeout(
                deadline_pool.clone(),
                expectation.clone(),
                RuntimeInteractionDatabaseTimeoutsV1::default(),
                RuntimeInteractionRouteTimeoutV1::new(Duration::from_millis(400)).unwrap(),
            )
            .await
            .unwrap();
        let store =
            PostgresRuntimeInteractionV1::connect_verified_default(executor_pool.clone(), expectation)
                .await
                .unwrap();
        let (same_left, same_right) = tokio::join!(
            store.register_instance_v1(instance("study_room")),
            store.register_instance_v1(instance("study_room"))
        );
        same_left.unwrap();
        same_right.unwrap();
        store.register_instance_v1(instance("study_room")).await.unwrap();
        assert_eq!(
            store.register_instance_v1(instance("other")).await,
            Err(InstanceStoreError::DuplicateInstance)
        );

        let mut race_left = instance("left");
        race_left.id = InstanceId::parse("race_room").unwrap();
        let mut race_right = instance("right");
        race_right.id = InstanceId::parse("race_room").unwrap();
        let (race_left, race_right) = tokio::join!(
            store.register_instance_v1(race_left),
            store.register_instance_v1(race_right)
        );
        assert!(matches!(
            (&race_left, &race_right),
            (Ok(()), Err(InstanceStoreError::DuplicateInstance))
                | (Err(InstanceStoreError::DuplicateInstance), Ok(()))
        ));

        let mut zero_creator = instance("zero_creator");
        zero_creator.id = InstanceId::parse("zero_creator").unwrap();
        zero_creator.created_by = UserId(0);
        assert_eq!(
            store.register_instance_v1(zero_creator).await,
            Err(InstanceStoreError::Backend(
                "runtime_interaction_invalid_input".to_string()
            ))
        );

        let room = InstanceId::parse("room").unwrap();
        let route = store
            .read_instance_route_v1(GuildId(7), &room)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(route.guild_id, GuildId(7));
        assert_eq!(route.id, room);
        assert!(store
            .read_instance_route_v1(GuildId(8), &InstanceId::parse("room").unwrap())
            .await
            .unwrap()
            .is_none());

        let mut teardown_instance = instance("study_room");
        teardown_instance.id = InstanceId::parse("teardown_room").unwrap();
        store
            .register_instance_v1(teardown_instance)
            .await
            .unwrap();
        let teardown_room = InstanceId::parse("teardown_room").unwrap();
        assert_eq!(
            store
                .get_for_teardown_v1(GuildId(7), &teardown_room)
                .await
                .unwrap()
                .unwrap()
                .status,
            InstanceStatus::Active
        );
        assert_eq!(
            store
                .claim_deleting_v1(GuildId(7), &teardown_room)
                .await
                .unwrap(),
            InstanceTeardownClaimOutcomeV1::Claimed
        );
        assert_eq!(
            store
                .claim_deleting_v1(GuildId(7), &teardown_room)
                .await
                .unwrap(),
            InstanceTeardownClaimOutcomeV1::AlreadyDeleting
        );
        let retryable = store
            .list_retryable_v1(GuildId(7), NonZeroUsize::new(1).unwrap())
            .await
            .unwrap();
        assert_eq!(retryable.len(), 1);
        assert_eq!(retryable[0].id, teardown_room);
        assert_eq!(
            store
                .mark_deleted_v1(GuildId(7), &teardown_room)
                .await
                .unwrap(),
            InstanceTeardownMarkOutcomeV1::MarkedDeleted
        );
        assert_eq!(
            store
                .mark_deleted_v1(GuildId(7), &teardown_room)
                .await
                .unwrap(),
            InstanceTeardownMarkOutcomeV1::AlreadyDeleted
        );
        assert_eq!(
            store
                .claim_deleting_v1(GuildId(7), &teardown_room)
                .await
                .unwrap(),
            InstanceTeardownClaimOutcomeV1::AlreadyDeleted
        );
        assert!(store
            .list_retryable_v1(GuildId(7), NonZeroUsize::new(1).unwrap())
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .list_retryable_v1(
                    GuildId(7),
                    NonZeroUsize::new(MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1 + 1).unwrap(),
                )
                .await,
            Err(InstanceStoreError::Backend(
                "runtime_interaction_invalid_input".to_string()
            ))
        );

        sqlx::query(
            "INSERT INTO public.automation_instances \
             (guild_id, instance_id, ruleset_key, ruleset_version, kind, created_by, status, resources) \
             SELECT CASE ordinal % 3 WHEN 0 THEN '2' WHEN 1 THEN '10' ELSE '7' END, \
                    'scan_' || pg_catalog.lpad(ordinal::TEXT, 4, '0'), \
                    'study', 1, 'study_room', '3', 'deleting', '{}'::JSONB \
             FROM pg_catalog.generate_series(0, 599) AS ordinal",
        )
        .execute(&owner_pool)
        .await
        .unwrap();
        let scan_limit = NonZeroUsize::new(128).unwrap();
        let mut scan_cursor = InstanceTeardownRetryScanCursorV2::initial();
        let mut scanned_keys = Vec::new();
        let mut cycle_through = None;
        loop {
            let page = store
                .scan_retryable_v2(&scan_cursor, scan_limit)
                .await
                .unwrap();
            if cycle_through.is_none() {
                cycle_through = page.through().cloned();
                sqlx::query(
                    "INSERT INTO public.automation_instances \
                     (guild_id, instance_id, ruleset_key, ruleset_version, kind, created_by, status, resources) \
                     VALUES ('99', 'inserted_later', 'study', 1, 'study_room', '3', 'deleting', '{}'::JSONB)",
                )
                .execute(&owner_pool)
                .await
                .unwrap();
            }
            assert_eq!(page.through(), cycle_through.as_ref());
            scanned_keys.extend(page.keys().iter().cloned());
            let Some(next) = page.next_cursor_v2() else {
                break;
            };
            scan_cursor = next;
        }
        assert_eq!(scanned_keys.len(), 600);
        assert_eq!(scanned_keys.first().unwrap().guild_id(), GuildId(10));
        assert!(scanned_keys
            .windows(2)
            .all(|pair| pair[0].cmp_c_v2(&pair[1]).is_lt()));
        assert!(!scanned_keys
            .iter()
            .any(|key| key.instance_id().as_str() == "inserted_later"));
        let next_cycle = store
            .scan_retryable_v2(
                &InstanceTeardownRetryScanCursorV2::initial(),
                NonZeroUsize::new(MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2).unwrap(),
            )
            .await
            .unwrap();
        assert!(next_cycle
            .through()
            .is_some_and(|key| key.instance_id().as_str() == "inserted_later"));
        assert_eq!(
            store
                .scan_retryable_v2(
                    &InstanceTeardownRetryScanCursorV2::initial(),
                    NonZeroUsize::new(MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2 + 1).unwrap(),
                )
                .await,
            Err(InstanceStoreError::Backend(
                "runtime_interaction_invalid_input".to_string()
            ))
        );

        let public_scan_grants: i64 = sqlx::query_scalar(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                 function_row.proacl, \
                 pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
                 AND privilege.grantee = 0",
        )
        .bind(TEARDOWN_RETRY_SCAN_FUNCTION)
        .fetch_one(&owner_pool)
        .await
        .unwrap();
        assert_eq!(public_scan_grants, 0);
        let cross_can_scan: bool = sqlx::query_scalar(
            "SELECT pg_catalog.has_function_privilege( \
                 $1, pg_catalog.to_regprocedure($2), 'EXECUTE' \
             )",
        )
        .bind(&cross_role)
        .bind(TEARDOWN_RETRY_SCAN_FUNCTION)
        .fetch_one(&owner_pool)
        .await
        .unwrap();
        assert!(!cross_can_scan);
        let cross_error = sqlx::query(
            "SELECT * FROM public.starring_runtime_interaction_instance_scan_retryable_v2(\
                '', '', '', '', 1\
             )",
        )
        .fetch_all(&cross_pool)
        .await
        .unwrap_err();
        assert_eq!(sqlstate(&cross_error).as_deref(), Some("42501"));

        owner_pool
            .execute(
                format!(
                    "REVOKE EXECUTE ON FUNCTION {TEARDOWN_RETRY_SCAN_FUNCTION} FROM {executor_role}"
                )
                .as_str(),
            )
            .await
            .unwrap();
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
        owner_pool
            .execute(function_grant(TEARDOWN_RETRY_SCAN_FUNCTION, &executor_role).as_str())
            .await
            .unwrap();
        store.verify_database_v1().await.unwrap();

        owner_pool
            .execute(
                "ALTER INDEX public.automation_instances_deleting_retry_scan_v2_idx \
                 RENAME TO automation_instances_deleting_retry_scan_v2_drift",
            )
            .await
            .unwrap();
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
        owner_pool
            .execute(
                "ALTER INDEX public.automation_instances_deleting_retry_scan_v2_drift \
                 RENAME TO automation_instances_deleting_retry_scan_v2_idx",
            )
            .await
            .unwrap();
        store.verify_database_v1().await.unwrap();

        let held_connection = deadline_pool.acquire().await.unwrap();
        let deadline_result = tokio::time::timeout(
            Duration::from_secs(1),
            deadline_store.read_instance_route_v1(GuildId(7), &room),
        )
        .await
        .unwrap();
        assert_eq!(
            deadline_result,
            Err(InstanceStoreError::TimedOut)
        );
        drop(held_connection);
        assert!(deadline_store
            .read_instance_route_v1(GuildId(7), &room)
            .await
            .unwrap()
            .is_some());

        let mut table_lock = owner_pool.begin().await.unwrap();
        table_lock
            .execute("LOCK TABLE public.automation_instances IN ACCESS EXCLUSIVE MODE")
            .await
            .unwrap();
        let cancelled_result = tokio::time::timeout(
            Duration::from_millis(100),
            deadline_store.read_instance_route_v1(GuildId(7), &room),
        )
        .await;
        assert!(cancelled_result.is_err());
        let replacement_after_cancellation =
            tokio::time::timeout(Duration::from_secs(1), deadline_pool.acquire())
                .await
                .unwrap()
                .unwrap();
        drop(replacement_after_cancellation);
        let locked_result = tokio::time::timeout(
            Duration::from_secs(1),
            deadline_store.read_instance_route_v1(GuildId(7), &room),
        )
        .await
        .unwrap();
        assert_eq!(locked_result, Err(InstanceStoreError::TimedOut));
        let replacement = tokio::time::timeout(Duration::from_secs(1), deadline_pool.acquire())
            .await
            .unwrap()
            .unwrap();
        drop(replacement);
        table_lock.rollback().await.unwrap();
        assert!(deadline_store
            .read_instance_route_v1(GuildId(7), &room)
            .await
            .unwrap()
            .is_some());

        let resolved = store
            .resolve_pinned_instance_v1(GuildId(7), &InstanceId::parse("room").unwrap())
            .await
            .unwrap();
        assert_eq!(resolved.artifact.created_by, UserId(4));
        assert_eq!(resolved.artifact.guild_id, GuildId(7));
        assert_eq!(resolved.artifact.ruleset_key.as_str(), "study");
        assert_eq!(resolved.artifact.version.get(), 1);

        let inactive = store
            .resolve_pinned_instance_v1(
                GuildId(7),
                &InstanceId::parse("disabled_orphan").unwrap(),
            )
            .await;
        assert_eq!(
            inactive,
            Err(PinnedInstanceResolverErrorV1::InstanceInactive(
                InstanceStatus::Disabled
            ))
        );

        let table_error = sqlx::query("SELECT * FROM public.automation_instances")
            .execute(&executor_pool)
            .await
            .unwrap_err();
        assert_eq!(sqlstate(&table_error).as_deref(), Some("42501"));

        for invalid_query in [
            "SELECT * FROM public.starring_runtime_interaction_route_read_v1('bad', 'room')",
            "SELECT * FROM public.starring_runtime_interaction_route_read_v1('0', 'room')",
            "SELECT public.starring_runtime_interaction_instance_register_v1(\
                '7', 'zero_resource', 'study', 1, 'study_room', '3', \
                '{\"roles\":{\"member\":\"0\"}}'::JSONB\
             )",
        ] {
            let invalid_error = sqlx::query(invalid_query)
                .fetch_all(&executor_pool)
                .await
                .unwrap_err();
            assert_eq!(sqlstate(&invalid_error).as_deref(), Some("RI003"));
        }

        let missing_error = sqlx::query(
            "SELECT public.starring_runtime_interaction_instance_register_v1(\
                '7', 'missing_room', 'missing', 1, 'study_room', '3', '{}'::JSONB\
             )",
        )
        .execute(&executor_pool)
        .await
        .unwrap_err();
        assert_eq!(sqlstate(&missing_error).as_deref(), Some("RI002"));

        sqlx::query(
            "UPDATE public.automation_instances SET status = 'disabled' \
             WHERE guild_id = '7' AND instance_id = 'room'",
        )
        .execute(&owner_pool)
        .await
        .unwrap();
        for statement in [
            "UPDATE public.automation_instances SET resources = '{}'::JSONB WHERE guild_id = '7' AND instance_id = 'room'",
            "DELETE FROM public.automation_instances WHERE guild_id = '7' AND instance_id = 'room'",
            "TRUNCATE TABLE public.automation_instances",
        ] {
            let error = sqlx::query(statement)
                .execute(&owner_pool)
                .await
                .unwrap_err();
            assert_eq!(sqlstate(&error).as_deref(), Some("RI001"));
        }

        let table_grants: i64 = sqlx::query_scalar(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_class AS relation \
             INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = 'public' AND relation.relkind = 'r' \
               AND (pg_catalog.has_table_privilege($1, relation.oid, 'SELECT') \
                    OR pg_catalog.has_table_privilege($1, relation.oid, 'INSERT') \
                    OR pg_catalog.has_table_privilege($1, relation.oid, 'UPDATE') \
                    OR pg_catalog.has_table_privilege($1, relation.oid, 'DELETE'))",
        )
        .bind(&executor_role)
        .fetch_one(&owner_pool)
        .await
        .unwrap();
        assert_eq!(table_grants, 0);

        let column_grants: i64 = sqlx::query_scalar(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_attribute AS attribute \
             INNER JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = 'public' AND attribute.attnum > 0 AND NOT attribute.attisdropped \
               AND (pg_catalog.has_column_privilege($1, relation.oid, attribute.attname, 'SELECT') \
                    OR pg_catalog.has_column_privilege($1, relation.oid, attribute.attname, 'INSERT') \
                    OR pg_catalog.has_column_privilege($1, relation.oid, attribute.attname, 'UPDATE') \
                    OR pg_catalog.has_column_privilege($1, relation.oid, attribute.attname, 'REFERENCES'))",
        )
        .bind(&executor_role)
        .fetch_one(&owner_pool)
        .await
        .unwrap();
        assert_eq!(column_grants, 0);

        for (grant, revoke) in [
            (
                format!(
                    "GRANT CONNECT ON DATABASE {database_name} TO {executor_role} WITH GRANT OPTION"
                ),
                format!(
                    "REVOKE GRANT OPTION FOR CONNECT ON DATABASE {database_name} FROM {executor_role}"
                ),
            ),
            (
                format!("GRANT USAGE ON SCHEMA public TO {executor_role} WITH GRANT OPTION"),
                format!(
                    "REVOKE GRANT OPTION FOR USAGE ON SCHEMA public FROM {executor_role}"
                ),
            ),
            (
                format!("GRANT SELECT ON TABLE pg_catalog.pg_database TO {executor_role}"),
                format!("REVOKE SELECT ON TABLE pg_catalog.pg_database FROM {executor_role}"),
            ),
            (
                format!("GRANT SET ON PARAMETER work_mem TO {executor_role}"),
                format!("REVOKE SET ON PARAMETER work_mem FROM {executor_role}"),
            ),
        ] {
            owner_pool.execute(grant.as_str()).await.unwrap();
            assert_eq!(
                store.verify_database_v1().await,
                Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
            );
            assert!(store
                .read_instance_route_v1(
                    GuildId(7),
                    &InstanceId::parse("race_room").unwrap(),
                )
                .await
                .unwrap()
                .is_some());
            owner_pool.execute(revoke.as_str()).await.unwrap();
            store.verify_database_v1().await.unwrap();
        }

        owner_pool
            .execute(
                format!("REVOKE EXECUTE ON FUNCTION {IDENTITY_FUNCTION} FROM {executor_role}")
                    .as_str(),
            )
            .await
            .unwrap();
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
        assert_eq!(
            store
                .read_instance_route_v1(
                    GuildId(7),
                    &InstanceId::parse("race_room").unwrap(),
                )
                .await,
            Err(InstanceStoreError::Backend(
                "runtime_interaction_unavailable".to_string()
            ))
        );
        owner_pool
            .execute(function_grant(IDENTITY_FUNCTION, &executor_role).as_str())
            .await
            .unwrap();
        store.verify_database_v1().await.unwrap();
        assert!(store
            .read_instance_route_v1(
                GuildId(7),
                &InstanceId::parse("race_room").unwrap(),
            )
            .await
            .unwrap()
            .is_some());

        let large_object: i32 = sqlx::query_scalar("SELECT pg_catalog.lo_create(0)::INTEGER")
            .fetch_one(&owner_pool)
            .await
            .unwrap();
        owner_pool
            .execute(
                format!("GRANT SELECT ON LARGE OBJECT {large_object} TO {executor_role}").as_str(),
            )
            .await
            .unwrap();
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
        owner_pool
            .execute(
                format!("REVOKE SELECT ON LARGE OBJECT {large_object} FROM {executor_role}")
                    .as_str(),
            )
            .await
            .unwrap();
        store.verify_database_v1().await.unwrap();

        owner_pool
            .execute(
                "DROP TRIGGER automation_ruleset_versions_reject_mutation \
                 ON public.automation_ruleset_versions",
            )
            .await
            .unwrap();
        owner_pool
            .execute(
                "CREATE TRIGGER automation_ruleset_versions_reject_mutation \
                 BEFORE UPDATE ON public.automation_ruleset_versions \
                 FOR EACH STATEMENT \
                 EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()",
            )
            .await
            .unwrap();
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
        owner_pool
            .execute(
                "DROP TRIGGER automation_ruleset_versions_reject_mutation \
                 ON public.automation_ruleset_versions",
            )
            .await
            .unwrap();
        owner_pool
            .execute(
                "CREATE TRIGGER automation_ruleset_versions_reject_mutation \
                 BEFORE UPDATE OR DELETE ON public.automation_ruleset_versions \
                 FOR EACH STATEMENT \
                 EXECUTE FUNCTION public.reject_ruleset_artifact_mutation()",
            )
            .await
            .unwrap();
        store.verify_database_v1().await.unwrap();

        owner_pool
            .execute(
                "ALTER TABLE public.automation_instances \
                 DROP CONSTRAINT automation_instances_pkey",
            )
            .await
            .unwrap();
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
        owner_pool
            .execute(
                "ALTER TABLE public.automation_instances \
                 ADD CONSTRAINT automation_instances_pkey \
                 PRIMARY KEY (guild_id, instance_id)",
            )
            .await
            .unwrap();
        store.verify_database_v1().await.unwrap();

        owner_pool
            .execute("CREATE SCHEMA interaction_shadow")
            .await
            .unwrap();
        owner_pool
            .execute(
                "CREATE FUNCTION interaction_shadow.starring_runtime_interaction_route_read_v1(TEXT,TEXT) \
                 RETURNS TABLE(guild_id TEXT, instance_id TEXT, ruleset_key TEXT, ruleset_version BIGINT, \
                               kind TEXT, created_by TEXT, status TEXT, resources JSONB) \
                 LANGUAGE sql AS 'SELECT ''attacker''::TEXT, NULL::TEXT, NULL::TEXT, NULL::BIGINT, \
                                         NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::JSONB'",
            )
            .await
            .unwrap();
        owner_pool
            .execute(format!("GRANT USAGE ON SCHEMA interaction_shadow TO {executor_role}").as_str())
            .await
            .unwrap();
        owner_pool
            .execute(
                format!(
                    "GRANT EXECUTE ON FUNCTION interaction_shadow.starring_runtime_interaction_route_read_v1(TEXT,TEXT) TO {executor_role}"
                )
                .as_str(),
            )
            .await
            .unwrap();
        let mut hostile_connection = executor_pool.acquire().await.unwrap();
        hostile_connection
            .execute("SET search_path = interaction_shadow, public")
            .await
            .unwrap();
        let observed_guild: String = sqlx::query_scalar(
            "SELECT guild_id FROM public.starring_runtime_interaction_route_read_v1('7', 'room')",
        )
        .fetch_one(&mut *hostile_connection)
        .await
        .unwrap();
        assert_eq!(observed_guild, "7");
        drop(hostile_connection);
        assert_eq!(
            store.verify_database_v1().await,
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
    })
    .await;
    cleanup(database).await;
    task.unwrap();
}
