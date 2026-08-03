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
    InteractionReceiptClaimCandidateV1, InteractionReceiptIdentityV1, InteractionReceiptStateV1,
    InteractionRequestDigestV1, InteractionRouteIncarnationV1, InteractionRuntimeBuildRevisionV1,
    InteractionTokenAuthenticatedDataInputV1, InteractionTokenEnvelopeTimeV1,
    XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_V1,
    XCHACHA20_POLY1305_INTERACTION_TOKEN_SUITE_VERSION_V1,
};
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionEffectResponseTailScanCursorV1,
    RuntimeInteractionPersistenceErrorV1, RuntimeInteractionReceiptClaimLeaseV1,
    RuntimeInteractionReceiptClaimOutcomeV1, RuntimeInteractionReceiptClaimRequestV1,
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
const EFFECT_PLAN_BIND_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_plan_bind_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA,BYTEA,BYTEA,JSONB)";
const EFFECT_INTEND_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_intend_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA,BIGINT,BIGINT,BYTEA,BYTEA,BYTEA,JSONB,BYTEA,JSONB,BIGINT)";
const EFFECT_FINISH_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_finish_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BYTEA,BIGINT,BIGINT,BYTEA,TEXT,TEXT)";
const EFFECT_RECOVERY_SCAN_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_scan_recoverable_v1(TIMESTAMPTZ,TEXT,TEXT,BIGINT,TIMESTAMPTZ,TEXT,TEXT,BIGINT,BIGINT)";
const EFFECT_RECOVERY_CLAIM_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_recovery_claim_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)";
const EFFECT_RECONCILE_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_reconcile_v1(TEXT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,TEXT,BYTEA,TEXT,BYTEA,TEXT,BIGINT)";
const EFFECT_COMPENSATION_INTEND_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_compensation_intend_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BYTEA,BYTEA,BIGINT)";
const EFFECT_COMPENSATION_FINISH_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_compensation_finish_v1(TEXT,TEXT,BIGINT,BIGINT,BIGINT,TEXT,BYTEA,TEXT,BYTEA,BIGINT)";
const EFFECT_RESPONSE_TAIL_SCAN_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_response_tail_scan_v1(TIMESTAMPTZ,TEXT,TEXT,BIGINT,TIMESTAMPTZ,TEXT,TEXT,BIGINT,BIGINT)";
const EFFECT_RESPONSE_TAIL_CLAIM_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_response_tail_claim_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BYTEA,BYTEA,BYTEA,BIGINT)";
const EFFECT_RESPONSE_TAIL_FINALIZE_FUNCTION: &str =
    "public.starring_runtime_interaction_effect_response_tail_finalize_v1(TEXT,TEXT,BIGINT,BIGINT,TEXT,BIGINT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BYTEA,BYTEA,TEXT,BYTEA,BYTEA,BIGINT)";

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

async fn assert_response_tail_scan_fix_checksum(pool: &PgPool) {
    let expected = MIGRATOR
        .iter()
        .find(|migration| migration.version == 202_608_010_002)
        .unwrap()
        .checksum
        .as_ref()
        .to_vec();
    let applied: Vec<u8> = sqlx::query_scalar(
        "SELECT checksum FROM public._sqlx_migrations \
         WHERE version = 202608010002 AND success",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(applied, expected);
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
        let later_migration_count: i64 = sqlx::query_scalar(
            "SELECT pg_catalog.count(*) FROM public._sqlx_migrations \
             WHERE version > $1 AND success",
        )
        .bind(boundary)
        .fetch_one(&owner_pool)
        .await
        .unwrap();
        assert_eq!(later_migration_count, 0);
        let boundary_identity = if boundary <= 202_607_300_004 {
            Some(
                "public.starring_runtime_interaction_instance_scan_retryable_v2(text,text,text,text,bigint)",
            )
        } else if boundary < 202_608_010_001 {
            Some("public.starring_runtime_interaction_effect_schema_manifest_v1()")
        } else {
            None
        };
        if let Some(boundary_identity) = boundary_identity {
            assert!(
                sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NULL")
                    .bind(boundary_identity)
                    .fetch_one(&owner_pool)
                    .await
                    .unwrap()
            );
        }
        if boundary == 202_608_010_001 {
            let error = sqlx::query(
                "SELECT * FROM \
                 public.starring_runtime_interaction_effect_response_tail_scan_v1(\
                     '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', -1, \
                     '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', -1, 1\
                 )",
            )
            .fetch_all(&owner_pool)
            .await
            .unwrap_err();
            assert_eq!(sqlstate(&error).as_deref(), Some("42883"));
        }
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
        function_grant(EFFECT_PLAN_BIND_FUNCTION, &role),
        function_grant(EFFECT_INTEND_FUNCTION, &role),
        function_grant(EFFECT_FINISH_FUNCTION, &role),
        function_grant(EFFECT_RECOVERY_SCAN_FUNCTION, &role),
        function_grant(EFFECT_RECOVERY_CLAIM_FUNCTION, &role),
        function_grant(EFFECT_RECONCILE_FUNCTION, &role),
        function_grant(EFFECT_COMPENSATION_INTEND_FUNCTION, &role),
        function_grant(EFFECT_COMPENSATION_FINISH_FUNCTION, &role),
        function_grant(EFFECT_RESPONSE_TAIL_SCAN_FUNCTION, &role),
        function_grant(EFFECT_RESPONSE_TAIL_CLAIM_FUNCTION, &role),
        function_grant(EFFECT_RESPONSE_TAIL_FINALIZE_FUNCTION, &role),
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

fn explain_has_seq_scan(plan: &serde_json::Value, relation: &str) -> bool {
    match plan {
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| explain_has_seq_scan(value, relation)),
        serde_json::Value::Object(values) => {
            values.get("Node Type").and_then(serde_json::Value::as_str) == Some("Seq Scan")
                && values
                    .get("Relation Name")
                    .and_then(serde_json::Value::as_str)
                    == Some(relation)
                || values
                    .values()
                    .any(|value| explain_has_seq_scan(value, relation))
        }
        _ => false,
    }
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

async fn advance_receipt_authority_to_successor(
    pool: &PgPool,
    process_instance_id: &str,
) -> String {
    let request_bytes = br#"{"fixture":"durable-receipt-successor"}"#.to_vec();
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
    for table in [
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
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_attestations \
         SELECT (pg_catalog.jsonb_populate_record( \
             NULL::public.runtime_attestations, \
             pg_catalog.to_jsonb(attestation) || pg_catalog.jsonb_build_object( \
                 'attestation_id', $1::TEXT, \
                 'attestation_digest', $1::TEXT, \
                 'deployment_revision', 2, \
                 'process_instance_id', $2::TEXT, \
                 'convergence_attempt_no', 2, \
                 'controller_fencing_token', 2, \
                 'v2_route_incarnation', 7, \
                 'v2_operation_id', 'fedcba0987654321fedcba0987654321', \
                 'v2_request_digest', $3::TEXT, \
                 'v2_request_bytes', pg_catalog.to_jsonb($4::BYTEA), \
                 'v2_live_attestation_bytes', pg_catalog.to_jsonb($5::BYTEA), \
                 'v2_route_admission', \
                     '{\"gateway_owner_lease_id\":{\"lease_epoch\":2},\"fixture\":\"successor\"}'::JSONB \
             ) \
         )).* \
         FROM public.runtime_attestations AS attestation \
         WHERE attestation.deployment_id = $6 AND attestation.deployment_revision = 1",
    )
    .bind(&attestation_id)
    .bind(process_instance_id)
    .bind(&request_digest)
    .bind(request_bytes)
    .bind(live_bytes)
    .bind(RECEIPT_DEPLOYMENT_ID)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET revision = 2, convergence_attempt_no = 2, live_attestation_id = $1, \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE deployment_id = $2",
    )
    .bind(&attestation_id)
    .bind(RECEIPT_DEPLOYMENT_ID)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_serving_leases \
         SET attestation_id = $1, process_instance_id = $2, lease_epoch = 2, \
             revision = revision + 1, acquired_at = pg_catalog.clock_timestamp(), \
             last_heartbeat_at = pg_catalog.clock_timestamp(), \
             expires_at = pg_catalog.clock_timestamp() + INTERVAL '5 minutes' \
         WHERE guild_id = $3 AND ruleset_key = $4",
    )
    .bind(&attestation_id)
    .bind(process_instance_id)
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_gateway_owners \
         SET process_instance_id = $1, lease_epoch = 2, owner_revision = owner_revision + 1, \
             expires_at = pg_catalog.clock_timestamp() + INTERVAL '5 minutes' \
         WHERE gateway_shard_id = $2",
    )
    .bind(process_instance_id)
    .bind(RECEIPT_GATEWAY_SHARD)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    for table in [
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
    attestation_id
}

fn receipt_expected_route(content_hash: &str) -> InteractionExpectedRouteV1 {
    receipt_expected_route_for(content_hash, RECEIPT_PROCESS_ID, RECEIPT_BUILD_REVISION)
}

fn receipt_expected_route_for(
    content_hash: &str,
    process_instance_id: &str,
    runtime_build_revision: &str,
) -> InteractionExpectedRouteV1 {
    receipt_expected_route_for_authority(
        content_hash,
        process_instance_id,
        runtime_build_revision,
        1,
        1,
    )
}

fn receipt_expected_route_for_authority(
    content_hash: &str,
    process_instance_id: &str,
    runtime_build_revision: &str,
    controller_fencing_token: u64,
    route_incarnation: u64,
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
        FencingToken::new(controller_fencing_token).unwrap(),
        InteractionRouteIncarnationV1::new(route_incarnation).unwrap(),
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
    test_receipt_request_with_route(
        store,
        content_hash,
        interaction_id,
        RuntimeInteractionReceiptRouteV1::static_route(route_key).unwrap(),
        lease,
        request_digest,
    )
    .await
}

async fn test_receipt_request_with_route(
    store: &PostgresRuntimeInteractionV1,
    content_hash: &str,
    interaction_id: u64,
    route: RuntimeInteractionReceiptRouteV1,
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
        .observe_interaction_receipt_authority_v1(candidate, route)
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

fn create_role_effect_action(action_index: u8) -> serde_json::Value {
    let dependency_indices = action_index
        .checked_sub(1)
        .map_or_else(Vec::new, |dependency| vec![dependency]);
    serde_json::json!({
        "action_index": action_index,
        "action_kind": "create_role",
        "dependency_indices": dependency_indices,
        "planned_identity_digest": "1".repeat(64),
        "input_digest": "2".repeat(64),
        "expected_postimage_digest": "3".repeat(64),
        "planned_recovery_input": {
            "references": [{
                "slot": "guild_id",
                "source": "existing",
                "id": RECEIPT_GUILD_ID.to_string()
            }]
        },
        "planned_preimage_digest": "4".repeat(64),
        "planned_preimage": {"kind": "none"},
        "output_kind": "created_role",
        "correlation_class": "audit_log_reason",
        "correlation_digest": "5".repeat(64),
        "correlation_marker": "6".repeat(64)
    })
}

fn edit_response_effect_action(action_index: u8) -> serde_json::Value {
    let dependency_indices = action_index
        .checked_sub(1)
        .map_or_else(Vec::new, |dependency| vec![dependency]);
    serde_json::json!({
        "action_index": action_index,
        "action_kind": "edit_response",
        "dependency_indices": dependency_indices,
        "planned_identity_digest": "7".repeat(64),
        "input_digest": "8".repeat(64),
        "expected_postimage_digest": "9".repeat(64),
        "planned_recovery_input": {
            "references": [],
            "payload_digest": "a".repeat(64)
        },
        "planned_preimage_digest": "b".repeat(64),
        "planned_preimage": {"kind": "none"},
        "output_kind": "original_response",
        "correlation_class": "interaction_receipt",
        "correlation_digest": "c".repeat(64),
        "correlation_marker": null
    })
}

async fn bind_effect_plan_document(
    pool: &PgPool,
    claim: &automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1,
    plan_digest: &InteractionActionPlanDigestV1,
    actions: serde_json::Value,
) -> Result<(String, i16), sqlx::Error> {
    sqlx::query_as(
        "SELECT outcome_name, resulting_action_count \
         FROM public.starring_runtime_interaction_effect_plan_bind_v1(\
             $1, $2, $3, $4, $5, pg_catalog.decode($6, 'hex'), \
             pg_catalog.decode(pg_catalog.repeat('d', 64), 'hex'), \
             pg_catalog.decode(pg_catalog.repeat('e', 64), 'hex'), $7::JSONB\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(
        claim
            .claim_root()
            .identity()
            .interaction_id()
            .get()
            .to_string(),
    )
    .bind(i64::try_from(claim.head_revision()).unwrap())
    .bind(i64::try_from(claim.claim_revision()).unwrap())
    .bind(claim.claim_process_instance_id().as_str())
    .bind(plan_digest.as_str())
    .bind(actions)
    .fetch_one(pool)
    .await
}

async fn prepare_deferred_receipt(
    store: &PostgresRuntimeInteractionV1,
    content_hash: &str,
    interaction_id: u64,
    plan_digest: InteractionActionPlanDigestV1,
) -> automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1 {
    prepare_deferred_receipt_for_route(
        store,
        content_hash,
        interaction_id,
        &format!("button:deferred-{interaction_id}"),
        plan_digest,
    )
    .await
}

async fn prepare_deferred_receipt_for_route(
    store: &PostgresRuntimeInteractionV1,
    content_hash: &str,
    interaction_id: u64,
    route_key: &str,
    plan_digest: InteractionActionPlanDigestV1,
) -> automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1 {
    let mut claim = acquire_test_receipt(
        store,
        content_hash,
        interaction_id,
        route_key,
        Duration::from_secs(30),
    )
    .await;
    let intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([31; 32]);
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut claim,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::DeferEphemeral,
                intent_digest.clone(),
            ),
        )
        .await
        .unwrap();
    store
        .finish_interaction_receipt_initial_response_v1(
            &mut claim,
            RuntimeInteractionReceiptInitialResponseResultV1::new(
                intent_digest,
                RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
                RuntimeInteractionReceiptOpaqueDigestV1::new([32; 32]),
            ),
        )
        .await
        .unwrap();
    assert_eq!(claim.state(), InteractionReceiptStateV1::Deferred);
    assert!(claim.action_plan_digest().is_none());
    store
        .bind_interaction_receipt_action_plan_v1(&mut claim, plan_digest.clone())
        .await
        .unwrap();
    assert_eq!(claim.state(), InteractionReceiptStateV1::Prepared);
    assert_eq!(claim.action_plan_digest(), Some(&plan_digest));
    claim
}

async fn intend_create_role_effect(
    pool: &PgPool,
    claim: &automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1,
) -> Result<(String, String, i64), sqlx::Error> {
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision \
         FROM public.starring_runtime_interaction_effect_intend_v1(\
             $1, $2, $3, $4, $5, pg_catalog.decode(pg_catalog.repeat('d', 64), 'hex'), \
             0, 1, $6, $7, $8, $9::JSONB, $10, $11::JSONB, 1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(
        claim
            .claim_root()
            .identity()
            .interaction_id()
            .get()
            .to_string(),
    )
    .bind(i64::try_from(claim.head_revision()).unwrap())
    .bind(i64::try_from(claim.claim_revision()).unwrap())
    .bind(claim.claim_process_instance_id().as_str())
    .bind(vec![17_u8; 32])
    .bind(vec![18_u8; 32])
    .bind(Vec::<u8>::new())
    .bind(serde_json::json!({
        "references": [{
            "slot": "guild_id",
            "id": RECEIPT_GUILD_ID.to_string()
        }]
    }))
    .bind(vec![19_u8; 32])
    .bind(serde_json::json!({"kind": "none"}))
    .fetch_one(pool)
    .await
}

async fn finish_create_role_effect(
    pool: &PgPool,
    claim: &automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1,
    outcome: &str,
) -> (String, String, i64) {
    try_finish_create_role_effect(
        pool,
        claim,
        claim.claim_process_instance_id().as_str(),
        outcome,
    )
    .await
    .unwrap()
}

async fn try_finish_create_role_effect(
    pool: &PgPool,
    claim: &automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1,
    process_instance_id: &str,
    outcome: &str,
) -> Result<(String, String, i64), sqlx::Error> {
    let output_id = if outcome == "succeeded" {
        "9400999"
    } else {
        ""
    };
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision \
         FROM public.starring_runtime_interaction_effect_finish_v1(\
             $1, $2, $3, $4, $5, pg_catalog.decode(pg_catalog.repeat('d', 64), 'hex'), \
             0, 2, $6, $7, $8\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(
        claim
            .claim_root()
            .identity()
            .interaction_id()
            .get()
            .to_string(),
    )
    .bind(i64::try_from(claim.head_revision()).unwrap())
    .bind(i64::try_from(claim.claim_revision()).unwrap())
    .bind(process_instance_id)
    .bind(vec![20_u8; 32])
    .bind(outcome)
    .bind(output_id)
    .fetch_one(pool)
    .await
}

async fn intend_edit_response_effect(
    pool: &PgPool,
    claim: &automation_runtime_interaction_postgres::RuntimeInteractionReceiptExclusiveClaimV1,
) -> (String, String, i64) {
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision \
         FROM public.starring_runtime_interaction_effect_intend_v1(\
             $1, $2, $3, $4, $5, pg_catalog.decode(pg_catalog.repeat('d', 64), 'hex'), \
             0, 1, $6, $7, $8, $9::JSONB, $10, $11::JSONB, 1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(
        claim
            .claim_root()
            .identity()
            .interaction_id()
            .get()
            .to_string(),
    )
    .bind(i64::try_from(claim.head_revision()).unwrap())
    .bind(i64::try_from(claim.claim_revision()).unwrap())
    .bind(claim.claim_process_instance_id().as_str())
    .bind(vec![21_u8; 32])
    .bind(vec![22_u8; 32])
    .bind(Vec::<u8>::new())
    .bind(serde_json::json!({
        "references": [],
        "payload_digest": "a".repeat(64)
    }))
    .bind(vec![23_u8; 32])
    .bind(serde_json::json!({"kind": "none"}))
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn make_effect_recovery_due(pool: &PgPool, interaction_id: u64) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_heads_v1 \
         SET next_recovery_at = pg_catalog.clock_timestamp() - INTERVAL '1 second' \
         WHERE application_id = $1 AND interaction_id = $2 \
           AND next_recovery_at IS NOT NULL",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn force_receipt_claim_expired(pool: &PgPool, interaction_id: u64) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_receipt_heads_v1 \
         SET claim_acquired_at = pg_catalog.clock_timestamp() - INTERVAL '2 seconds', \
             claim_expires_at = pg_catalog.clock_timestamp() - INTERVAL '1 second', \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn effect_recovery_scan_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM \
         public.starring_runtime_interaction_effect_scan_recoverable_v1(\
             '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', -1, \
             '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', -1, 256\
         )",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn claim_effect_recovery(
    owner_pool: &PgPool,
    pool: &PgPool,
    interaction_id: u64,
    action_index: i64,
    effect_head_revision: i64,
) -> Result<(String, String, i64, i64), sqlx::Error> {
    let authority: (i64, i64, i64) = sqlx::query_as(
        "SELECT runtime_generation, route_controller_fencing_token, route_incarnation \
         FROM public.runtime_interaction_receipt_roots_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision, \
                resulting_recovery_claim_revision \
         FROM public.starring_runtime_interaction_effect_recovery_claim_v1(\
             $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(action_index)
    .bind(effect_head_revision)
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(authority.0)
    .bind(authority.1)
    .bind(authority.2)
    .fetch_one(pool)
    .await
}

async fn intend_effect_compensation(
    owner_pool: &PgPool,
    pool: &PgPool,
    interaction_id: u64,
    action_index: i64,
    effect_head_revision: i64,
) -> Result<(String, String, i64, i64), sqlx::Error> {
    let authority: (Vec<u8>, i64, i64, i64) = sqlx::query_as(
        "SELECT effect.preflight_certificate_digest, receipt.runtime_generation, \
                receipt.route_controller_fencing_token, receipt.route_incarnation \
         FROM public.runtime_interaction_effect_roots_v1 AS effect \
         INNER JOIN public.runtime_interaction_receipt_roots_v1 AS receipt \
           ON receipt.application_id = effect.application_id \
          AND receipt.interaction_id = effect.interaction_id \
         WHERE effect.application_id = $1 AND effect.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision, \
                resulting_recovery_claim_revision \
         FROM public.starring_runtime_interaction_effect_compensation_intend_v1(\
             $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(action_index)
    .bind(effect_head_revision)
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(authority.1)
    .bind(authority.2)
    .bind(authority.3)
    .bind(authority.0)
    .bind(vec![0xca_u8; 32])
    .fetch_one(pool)
    .await
}

async fn reconcile_observed_effect(
    owner_pool: &PgPool,
    pool: &PgPool,
    interaction_id: u64,
    revisions: (i64, i64),
    observation: (&str, &[u8]),
    runtime_generation_delta: i64,
) -> Result<(String, String, i64), sqlx::Error> {
    let authority: (Vec<u8>, i64, i64, i64) = sqlx::query_as(
        "SELECT effect.preflight_certificate_digest, receipt.runtime_generation, \
                receipt.route_controller_fencing_token, receipt.route_incarnation \
         FROM public.runtime_interaction_effect_roots_v1 AS effect \
         INNER JOIN public.runtime_interaction_receipt_roots_v1 AS receipt \
           ON receipt.application_id = effect.application_id \
          AND receipt.interaction_id = effect.interaction_id \
         WHERE effect.application_id = $1 AND effect.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision \
         FROM public.starring_runtime_interaction_effect_reconcile_v1(\
             $1,$2,0,$3,$4,$5,$6,$7,$8,$9,$10,'observing','observation',\
             $11,$12,$13,'',1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(revisions.0)
    .bind(revisions.1)
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(authority.1 + runtime_generation_delta)
    .bind(authority.2)
    .bind(authority.3)
    .bind(authority.0)
    .bind(observation.0)
    .bind(observation.1)
    .fetch_one(pool)
    .await
}

async fn finish_effect_compensation(
    owner_pool: &PgPool,
    pool: &PgPool,
    interaction_id: u64,
    revisions: (i64, i64),
    process_instance_id: &str,
    result: (&str, &[u8]),
) -> Result<(String, String, i64), sqlx::Error> {
    let preflight_certificate_digest: Vec<u8> = sqlx::query_scalar(
        "SELECT preflight_certificate_digest \
         FROM public.runtime_interaction_effect_roots_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision \
         FROM public.starring_runtime_interaction_effect_compensation_finish_v1(\
             $1,$2,0,$3,$4,$5,$6,$7,$8,1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(revisions.0)
    .bind(revisions.1)
    .bind(process_instance_id)
    .bind(preflight_certificate_digest)
    .bind(result.0)
    .bind(result.1)
    .fetch_one(pool)
    .await
}

async fn claim_response_tail(
    owner_pool: &PgPool,
    pool: &PgPool,
    interaction_id: u64,
    effect_head_revision: i64,
) -> Result<(String, String, i64, i64, i32, String, i64), sqlx::Error> {
    let authority: (i64, i64, i64) = sqlx::query_as(
        "SELECT runtime_generation, route_controller_fencing_token, route_incarnation \
         FROM public.runtime_interaction_receipt_roots_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision, \
                resulting_recovery_claim_revision, resulting_observation_attempt_count, \
                receipt_state, resulting_receipt_head_revision \
         FROM public.starring_runtime_interaction_effect_response_tail_claim_v1(\
             $1,$2,0,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(effect_head_revision)
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(authority.0)
    .bind(authority.1)
    .bind(authority.2)
    .bind(vec![0xdd_u8; 32])
    .bind(vec![0x99_u8; 32])
    .bind(vec![0xee_u8; 32])
    .fetch_one(pool)
    .await
}

async fn finalize_response_tail(
    owner_pool: &PgPool,
    pool: &PgPool,
    interaction_id: u64,
    revisions: (i64, i64, i64),
    process_instance_id: &str,
    outcome: (&str, &[u8], &[u8]),
) -> Result<(String, String, i64, String, i64), sqlx::Error> {
    let authority: (Vec<u8>, i64, i64, i64) = sqlx::query_as(
        "SELECT effect.preflight_certificate_digest, receipt.runtime_generation, \
                receipt.route_controller_fencing_token, receipt.route_incarnation \
         FROM public.runtime_interaction_effect_roots_v1 AS effect \
         INNER JOIN public.runtime_interaction_receipt_roots_v1 AS receipt \
           ON receipt.application_id = effect.application_id \
          AND receipt.interaction_id = effect.interaction_id \
         WHERE effect.application_id = $1 AND effect.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT outcome_name, effect_state, resulting_effect_head_revision, \
                receipt_state, resulting_receipt_head_revision \
         FROM public.starring_runtime_interaction_effect_response_tail_finalize_v1(\
             $1,$2,0,$3,'executing',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,1000\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(interaction_id.to_string())
    .bind(revisions.0)
    .bind(revisions.1)
    .bind(revisions.2)
    .bind(process_instance_id)
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(authority.1)
    .bind(authority.2)
    .bind(authority.3)
    .bind(authority.0)
    .bind(vec![0x99_u8; 32])
    .bind(outcome.0)
    .bind(outcome.1)
    .bind(outcome.2)
    .fetch_one(pool)
    .await
}

async fn complete_actionless_receipt(
    store: &PostgresRuntimeInteractionV1,
    pool: &PgPool,
    content_hash: &str,
    interaction_id: u64,
    route_key: &str,
) {
    let plan_digest =
        InteractionActionPlanDigestV1::parse(format!("{interaction_id:064x}")).unwrap();
    let mut claim = acquire_test_receipt(
        store,
        content_hash,
        interaction_id,
        route_key,
        Duration::from_secs(30),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(&mut claim, plan_digest.clone())
        .await
        .unwrap();
    bind_effect_plan_document(pool, &claim, &plan_digest, serde_json::json!([]))
        .await
        .unwrap();
    let intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([41; 32]);
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut claim,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::RespondEphemeral,
                intent_digest.clone(),
            ),
        )
        .await
        .unwrap();
    store
        .finish_interaction_receipt_initial_response_v1(
            &mut claim,
            RuntimeInteractionReceiptInitialResponseResultV1::new(
                intent_digest,
                RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
                RuntimeInteractionReceiptOpaqueDigestV1::new([42; 32]),
            ),
        )
        .await
        .unwrap();
    store
        .finish_interaction_receipt_v1(
            &mut claim,
            RuntimeInteractionReceiptTerminalOutcomeV1::new(
                RuntimeInteractionReceiptTerminalStateV1::Completed,
                "actionless_completed",
                RuntimeInteractionReceiptOpaqueDigestV1::new([43; 32]),
            )
            .unwrap(),
        )
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
    assert!(terminalize.contains(
        "attestation.controller_fencing_token\n                    > root_row.route_controller_fencing_token"
    ));
    assert!(terminalize.contains("serving.expires_at AS serving_expires_at"));
    assert!(terminalize.contains("owner.expires_at AS gateway_owner_expires_at"));
    assert!(terminalize.contains("execution_row.content_hash"));
    assert!(!terminalize.contains("token_ciphertext"));
    assert!(!terminalize.contains("root_route_key"));
    let claim = migration
        .split("CREATE FUNCTION public.starring_runtime_interaction_receipt_claim_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    assert!(claim.contains("THEN 'response_recovery_terminal'"));
    assert!(claim.contains("THEN 'indeterminate'"));
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn response_tail_scan_fix_is_adjacent_exact_and_metadata_preserving() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let journal = versions
        .iter()
        .position(|version| *version == 202_608_010_001)
        .unwrap();
    let fix = versions
        .iter()
        .position(|version| *version == 202_608_010_002)
        .unwrap();
    assert_eq!(fix, journal + 1);

    let migration = include_str!(
        "../../../migrations/202608010002_fix_runtime_interaction_effect_response_tail_scan_v1.sql"
    );
    for required in [
        "public.starring_runtime_interaction_effect_response_tail_scan_v1(timestamp with time zone,text,text,bigint,timestamp with time zone,text,text,bigint,bigint)",
        "applied_count <> 116",
        "applied_head <> 202608010001",
        "69bc36f83f1e6b575205ca67703639124e41abd9d19b1b701d845ca150dcc4d6202608176b0583c864b4663937bd89e7",
        "c4841de684e511174bb2c3186a7ed94e6a826d127c85e9b73b531c75bf726917",
        "83f98c884ef9bed3706ace2a9430c401760277fb3cc83ea16646d147c819550b",
        "948198e83dfad27778d0d9b1e254a3c1319e405d8fb147f5eea37cbb9077f00e",
        "old_fragment TEXT := 'pg_catalog.greatest('",
        "new_fragment TEXT := 'GREATEST('",
        "/ pg_catalog.char_length(old_fragment) <> 2",
        "/ pg_catalog.char_length(new_fragment) <> 2",
        "metadata_after IS DISTINCT FROM metadata_before",
        "f293f524ef97b491b6781a795888bf879aa0a82f790be699f31e4cee8c054152",
        "8db2428cc05e5f973639d6f8af244722c40ae9183f3b32202ec45af5e22d5215",
        "2e4143ab4b3feb364f1dd83d9c085ff7e719f85cd7e2b4f4073840393fcdfdd2",
        "4cb3618c886f231ab75cd9224422131aadba925e9abac870416244a596be2e17",
        "public.starring_runtime_interaction_effect_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
    ] {
        assert!(migration.contains(required), "missing contract: {required}");
    }
    for forbidden in [
        "\nCREATE ROLE ",
        "\nCREATE TABLE ",
        "\nALTER TABLE ",
        "\nINSERT ",
        "\nUPDATE ",
        "\nDELETE ",
        "\nTRUNCATE ",
        "\nGRANT ",
        "COMMENT ON",
    ] {
        assert!(!migration.contains(forbidden), "forbidden SQL: {forbidden}");
    }
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn ack_first_effect_plan_bind_fix_is_head_exact_and_metadata_preserving() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let status_projection = versions
        .iter()
        .position(|version| *version == 202_608_020_001)
        .unwrap();
    let fix = versions
        .iter()
        .position(|version| *version == 202_608_020_002)
        .unwrap();
    assert_eq!(fix, status_projection + 1);
    assert_eq!(versions.get(fix + 1), Some(&202_608_030_001));
    assert_eq!(versions.get(fix + 2), Some(&202_608_030_002));
    assert_eq!(fix + 2, versions.len() - 1);

    let migration = include_str!(
        "../../../migrations/202608020002_fix_runtime_interaction_effect_ack_first_plan_bind_v1.sql"
    );
    for required in [
        "public.starring_runtime_interaction_effect_plan_bind_v1(text,text,bigint,bigint,text,bytea,bytea,bytea,jsonb)",
        "applied_count <> 118",
        "applied_head <> 202608020001",
        "f00b245ee986adaa9358b30f56756d11642a2c6610c0a03778d09efc630fd379225cfd08c8ec85121a6a38f854955c0f",
        "986be456dee9d29fc2be05cc67291c733195dda219cfd9a68581bcd013893951",
        "4dcdf8c5abdd4a11dd91c60a0722a84f3dba0321f94ce718e767a5992d6e334e",
        "74569a6e5d8d7b6e53ef502b2ff95805927c41473b3e64b2edab9b2d621377eb",
        "receipt_head.state <> ''deferred''",
        "receipt_head.state NOT IN (''prepared'', ''deferred'')",
        "/ pg_catalog.char_length(old_fragment) <> 1",
        "/ pg_catalog.char_length(new_fragment) <> 1",
        "metadata_after IS DISTINCT FROM metadata_before",
        "8db2428cc05e5f973639d6f8af244722c40ae9183f3b32202ec45af5e22d5215",
        "2c26f1d73f15e926dc4dd2af76f698462082490ae298b0b7ce3b366a341378f1",
        "4cb3618c886f231ab75cd9224422131aadba925e9abac870416244a596be2e17",
        "5eb6e46d8d2bfe6f4654d222bb9abe2a8193c31725b36f4485b96fa5b2cd8834",
        "public.starring_runtime_interaction_effect_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
    ] {
        assert!(migration.contains(required), "missing contract: {required}");
    }
    for forbidden in [
        "\nCREATE ROLE ",
        "\nCREATE TABLE ",
        "\nALTER TABLE ",
        "\nINSERT ",
        "\nUPDATE ",
        "\nDELETE ",
        "\nTRUNCATE ",
        "\nGRANT ",
        "COMMENT ON",
    ] {
        assert!(!migration.contains(forbidden), "forbidden SQL: {forbidden}");
    }
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
async fn effect_journal_upgrades_from_durable_receipts_with_exact_catalog_and_acl() {
    let database = isolated_database_with_upgrade_boundary(Some(202_607_310_022)).await;
    let applied: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public._sqlx_migrations \
         WHERE version = 202608010001 AND success",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(applied, 1);
    let manifests: (bool, bool, bool) = sqlx::query_as(
        "SELECT public.starring_runtime_interaction_effect_schema_manifest_v1(), \
                public.starring_runtime_interaction_receipt_schema_manifest_v1(), \
                public.starring_runtime_interaction_schema_manifest_v1()",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(manifests, (true, true, true));
    let catalog: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
             (SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS relation \
              INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
              WHERE namespace.nspname = 'public' AND relation.relkind = 'r' \
                AND relation.relname LIKE 'runtime_interaction_effect_%'), \
             (SELECT pg_catalog.count(*) FROM pg_catalog.pg_indexes \
              WHERE schemaname = 'public' AND indexname LIKE 'runtime_interaction_effect_%'), \
             (SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc AS function_row \
              INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = function_row.pronamespace \
              WHERE namespace.nspname = 'public' AND (function_row.proname LIKE 'starring_runtime_interaction_effect_%' \
                OR function_row.proname LIKE 'guard_runtime_interaction_effect_%')), \
             (SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS relation \
              INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
              WHERE namespace.nspname = 'public')",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(catalog, (4, 8, 22, 198));
    let executable: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname = 'public' \
           AND function_row.proname LIKE 'starring_runtime_interaction_effect_%' \
           AND pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE')",
    )
    .bind(&database.role)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(executable, 11);
    let direct_write = sqlx::query(
        "INSERT INTO public.runtime_interaction_effect_roots_v1 (application_id) VALUES ('1')",
    )
    .execute(&database.executor_pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&direct_write).as_deref(), Some("42501"));
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn response_tail_scan_fix_upgrades_from_effect_journal_and_serves_empty_scan() {
    let database = isolated_database_with_upgrade_boundary(Some(202_608_010_001)).await;
    assert_response_tail_scan_fix_checksum(&database.owner_pool).await;
    let ledger: (i64, i64, i32) = sqlx::query_as(
        "SELECT pg_catalog.count(*), pg_catalog.max(version), \
                pg_catalog.octet_length((\
                    SELECT migration.checksum \
                    FROM public._sqlx_migrations AS migration \
                    WHERE migration.version = 202608010002 \
                        AND migration.success\
                )) \
         FROM public._sqlx_migrations WHERE success",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(ledger, (121, 202_608_030_002, 48));
    let definitions: (String, String, i32, i32) = sqlx::query_as(
        "SELECT \
             pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
                 pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1)), 'UTF8'\
             )), 'hex'), \
             pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
                 pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($2)), 'UTF8'\
             )), 'hex'), \
             (pg_catalog.char_length(pg_catalog.pg_get_functiondef(\
                  pg_catalog.to_regprocedure($1)\
              )) - pg_catalog.char_length(pg_catalog.replace(\
                  pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1)), \
                  'pg_catalog.greatest(', ''\
              ))) / pg_catalog.char_length('pg_catalog.greatest('), \
             (pg_catalog.char_length(pg_catalog.pg_get_functiondef(\
                  pg_catalog.to_regprocedure($1)\
              )) - pg_catalog.char_length(pg_catalog.replace(\
                  pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1)), \
                  'GREATEST(', ''\
              ))) / pg_catalog.char_length('GREATEST(')",
    )
    .bind(EFFECT_RESPONSE_TAIL_SCAN_FUNCTION)
    .bind("public.starring_runtime_interaction_effect_schema_manifest_v1()")
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        definitions,
        (
            "83f98c884ef9bed3706ace2a9430c401760277fb3cc83ea16646d147c819550b".to_string(),
            "5eb6e46d8d2bfe6f4654d222bb9abe2a8193c31725b36f4485b96fa5b2cd8834".to_string(),
            0,
            2,
        )
    );
    let manifests: (bool, bool, bool) = sqlx::query_as(
        "SELECT public.starring_runtime_interaction_effect_schema_manifest_v1(), \
                public.starring_runtime_interaction_receipt_schema_manifest_v1(), \
                public.starring_runtime_interaction_schema_manifest_v1()",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(manifests, (true, true, true));

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
    let page = store
        .scan_recoverable_interaction_response_tails_v1(
            &RuntimeInteractionEffectResponseTailScanCursorV1::default(),
            NonZeroUsize::new(1).unwrap(),
        )
        .await
        .unwrap();
    assert!(page.candidates().is_empty());
    assert!(page.exhausted());
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
async fn durable_defer_can_precede_plan_binding_and_terminalize_preparation_failure() {
    let database = isolated_database().await;
    let content_hash = seed_receipt_authority(&database.owner_pool).await;
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

    let mut prepared = acquire_test_receipt(
        &store,
        &content_hash,
        9_300_005,
        "button:defer-before-plan",
        Duration::from_secs(30),
    )
    .await;
    let prepared_intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([51; 32]);
    assert_eq!(
        store
            .intend_interaction_receipt_initial_response_v1(
                &mut prepared,
                RuntimeInteractionReceiptInitialResponseIntentV1::new(
                    RuntimeInteractionReceiptInitialResponseKindV1::DeferEphemeral,
                    prepared_intent_digest.clone(),
                ),
            )
            .await
            .unwrap(),
        RuntimeInteractionReceiptInitialResponseIntentDispositionV1::ExternalCallAuthorized
    );
    assert_eq!(prepared.state(), InteractionReceiptStateV1::Acknowledging);
    assert!(prepared.action_plan_digest().is_none());
    assert_eq!(
        store
            .finish_interaction_receipt_initial_response_v1(
                &mut prepared,
                RuntimeInteractionReceiptInitialResponseResultV1::new(
                    prepared_intent_digest,
                    RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
                    RuntimeInteractionReceiptOpaqueDigestV1::new([52; 32]),
                ),
            )
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(prepared.state(), InteractionReceiptStateV1::Deferred);
    assert!(prepared.action_plan_digest().is_none());
    let plan_digest = InteractionActionPlanDigestV1::parse("5".repeat(64)).unwrap();
    assert_eq!(
        store
            .bind_interaction_receipt_action_plan_v1(&mut prepared, plan_digest.clone())
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(prepared.state(), InteractionReceiptStateV1::Prepared);
    assert_eq!(prepared.action_plan_digest(), Some(&plan_digest));

    let failed_interaction_id = 9_300_006;
    let mut failed = acquire_test_receipt(
        &store,
        &content_hash,
        failed_interaction_id,
        "button:defer-before-failure",
        Duration::from_secs(30),
    )
    .await;
    let failed_intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([53; 32]);
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut failed,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::DeferEphemeral,
                failed_intent_digest.clone(),
            ),
        )
        .await
        .unwrap();
    store
        .finish_interaction_receipt_initial_response_v1(
            &mut failed,
            RuntimeInteractionReceiptInitialResponseResultV1::new(
                failed_intent_digest,
                RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
                RuntimeInteractionReceiptOpaqueDigestV1::new([54; 32]),
            ),
        )
        .await
        .unwrap();
    assert_eq!(failed.state(), InteractionReceiptStateV1::Deferred);
    assert!(failed.action_plan_digest().is_none());
    assert_eq!(
        store
            .finish_interaction_receipt_v1(
                &mut failed,
                RuntimeInteractionReceiptTerminalOutcomeV1::new(
                    RuntimeInteractionReceiptTerminalStateV1::Failed,
                    "preparation_failed_after_defer",
                    RuntimeInteractionReceiptOpaqueDigestV1::new([55; 32]),
                )
                .unwrap(),
            )
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(failed.state(), InteractionReceiptStateV1::Failed);
    assert!(failed.action_plan_digest().is_none());
    let failed_secret_count: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_interaction_receipt_token_secrets_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(failed_interaction_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(failed_secret_count, 0);
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn effect_plan_bind_enforces_defer_tail_policy_and_serializes_replay() {
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

    let direct_plan = InteractionActionPlanDigestV1::parse("1".repeat(64)).unwrap();
    let mut direct = acquire_test_receipt(
        &store,
        &content_hash,
        9_300_001,
        "button:direct-zero",
        Duration::from_secs(30),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(&mut direct, direct_plan.clone())
        .await
        .unwrap();
    assert_eq!(
        bind_effect_plan_document(
            &database.executor_pool,
            &direct,
            &direct_plan,
            serde_json::json!([]),
        )
        .await
        .unwrap(),
        ("plan_bound".to_string(), 0)
    );
    let direct_intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([33; 32]);
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut direct,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::RespondEphemeral,
                direct_intent_digest.clone(),
            ),
        )
        .await
        .unwrap();
    store
        .finish_interaction_receipt_initial_response_v1(
            &mut direct,
            RuntimeInteractionReceiptInitialResponseResultV1::new(
                direct_intent_digest,
                RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
                RuntimeInteractionReceiptOpaqueDigestV1::new([34; 32]),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .finish_interaction_receipt_v1(
                &mut direct,
                RuntimeInteractionReceiptTerminalOutcomeV1::new(
                    RuntimeInteractionReceiptTerminalStateV1::Completed,
                    "direct_zero_completed",
                    RuntimeInteractionReceiptOpaqueDigestV1::new([35; 32]),
                )
                .unwrap(),
            )
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );

    let unacknowledged_plan = InteractionActionPlanDigestV1::parse("2".repeat(64)).unwrap();
    let mut unacknowledged = acquire_test_receipt(
        &store,
        &content_hash,
        9_300_002,
        "button:unacknowledged-mutation",
        Duration::from_secs(30),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(&mut unacknowledged, unacknowledged_plan.clone())
        .await
        .unwrap();
    let unacknowledged_error = bind_effect_plan_document(
        &database.executor_pool,
        &unacknowledged,
        &unacknowledged_plan,
        serde_json::json!([create_role_effect_action(0), edit_response_effect_action(1)]),
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&unacknowledged_error).as_deref(), Some("RI001"));
    let unacknowledged_rows: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_interaction_effect_roots_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9300002")
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(unacknowledged_rows, 0);

    let deferred_plan = InteractionActionPlanDigestV1::parse("3".repeat(64)).unwrap();
    let mut deferred =
        prepare_deferred_receipt(&store, &content_hash, 9_300_003, deferred_plan.clone()).await;
    let mut null_dependency = create_role_effect_action(0);
    null_dependency["dependency_indices"] = serde_json::json!([null]);
    let mut predecessor_with_null = create_role_effect_action(1);
    predecessor_with_null["dependency_indices"] = serde_json::json!([0, null]);
    predecessor_with_null["correlation_marker"] = serde_json::json!("7".repeat(64));
    for invalid in [
        serde_json::json!([null_dependency]),
        serde_json::json!([create_role_effect_action(0), predecessor_with_null]),
        serde_json::json!([create_role_effect_action(0)]),
        serde_json::json!([
            edit_response_effect_action(0),
            edit_response_effect_action(1)
        ]),
        serde_json::json!([edit_response_effect_action(0), create_role_effect_action(1)]),
    ] {
        let error =
            bind_effect_plan_document(&database.executor_pool, &deferred, &deferred_plan, invalid)
                .await
                .unwrap_err();
        assert_eq!(sqlstate(&error).as_deref(), Some("RI003"));
    }
    let valid = serde_json::json!([create_role_effect_action(0), edit_response_effect_action(1)]);
    let (left, right) = tokio::join!(
        bind_effect_plan_document(
            &database.executor_pool,
            &deferred,
            &deferred_plan,
            valid.clone(),
        ),
        bind_effect_plan_document(&database.executor_pool, &deferred, &deferred_plan, valid,)
    );
    let mut outcomes = [left.unwrap().0, right.unwrap().0];
    outcomes.sort();
    assert_eq!(outcomes, ["exact_replay", "plan_bound"]);
    assert_eq!(
        store
            .intend_interaction_receipt_execution_v1(&mut deferred)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );

    let legacy_plan = InteractionActionPlanDigestV1::parse("6".repeat(64)).unwrap();
    let mut legacy = acquire_test_receipt(
        &store,
        &content_hash,
        9_300_007,
        "button:legacy-plan-before-defer",
        Duration::from_secs(30),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(&mut legacy, legacy_plan.clone())
        .await
        .unwrap();
    let legacy_intent_digest = RuntimeInteractionReceiptOpaqueDigestV1::new([36; 32]);
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut legacy,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::DeferEphemeral,
                legacy_intent_digest.clone(),
            ),
        )
        .await
        .unwrap();
    store
        .finish_interaction_receipt_initial_response_v1(
            &mut legacy,
            RuntimeInteractionReceiptInitialResponseResultV1::new(
                legacy_intent_digest,
                RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded,
                RuntimeInteractionReceiptOpaqueDigestV1::new([37; 32]),
            ),
        )
        .await
        .unwrap();
    assert_eq!(legacy.state(), InteractionReceiptStateV1::Deferred);
    let mut legacy_create_role = create_role_effect_action(0);
    legacy_create_role["correlation_marker"] = serde_json::json!("d".repeat(64));
    let legacy_actions = serde_json::json!([legacy_create_role, edit_response_effect_action(1)]);
    assert_eq!(
        bind_effect_plan_document(
            &database.executor_pool,
            &legacy,
            &legacy_plan,
            legacy_actions.clone(),
        )
        .await
        .unwrap(),
        ("plan_bound".to_string(), 2)
    );
    assert_eq!(
        bind_effect_plan_document(
            &database.executor_pool,
            &legacy,
            &legacy_plan,
            legacy_actions,
        )
        .await
        .unwrap(),
        ("exact_replay".to_string(), 2)
    );
    assert_eq!(
        store
            .intend_interaction_receipt_execution_v1(&mut legacy)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );

    let response_only_plan = InteractionActionPlanDigestV1::parse("4".repeat(64)).unwrap();
    let response_only =
        prepare_deferred_receipt(&store, &content_hash, 9_300_004, response_only_plan.clone())
            .await;
    assert_eq!(
        bind_effect_plan_document(
            &database.executor_pool,
            &response_only,
            &response_only_plan,
            serde_json::json!([edit_response_effect_action(0)]),
        )
        .await
        .unwrap(),
        ("plan_bound".to_string(), 1)
    );
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn effect_same_route_admission_is_serialized_and_completed_history_is_index_bounded() {
    let database = isolated_database().await;
    let content_hash = seed_receipt_authority(&database.owner_pool).await;
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
    let route_key = "button:shared-static";
    complete_actionless_receipt(
        &store,
        &database.executor_pool,
        &content_hash,
        9_400_001,
        route_key,
    )
    .await;

    let terminal_plan = InteractionActionPlanDigestV1::parse("6".repeat(64)).unwrap();
    let mut terminal_source = prepare_deferred_receipt_for_route(
        &store,
        &content_hash,
        9_400_100,
        "button:terminal-source",
        terminal_plan.clone(),
    )
    .await;
    let mut terminal_action = create_role_effect_action(0);
    terminal_action["correlation_marker"] = serde_json::json!("6".repeat(64));
    bind_effect_plan_document(
        &database.executor_pool,
        &terminal_source,
        &terminal_plan,
        serde_json::json!([terminal_action, edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut terminal_source)
        .await
        .unwrap();
    assert_eq!(
        intend_create_role_effect(&database.executor_pool, &terminal_source)
            .await
            .unwrap(),
        ("intended".to_string(), "intended".to_string(), 2)
    );
    assert_eq!(
        finish_create_role_effect(
            &database.executor_pool,
            &terminal_source,
            "definitive_failure",
        )
        .await,
        (
            "definitive_failure".to_string(),
            "known_failed".to_string(),
            3
        )
    );
    assert_eq!(
        finish_create_role_effect(
            &database.executor_pool,
            &terminal_source,
            "definitive_failure",
        )
        .await,
        ("exact_replay".to_string(), "known_failed".to_string(), 3)
    );
    let finish_replay_before: (String, i64, i64) = sqlx::query_as(
        "SELECT state, head_revision, \
                (SELECT pg_catalog.count(*) \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400100")
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let stale_finish_process = try_finish_create_role_effect(
        &database.executor_pool,
        &terminal_source,
        "process-receipt-stale",
        "definitive_failure",
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&stale_finish_process).as_deref(), Some("RI001"));
    let finish_replay_after: (String, i64, i64) = sqlx::query_as(
        "SELECT state, head_revision, \
                (SELECT pg_catalog.count(*) \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400100")
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(finish_replay_after, finish_replay_before);

    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_receipt_roots_v1 \
         SELECT (pg_catalog.jsonb_populate_record(\
             NULL::public.runtime_interaction_receipt_roots_v1, \
             pg_catalog.to_jsonb(source) || pg_catalog.jsonb_build_object(\
                 'interaction_id', (950000000 + ordinal)::TEXT\
             )\
         )).* \
         FROM public.runtime_interaction_receipt_roots_v1 AS source \
         CROSS JOIN pg_catalog.generate_series(1, 5000) AS ordinal \
         WHERE source.application_id = $1 AND source.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400001")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_receipt_heads_v1 \
         SELECT (pg_catalog.jsonb_populate_record(\
             NULL::public.runtime_interaction_receipt_heads_v1, \
             pg_catalog.to_jsonb(source) || pg_catalog.jsonb_build_object(\
                 'interaction_id', (950000000 + ordinal)::TEXT\
             )\
         )).* \
         FROM public.runtime_interaction_receipt_heads_v1 AS source \
         CROSS JOIN pg_catalog.generate_series(1, 5000) AS ordinal \
         WHERE source.application_id = $1 AND source.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400001")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_effect_roots_v1 \
         SELECT (pg_catalog.jsonb_populate_record(\
             NULL::public.runtime_interaction_effect_roots_v1, \
             pg_catalog.to_jsonb(source) || pg_catalog.jsonb_build_object(\
                 'interaction_id', (950000000 + ordinal)::TEXT\
             )\
         )).* \
         FROM public.runtime_interaction_effect_roots_v1 AS source \
         CROSS JOIN pg_catalog.generate_series(1, 5000) AS ordinal \
         WHERE source.application_id = $1 AND source.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400100")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_effect_heads_v1 \
         SELECT (pg_catalog.jsonb_populate_record(\
             NULL::public.runtime_interaction_effect_heads_v1, \
             pg_catalog.to_jsonb(source) || pg_catalog.jsonb_build_object(\
                 'interaction_id', (950000000 + ordinal)::TEXT, \
                 'correlation_marker', CASE \
                     WHEN source.correlation_marker IS NULL THEN NULL \
                     ELSE pg_catalog.lpad(pg_catalog.to_hex(ordinal), 64, '0') \
                 END\
             )\
         )).* \
         FROM public.runtime_interaction_effect_heads_v1 AS source \
         CROSS JOIN pg_catalog.generate_series(1, 5000) AS ordinal \
         WHERE source.application_id = $1 AND source.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400100")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_effect_rollbacks_v1 \
         SELECT (pg_catalog.jsonb_populate_record(\
             NULL::public.runtime_interaction_effect_rollbacks_v1, \
             pg_catalog.to_jsonb(source) || pg_catalog.jsonb_build_object(\
                 'interaction_id', (950000000 + ordinal)::TEXT, \
                 'state', 'completed', 'revision', 2, \
                 'completed_at', source.required_at\
             )\
         )).* \
         FROM public.runtime_interaction_effect_rollbacks_v1 AS source \
         CROSS JOIN pg_catalog.generate_series(1, 5000) AS ordinal \
         WHERE source.application_id = $1 AND source.interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind("9400100")
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let mut left = prepare_deferred_receipt_for_route(
        &store,
        &content_hash,
        9_400_002,
        route_key,
        InteractionActionPlanDigestV1::parse("4".repeat(64)).unwrap(),
    )
    .await;
    let mut right = prepare_deferred_receipt_for_route(
        &store,
        &content_hash,
        9_400_003,
        route_key,
        InteractionActionPlanDigestV1::parse("5".repeat(64)).unwrap(),
    )
    .await;
    let mut left_action = create_role_effect_action(0);
    left_action["correlation_marker"] = serde_json::json!("4".repeat(64));
    let mut right_action = create_role_effect_action(0);
    right_action["correlation_marker"] = serde_json::json!("5".repeat(64));
    bind_effect_plan_document(
        &database.executor_pool,
        &left,
        &InteractionActionPlanDigestV1::parse("4".repeat(64)).unwrap(),
        serde_json::json!([left_action, edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    bind_effect_plan_document(
        &database.executor_pool,
        &right,
        &InteractionActionPlanDigestV1::parse("5".repeat(64)).unwrap(),
        serde_json::json!([right_action, edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut left)
        .await
        .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut right)
        .await
        .unwrap();

    let (left_outcome, right_outcome) = tokio::join!(
        intend_create_role_effect(&database.executor_pool, &left),
        intend_create_role_effect(&database.executor_pool, &right)
    );
    assert!(
        matches!(
            (&left_outcome, &right_outcome),
            (Ok((outcome, state, 2)), Err(error))
                if outcome == "intended" && state == "intended"
                    && sqlstate(error).as_deref() == Some("RI004")
        ) || matches!(
            (&left_outcome, &right_outcome),
            (Err(error), Ok((outcome, state, 2)))
                if outcome == "intended" && state == "intended"
                    && sqlstate(error).as_deref() == Some("RI004")
        ),
        "left={left_outcome:?}, right={right_outcome:?}"
    );
    let winner = if left_outcome.is_ok() { &left } else { &right };
    assert_eq!(
        intend_create_role_effect(&database.executor_pool, winner)
            .await
            .unwrap(),
        ("exact_replay".to_string(), "intended".to_string(), 2)
    );

    let fresh_request = test_receipt_request(
        &store,
        &content_hash,
        9_400_004,
        route_key,
        Duration::from_secs(30),
    )
    .await;
    assert!(matches!(
        store.claim_interaction_receipt_v1(fresh_request).await,
        Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
    ));

    sqlx::query(
        "ANALYZE public.runtime_interaction_receipt_roots_v1, \
         public.runtime_interaction_effect_heads_v1, \
         public.runtime_interaction_effect_rollbacks_v1",
    )
    .execute(&database.owner_pool)
    .await
    .unwrap();
    let plan: serde_json::Value = sqlx::query_scalar(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) \
         SELECT 1 FROM (\
             SELECT effect.application_id, effect.interaction_id \
             FROM public.runtime_interaction_effect_heads_v1 AS effect \
             INNER JOIN public.runtime_interaction_receipt_roots_v1 AS root \
                 ON root.application_id = effect.application_id \
                 AND root.interaction_id = effect.interaction_id \
             WHERE effect.application_id = $1 \
                 AND effect.action_kind <> 'edit_response' \
                 AND effect.state IN (\
                     'intended','indeterminate','observing','observation_pending',\
                     'compensation_intended','compensation_indeterminate',\
                     'compensation_observing','compensation_observation_pending',\
                     'recovery_required'\
                 ) \
                 AND root.guild_id = $2 AND root.ruleset_key = $3 \
                 AND root.route_kind = 'static' AND root.route_key = $4 \
             UNION ALL \
             SELECT effect.application_id, effect.interaction_id \
             FROM public.runtime_interaction_effect_rollbacks_v1 AS rollback \
             INNER JOIN public.runtime_interaction_effect_heads_v1 AS effect \
                 ON effect.application_id = rollback.application_id \
                 AND effect.interaction_id = rollback.interaction_id \
                 AND effect.action_index <= rollback.abort_action_index \
             INNER JOIN public.runtime_interaction_receipt_roots_v1 AS root \
                 ON root.application_id = effect.application_id \
                 AND root.interaction_id = effect.interaction_id \
             WHERE rollback.state = 'required' AND effect.application_id = $1 \
                 AND effect.action_kind <> 'edit_response' \
                 AND effect.state IN ('known_succeeded','reconciled_succeeded') \
                 AND root.guild_id = $2 AND root.ruleset_key = $3 \
                 AND root.route_kind = 'static' AND root.route_key = $4\
         ) AS blocked LIMIT 1",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(RECEIPT_GUILD_ID.to_string())
    .bind(RECEIPT_RULESET_KEY)
    .bind(route_key)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let plan_text = plan.to_string();
    assert!(plan_text.contains("runtime_interaction_effect_heads_route_unsafe_v1_idx"));
    assert!(plan_text.contains("runtime_interaction_effect_rollbacks_required_v1_idx"));
    assert!(!explain_has_seq_scan(
        &plan,
        "runtime_interaction_effect_heads_v1"
    ));
    assert!(!explain_has_seq_scan(
        &plan,
        "runtime_interaction_effect_rollbacks_v1"
    ));
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn response_tail_scan_returns_an_eligible_expired_edit_response() {
    let database = isolated_database().await;
    assert_response_tail_scan_fix_checksum(&database.owner_pool).await;
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
    let interaction_id = 9_600_099;
    let action_plan = InteractionActionPlanDigestV1::parse("9".repeat(64)).unwrap();
    let mut receipt =
        prepare_deferred_receipt(&store, &content_hash, interaction_id, action_plan.clone()).await;
    bind_effect_plan_document(
        &database.executor_pool,
        &receipt,
        &action_plan,
        serde_json::json!([edit_response_effect_action(0)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut receipt)
        .await
        .unwrap();
    assert_eq!(
        intend_edit_response_effect(&database.executor_pool, &receipt).await,
        ("intended".to_string(), "intended".to_string(), 2)
    );
    make_effect_recovery_due(&database.owner_pool, interaction_id).await;
    force_receipt_claim_expired(&database.owner_pool, interaction_id).await;

    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT interaction_id, effect_state, effect_head_revision \
         FROM public.starring_runtime_interaction_effect_response_tail_scan_v1(\
             '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', -1, \
             '1970-01-01 00:00:00+00'::TIMESTAMPTZ, '', '', -1, 256\
         )",
    )
    .fetch_all(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(interaction_id.to_string(), "intended".to_string(), 2)]
    );
    cleanup(database).await;
}

#[tokio::test]
#[ignore]
async fn effect_recovery_fences_active_receipts_and_persists_budget_and_response_replays() {
    let database = isolated_database().await;
    let content_hash = seed_receipt_authority(&database.owner_pool).await;
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

    let observation_id = 9_600_001;
    let observation_plan = InteractionActionPlanDigestV1::parse("f".repeat(64)).unwrap();
    let mut observation = prepare_deferred_receipt(
        &store,
        &content_hash,
        observation_id,
        observation_plan.clone(),
    )
    .await;
    bind_effect_plan_document(
        &database.executor_pool,
        &observation,
        &observation_plan,
        serde_json::json!([create_role_effect_action(0), edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut observation)
        .await
        .unwrap();
    assert_eq!(
        intend_create_role_effect(&database.executor_pool, &observation)
            .await
            .unwrap(),
        ("intended".to_string(), "intended".to_string(), 2)
    );
    make_effect_recovery_due(&database.owner_pool, observation_id).await;
    assert_eq!(effect_recovery_scan_count(&database.executor_pool).await, 0);
    let before: (String, i64, i64) = sqlx::query_as(
        "SELECT state, head_revision, \
                (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let active_error = claim_effect_recovery(
        &database.owner_pool,
        &database.executor_pool,
        observation_id,
        0,
        2,
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&active_error).as_deref(), Some("RI004"));
    let active_compensation_error = intend_effect_compensation(
        &database.owner_pool,
        &database.executor_pool,
        observation_id,
        0,
        2,
    )
    .await
    .unwrap_err();
    assert_eq!(
        sqlstate(&active_compensation_error).as_deref(),
        Some("RI004")
    );
    let after_active: (String, i64, i64) = sqlx::query_as(
        "SELECT state, head_revision, \
                (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(after_active, before);

    force_receipt_claim_expired(&database.owner_pool, observation_id).await;
    assert_eq!(effect_recovery_scan_count(&database.executor_pool).await, 1);
    assert_eq!(
        claim_effect_recovery(
            &database.owner_pool,
            &database.executor_pool,
            observation_id,
            0,
            2,
        )
        .await
        .unwrap(),
        (
            "recovery_claimed".to_string(),
            "observing".to_string(),
            3,
            1
        )
    );
    let preserved_result = vec![0x70_u8; 32];
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_heads_v1 \
         SET state = 'observation_pending', head_revision = 4, \
             observation_attempt_count = 64, result_digest = $3, \
             result_at = pg_catalog.clock_timestamp(), \
             recovery_process_instance_id = NULL, recovery_gateway_shard_id = NULL, \
             recovery_runtime_build_revision = NULL, recovery_acquired_at = NULL, \
             recovery_expires_at = NULL, \
             next_recovery_at = pg_catalog.clock_timestamp() - INTERVAL '1 second', \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .bind(&preserved_result)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        claim_effect_recovery(
            &database.owner_pool,
            &database.executor_pool,
            observation_id,
            0,
            4,
        )
        .await
        .unwrap(),
        (
            "recovery_blocked_attempt_budget_exhausted".to_string(),
            "recovery_required".to_string(),
            5,
            1
        )
    );
    let blocked_event_count: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        claim_effect_recovery(
            &database.owner_pool,
            &database.executor_pool,
            observation_id,
            0,
            4,
        )
        .await
        .unwrap(),
        (
            "exact_replay".to_string(),
            "recovery_required".to_string(),
            5,
            1
        )
    );
    let blocked_head: (Vec<u8>, i64) = sqlx::query_as(
        "SELECT result_digest, \
                (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(blocked_head, (preserved_result, blocked_event_count));
    assert_eq!(effect_recovery_scan_count(&database.executor_pool).await, 0);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_events_v1 \
         SET from_state = 'compensation_observation_pending' \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0 \
           AND event_revision = 5",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let tampered_before: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let tampered_replay = claim_effect_recovery(
        &database.owner_pool,
        &database.executor_pool,
        observation_id,
        0,
        4,
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&tampered_replay).as_deref(), Some("RI001"));
    let tampered_after: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(observation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(tampered_after, tampered_before);

    let compensation_id = 9_600_004;
    let compensation_plan = InteractionActionPlanDigestV1::parse("c".repeat(64)).unwrap();
    let mut compensation = prepare_deferred_receipt(
        &store,
        &content_hash,
        compensation_id,
        compensation_plan.clone(),
    )
    .await;
    let mut compensation_action = create_role_effect_action(0);
    compensation_action["correlation_marker"] = serde_json::json!("7".repeat(64));
    bind_effect_plan_document(
        &database.executor_pool,
        &compensation,
        &compensation_plan,
        serde_json::json!([compensation_action, edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut compensation)
        .await
        .unwrap();
    assert_eq!(
        intend_create_role_effect(&database.executor_pool, &compensation)
            .await
            .unwrap(),
        ("intended".to_string(), "intended".to_string(), 2)
    );
    force_receipt_claim_expired(&database.owner_pool, compensation_id).await;
    let compensation_observation_result = vec![0x73_u8; 32];
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_heads_v1 \
         SET state = 'compensation_observation_pending', head_revision = 3, \
             compensation_observation_attempt_count = 64, result_digest = $3, \
             result_at = pg_catalog.clock_timestamp(), \
             success_binding_kind = 'attempt_result', success_binding_digest = $3, \
             output_id = '123456789', compensation_intent_digest = $4, \
             compensation_intent_at = pg_catalog.clock_timestamp(), \
             compensation_result_digest = $5, \
             compensation_result_at = pg_catalog.clock_timestamp(), \
             recovery_process_instance_id = NULL, recovery_gateway_shard_id = NULL, \
             recovery_runtime_build_revision = NULL, recovery_acquired_at = NULL, \
             recovery_expires_at = NULL, \
             next_recovery_at = pg_catalog.clock_timestamp() - INTERVAL '1 second', \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_id.to_string())
    .bind(vec![0x74_u8; 32])
    .bind(vec![0x75_u8; 32])
    .bind(&compensation_observation_result)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_effect_rollbacks_v1 (\
             application_id, interaction_id, abort_action_index, abort_reason, \
             state, revision, required_at, completed_at\
         ) VALUES ($1,$2,0,'recovery_required','required',1,\
                   pg_catalog.clock_timestamp(),NULL)",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        claim_effect_recovery(
            &database.owner_pool,
            &database.executor_pool,
            compensation_id,
            0,
            3,
        )
        .await
        .unwrap(),
        (
            "recovery_blocked_attempt_budget_exhausted".to_string(),
            "recovery_required".to_string(),
            4,
            0
        )
    );
    let compensation_events: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        claim_effect_recovery(
            &database.owner_pool,
            &database.executor_pool,
            compensation_id,
            0,
            3,
        )
        .await
        .unwrap(),
        (
            "exact_replay".to_string(),
            "recovery_required".to_string(),
            4,
            0
        )
    );
    let compensation_evidence: (Vec<u8>, i64) = sqlx::query_as(
        "SELECT compensation_result_digest, \
                (SELECT pg_catalog.count(*) \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        compensation_evidence,
        (compensation_observation_result, compensation_events)
    );
    assert_eq!(effect_recovery_scan_count(&database.executor_pool).await, 0);

    let compensation_finish_id = 9_600_006;
    let compensation_finish_plan = InteractionActionPlanDigestV1::parse("a".repeat(64)).unwrap();
    let mut compensation_finish = prepare_deferred_receipt(
        &store,
        &content_hash,
        compensation_finish_id,
        compensation_finish_plan.clone(),
    )
    .await;
    let mut compensation_finish_action = create_role_effect_action(0);
    compensation_finish_action["correlation_marker"] = serde_json::json!("9".repeat(64));
    bind_effect_plan_document(
        &database.executor_pool,
        &compensation_finish,
        &compensation_finish_plan,
        serde_json::json!([compensation_finish_action, edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut compensation_finish)
        .await
        .unwrap();
    assert_eq!(
        intend_create_role_effect(&database.executor_pool, &compensation_finish)
            .await
            .unwrap(),
        ("intended".to_string(), "intended".to_string(), 2)
    );
    force_receipt_claim_expired(&database.owner_pool, compensation_finish_id).await;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_heads_v1 \
         SET state = 'compensation_intended', head_revision = 3, \
             compensation_attempt_count = 1, result_digest = $3, \
             result_at = pg_catalog.statement_timestamp(), \
             success_binding_kind = 'attempt_result', success_binding_digest = $3, \
             output_id = '223456789', recovery_claim_revision = 1, \
             recovery_process_instance_id = $4, \
             recovery_gateway_shard_id = $5, \
             recovery_runtime_build_revision = $6, \
             recovery_acquired_at = pg_catalog.statement_timestamp(), \
             recovery_expires_at = pg_catalog.statement_timestamp() + INTERVAL '1 minute', \
             next_recovery_at = pg_catalog.statement_timestamp() + INTERVAL '1 minute', \
             compensation_intent_digest = $7, \
             compensation_intent_at = pg_catalog.statement_timestamp(), \
             updated_at = pg_catalog.statement_timestamp() \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .bind(vec![0x77_u8; 32])
    .bind(RECEIPT_PROCESS_ID)
    .bind(RECEIPT_GATEWAY_SHARD)
    .bind(RECEIPT_BUILD_REVISION)
    .bind(vec![0x78_u8; 32])
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_interaction_effect_rollbacks_v1 (\
             application_id, interaction_id, abort_action_index, abort_reason, \
             state, revision, required_at, completed_at\
         ) VALUES ($1,$2,0,'recovery_required','required',1,\
                   pg_catalog.clock_timestamp(),NULL)",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let compensation_finish_digest = vec![0x79_u8; 32];
    assert_eq!(
        finish_effect_compensation(
            &database.owner_pool,
            &database.executor_pool,
            compensation_finish_id,
            (3, 1),
            RECEIPT_PROCESS_ID,
            ("compensated", &compensation_finish_digest),
        )
        .await
        .unwrap(),
        ("compensated".to_string(), "compensated".to_string(), 4)
    );
    assert_eq!(
        finish_effect_compensation(
            &database.owner_pool,
            &database.executor_pool,
            compensation_finish_id,
            (3, 1),
            RECEIPT_PROCESS_ID,
            ("compensated", &compensation_finish_digest),
        )
        .await
        .unwrap(),
        ("exact_replay".to_string(), "compensated".to_string(), 4)
    );
    let compensation_finish_before: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let stale_compensation_process = finish_effect_compensation(
        &database.owner_pool,
        &database.executor_pool,
        compensation_finish_id,
        (3, 1),
        "process-receipt-stale",
        ("compensated", &compensation_finish_digest),
    )
    .await
    .unwrap_err();
    assert_eq!(
        sqlstate(&stale_compensation_process).as_deref(),
        Some("RI001")
    );
    let compensation_finish_after: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(compensation_finish_after, compensation_finish_before);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_events_v1 \
         SET outcome_code = 'indeterminate' \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0 \
           AND event_revision = 4",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let tampered_compensation_before: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let tampered_compensation = finish_effect_compensation(
        &database.owner_pool,
        &database.executor_pool,
        compensation_finish_id,
        (3, 1),
        RECEIPT_PROCESS_ID,
        ("compensated", &compensation_finish_digest),
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&tampered_compensation).as_deref(), Some("RI001"));
    let tampered_compensation_after: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(compensation_finish_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(tampered_compensation_after, tampered_compensation_before);

    let replay_id = 9_600_005;
    let replay_plan = InteractionActionPlanDigestV1::parse("b".repeat(64)).unwrap();
    let mut replay =
        prepare_deferred_receipt(&store, &content_hash, replay_id, replay_plan.clone()).await;
    let mut replay_action = create_role_effect_action(0);
    replay_action["correlation_marker"] = serde_json::json!("8".repeat(64));
    bind_effect_plan_document(
        &database.executor_pool,
        &replay,
        &replay_plan,
        serde_json::json!([replay_action, edit_response_effect_action(1)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut replay)
        .await
        .unwrap();
    assert_eq!(
        intend_create_role_effect(&database.executor_pool, &replay)
            .await
            .unwrap(),
        ("intended".to_string(), "intended".to_string(), 2)
    );
    make_effect_recovery_due(&database.owner_pool, replay_id).await;
    force_receipt_claim_expired(&database.owner_pool, replay_id).await;
    assert_eq!(
        claim_effect_recovery(
            &database.owner_pool,
            &database.executor_pool,
            replay_id,
            0,
            2,
        )
        .await
        .unwrap(),
        (
            "recovery_claimed".to_string(),
            "observing".to_string(),
            3,
            1
        )
    );
    let replay_digest = vec![0x76_u8; 32];
    assert_eq!(
        reconcile_observed_effect(
            &database.owner_pool,
            &database.executor_pool,
            replay_id,
            (3, 1),
            ("conflict", &replay_digest),
            0,
        )
        .await
        .unwrap(),
        ("conflict".to_string(), "recovery_required".to_string(), 4)
    );
    assert_eq!(
        reconcile_observed_effect(
            &database.owner_pool,
            &database.executor_pool,
            replay_id,
            (3, 1),
            ("conflict", &replay_digest),
            0,
        )
        .await
        .unwrap(),
        (
            "exact_replay".to_string(),
            "recovery_required".to_string(),
            4
        )
    );
    let wrong_outcome_before: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(replay_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let stale_authority = reconcile_observed_effect(
        &database.owner_pool,
        &database.executor_pool,
        replay_id,
        (3, 1),
        ("conflict", &replay_digest),
        1,
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&stale_authority).as_deref(), Some("RI001"));
    let wrong_outcome = reconcile_observed_effect(
        &database.owner_pool,
        &database.executor_pool,
        replay_id,
        (3, 1),
        ("unsupported", &replay_digest),
        0,
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&wrong_outcome).as_deref(), Some("RI001"));
    let wrong_outcome_after: (String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(replay_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(wrong_outcome_after, wrong_outcome_before);

    let response_finalize_id = 9_600_007;
    let response_finalize_plan = InteractionActionPlanDigestV1::parse("1".repeat(64)).unwrap();
    let mut response_finalize = prepare_deferred_receipt(
        &store,
        &content_hash,
        response_finalize_id,
        response_finalize_plan.clone(),
    )
    .await;
    bind_effect_plan_document(
        &database.executor_pool,
        &response_finalize,
        &response_finalize_plan,
        serde_json::json!([edit_response_effect_action(0)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut response_finalize)
        .await
        .unwrap();
    assert_eq!(
        intend_edit_response_effect(&database.executor_pool, &response_finalize).await,
        ("intended".to_string(), "intended".to_string(), 2)
    );
    make_effect_recovery_due(&database.owner_pool, response_finalize_id).await;
    force_receipt_claim_expired(&database.owner_pool, response_finalize_id).await;
    let response_finalize_claim = claim_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_finalize_id,
        2,
    )
    .await
    .unwrap();
    assert_eq!(response_finalize_claim.0, "response_tail_claimed");
    assert_eq!(response_finalize_claim.1, "observing");
    assert_eq!(response_finalize_claim.2, 3);
    assert_eq!(response_finalize_claim.3, 1);
    assert_eq!(response_finalize_claim.4, 1);
    assert_eq!(response_finalize_claim.5, "executing");
    let response_finalize_observation = vec![0x7a_u8; 32];
    let response_finalize_terminal = vec![0x99_u8; 32];
    let response_finalize_result = finalize_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_finalize_id,
        (response_finalize_claim.6, 3, 1),
        RECEIPT_PROCESS_ID,
        (
            "exact_success",
            &response_finalize_observation,
            &response_finalize_terminal,
        ),
    )
    .await
    .unwrap();
    assert_eq!(response_finalize_result.0, "effects_recovered_completed");
    assert_eq!(response_finalize_result.1, "reconciled_succeeded");
    assert_eq!(response_finalize_result.2, 4);
    assert_eq!(response_finalize_result.3, "completed");
    assert_eq!(response_finalize_result.4, response_finalize_claim.6 + 1);
    let response_finalize_replay = finalize_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_finalize_id,
        (response_finalize_claim.6, 3, 1),
        RECEIPT_PROCESS_ID,
        (
            "exact_success",
            &response_finalize_observation,
            &response_finalize_terminal,
        ),
    )
    .await
    .unwrap();
    assert_eq!(response_finalize_replay.0, "exact_replay");
    assert_eq!(response_finalize_replay.1, "reconciled_succeeded");
    assert_eq!(response_finalize_replay.2, 4);
    assert_eq!(response_finalize_replay.3, "completed");
    assert_eq!(response_finalize_replay.4, response_finalize_claim.6 + 1);
    let response_finalize_before: (String, String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_receipt_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let stale_response_process = finalize_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_finalize_id,
        (response_finalize_claim.6, 3, 1),
        "process-receipt-stale",
        (
            "exact_success",
            &response_finalize_observation,
            &response_finalize_terminal,
        ),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        sqlstate(&stale_response_process).as_deref(),
        Some("RI001" | "RI004")
    ));
    let response_finalize_after: (String, String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_receipt_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(response_finalize_after, response_finalize_before);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_receipt_events_v1 \
         SET outcome_code = 'interaction_response_unrecoverable' \
         WHERE application_id = $1 AND interaction_id = $2 AND event_revision = $3",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .bind(response_finalize_result.4)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let tampered_receipt_before: (String, String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_receipt_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let tampered_receipt = finalize_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_finalize_id,
        (response_finalize_claim.6, 3, 1),
        RECEIPT_PROCESS_ID,
        (
            "exact_success",
            &response_finalize_observation,
            &response_finalize_terminal,
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&tampered_receipt).as_deref(), Some("RI001"));
    let tampered_receipt_after: (String, String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_receipt_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(tampered_receipt_after, tampered_receipt_before);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_receipt_events_v1 \
         SET outcome_code = 'effects_recovered_completed' \
         WHERE application_id = $1 AND interaction_id = $2 AND event_revision = $3",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .bind(response_finalize_result.4)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        finalize_response_tail(
            &database.owner_pool,
            &database.executor_pool,
            response_finalize_id,
            (response_finalize_claim.6, 3, 1),
            RECEIPT_PROCESS_ID,
            (
                "exact_success",
                &response_finalize_observation,
                &response_finalize_terminal,
            ),
        )
        .await
        .unwrap()
        .0,
        "exact_replay"
    );
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_events_v1 \
         SET outcome_code = 'observed_failure' \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0 \
           AND event_revision = 4",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let tampered_response_before: (String, String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_receipt_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let tampered_response = finalize_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_finalize_id,
        (response_finalize_claim.6, 3, 1),
        RECEIPT_PROCESS_ID,
        (
            "exact_success",
            &response_finalize_observation,
            &response_finalize_terminal,
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&tampered_response).as_deref(), Some("RI001"));
    let tampered_response_after: (String, String, String) = sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(head)::TEXT, \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(event) \
                    ORDER BY event.event_revision)::TEXT \
                 FROM public.runtime_interaction_receipt_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_finalize_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(tampered_response_after, tampered_response_before);

    let response_id = 9_600_002;
    let response_plan = InteractionActionPlanDigestV1::parse("e".repeat(64)).unwrap();
    let mut response =
        prepare_deferred_receipt(&store, &content_hash, response_id, response_plan.clone()).await;
    bind_effect_plan_document(
        &database.executor_pool,
        &response,
        &response_plan,
        serde_json::json!([edit_response_effect_action(0)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut response)
        .await
        .unwrap();
    assert_eq!(
        intend_edit_response_effect(&database.executor_pool, &response).await,
        ("intended".to_string(), "intended".to_string(), 2)
    );
    force_receipt_claim_expired(&database.owner_pool, response_id).await;
    let response_result = vec![0x71_u8; 32];
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_interaction_effect_heads_v1 \
         SET state = 'observation_pending', head_revision = 3, \
             observation_attempt_count = 64, result_digest = $3, \
             result_at = pg_catalog.clock_timestamp(), \
             next_recovery_at = pg_catalog.clock_timestamp() - INTERVAL '1 second', \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_id.to_string())
    .bind(&response_result)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let response_blocked = claim_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        response_id,
        3,
    )
    .await
    .unwrap();
    assert_eq!(response_blocked.0, "interaction_response_unrecoverable");
    assert_eq!(response_blocked.1, "recovery_required");
    assert_eq!(response_blocked.2, 4);
    assert_eq!(response_blocked.3, 0);
    assert_eq!(response_blocked.4, 64);
    assert_eq!(response_blocked.5, "completed");
    let response_events: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        claim_response_tail(
            &database.owner_pool,
            &database.executor_pool,
            response_id,
            3,
        )
        .await
        .unwrap(),
        response_blocked
    );
    let response_evidence: (Vec<u8>, i64, i64) = sqlx::query_as(
        "SELECT result_digest, \
                (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event \
                 WHERE event.application_id = head.application_id \
                   AND event.interaction_id = head.interaction_id \
                   AND event.action_index = head.action_index), \
                (SELECT pg_catalog.count(*) FROM public.runtime_interaction_receipt_token_secrets_v1 AS token \
                 WHERE token.application_id = head.application_id \
                   AND token.interaction_id = head.interaction_id) \
         FROM public.runtime_interaction_effect_heads_v1 AS head \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(response_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(response_evidence, (response_result, response_events, 0));

    let lost_reply_id = 9_600_003;
    let lost_reply_plan = InteractionActionPlanDigestV1::parse("d".repeat(64)).unwrap();
    let mut lost_reply = prepare_deferred_receipt(
        &store,
        &content_hash,
        lost_reply_id,
        lost_reply_plan.clone(),
    )
    .await;
    bind_effect_plan_document(
        &database.executor_pool,
        &lost_reply,
        &lost_reply_plan,
        serde_json::json!([edit_response_effect_action(0)]),
    )
    .await
    .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut lost_reply)
        .await
        .unwrap();
    intend_edit_response_effect(&database.executor_pool, &lost_reply).await;
    make_effect_recovery_due(&database.owner_pool, lost_reply_id).await;
    force_receipt_claim_expired(&database.owner_pool, lost_reply_id).await;
    remove_interaction_token(&database.owner_pool, lost_reply_id).await;
    let lost_reply_first = claim_response_tail(
        &database.owner_pool,
        &database.executor_pool,
        lost_reply_id,
        2,
    )
    .await
    .unwrap();
    assert_eq!(lost_reply_first.0, "interaction_response_unrecoverable");
    assert_eq!(lost_reply_first.2, 4);
    let lost_reply_events: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(lost_reply_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(
        claim_response_tail(
            &database.owner_pool,
            &database.executor_pool,
            lost_reply_id,
            2,
        )
        .await
        .unwrap(),
        lost_reply_first
    );
    let lost_reply_replay_events: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 \
         WHERE application_id = $1 AND interaction_id = $2 AND action_index = 0",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(lost_reply_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(lost_reply_replay_events, lost_reply_events);
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

    let known_failed_id = 9_200_014;
    let mut known_failed = acquire_test_receipt(
        &store,
        &content_hash,
        known_failed_id,
        "button:known-failed",
        Duration::from_secs(30),
    )
    .await;
    let known_failure = RuntimeInteractionReceiptTerminalOutcomeV1::new(
        RuntimeInteractionReceiptTerminalStateV1::Failed,
        "known_no_effect_failure",
        RuntimeInteractionReceiptOpaqueDigestV1::new([0x31; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .finish_interaction_receipt_v1(&mut known_failed, known_failure.clone())
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(
        store
            .finish_interaction_receipt_v1(&mut known_failed, known_failure)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::ExactReplay
    );

    let ambiguous_id = 9_200_015;
    let mut ambiguous = acquire_test_receipt(
        &store,
        &content_hash,
        ambiguous_id,
        "button:ambiguous",
        Duration::from_secs(30),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(
            &mut ambiguous,
            InteractionActionPlanDigestV1::parse("3".repeat(64)).unwrap(),
        )
        .await
        .unwrap();
    store
        .intend_interaction_receipt_execution_v1(&mut ambiguous)
        .await
        .unwrap();
    let recovery_required = RuntimeInteractionReceiptTerminalOutcomeV1::new(
        RuntimeInteractionReceiptTerminalStateV1::RecoveryRequired,
        "ambiguous_external_effect",
        RuntimeInteractionReceiptOpaqueDigestV1::new([0x32; 32]),
    )
    .unwrap();
    assert_eq!(
        store
            .finish_interaction_receipt_v1(&mut ambiguous, recovery_required.clone())
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::Applied
    );
    assert_eq!(
        store
            .finish_interaction_receipt_v1(&mut ambiguous, recovery_required)
            .await
            .unwrap(),
        RuntimeInteractionReceiptMutationDispositionV1::ExactReplay
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
    let unsafe_failed = sqlx::query(
        "SELECT * FROM public.starring_runtime_interaction_receipt_finish_v1(\
            $1::TEXT, $2::TEXT, $3::BIGINT, $4::BIGINT, $5::TEXT, $6::BYTEA, \
            'failed'::TEXT, 'unsafe_failed'::TEXT, $7::BYTEA\
         )",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(lifecycle_id.to_string())
    .bind(i64::try_from(lifecycle.head_revision()).unwrap())
    .bind(i64::try_from(lifecycle.claim_revision()).unwrap())
    .bind(lifecycle.claim_process_instance_id().as_str())
    .bind(vec![0x11_u8; 32])
    .bind(vec![0x22_u8; 32])
    .fetch_all(&database.executor_pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&unsafe_failed).as_deref(), Some("RI001"));
    let (unsafe_failed_state, unsafe_failed_token_count): (String, i64) = sqlx::query_as(
        "SELECT head.state, pg_catalog.count(secret.application_id) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         LEFT JOIN public.runtime_interaction_receipt_token_secrets_v1 AS secret \
           ON secret.application_id = head.application_id \
          AND secret.interaction_id = head.interaction_id \
         WHERE head.application_id = $1 AND head.interaction_id = $2 \
         GROUP BY head.state",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(lifecycle_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(unsafe_failed_state, "executing");
    assert_eq!(unsafe_failed_token_count, 1);
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
    let persisted_terminal: (String, Vec<u8>) = sqlx::query_as(
        "SELECT terminal_outcome_code, terminal_result_digest \
         FROM public.runtime_interaction_receipt_heads_v1 \
         WHERE application_id = $1 AND interaction_id = $2",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(lifecycle_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(persisted_terminal.0, "completed_test");
    assert_eq!(persisted_terminal.1, vec![4; 32]);
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
    let redelivery_id = 9_200_016;
    let mut redelivery_claim = acquire_test_receipt(
        &store,
        &content_hash,
        redelivery_id,
        "button:redelivery",
        Duration::from_secs(1),
    )
    .await;
    store
        .bind_interaction_receipt_action_plan_v1(
            &mut redelivery_claim,
            InteractionActionPlanDigestV1::parse("a".repeat(64)).unwrap(),
        )
        .await
        .unwrap();
    store
        .intend_interaction_receipt_initial_response_v1(
            &mut redelivery_claim,
            RuntimeInteractionReceiptInitialResponseIntentV1::new(
                RuntimeInteractionReceiptInitialResponseKindV1::RespondEphemeral,
                RuntimeInteractionReceiptOpaqueDigestV1::new([22; 32]),
            ),
        )
        .await
        .unwrap();
    let redelivery_request = test_receipt_request(
        &store,
        &content_hash,
        redelivery_id,
        "button:redelivery",
        Duration::from_secs(1),
    )
    .await;
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
    let redelivery_candidate = candidate(redelivery_id);
    let redelivery_terminalize =
        RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
            &redelivery_candidate,
            &receipt_expected_route(&content_hash),
            RuntimeInteractionReceiptOpaqueDigestV1::new([23; 32]),
        )
        .unwrap();
    let (redelivery, redelivery_supervisor) = tokio::join!(
        store.claim_interaction_receipt_v1(redelivery_request),
        store.terminalize_expired_interaction_receipt_v1(redelivery_terminalize),
    );
    assert!(matches!(
        redelivery.unwrap(),
        RuntimeInteractionReceiptClaimOutcomeV1::RecoveryRequired(_)
    ));
    assert!(matches!(
        redelivery_supervisor.unwrap().disposition(),
        RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::RecoveryRequired
            | RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::TerminalReceipt
    ));
    let (
        redelivery_state,
        redelivery_acknowledgement_state,
        redelivery_acknowledgement_result,
        redelivery_token_count,
        redelivery_terminal_event_count,
    ): (String, String, Option<String>, i64, i64) = sqlx::query_as(
        "SELECT head.state, head.acknowledgement_state, head.acknowledgement_result, \
                pg_catalog.count(DISTINCT secret.interaction_id), \
                pg_catalog.count(DISTINCT event.event_revision) \
         FROM public.runtime_interaction_receipt_heads_v1 AS head \
         LEFT JOIN public.runtime_interaction_receipt_token_secrets_v1 AS secret \
           ON secret.application_id = head.application_id \
          AND secret.interaction_id = head.interaction_id \
         LEFT JOIN public.runtime_interaction_receipt_events_v1 AS event \
           ON event.application_id = head.application_id \
          AND event.interaction_id = head.interaction_id \
          AND event.event_kind = 'recovery_required' \
         WHERE head.application_id = $1 AND head.interaction_id = $2 \
         GROUP BY head.state, head.acknowledgement_state, head.acknowledgement_result",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(redelivery_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(redelivery_state, "recovery_required");
    assert_eq!(
        redelivery_acknowledgement_state,
        "response_recovery_terminal"
    );
    assert_eq!(
        redelivery_acknowledgement_result.as_deref(),
        Some("indeterminate")
    );
    assert_eq!(redelivery_token_count, 0);
    assert_eq!(redelivery_terminal_event_count, 1);
    let successor_candidate = candidate(successor_id);
    let successor_recovery = store
        .recover_interaction_receipt_v1(
            RuntimeInteractionReceiptRecoveryRequestV1::new(
                successor_candidate.clone(),
                receipt_expected_route_for(
                    &content_hash,
                    "process-successor",
                    RECEIPT_BUILD_REVISION,
                ),
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

    let successor_attestation_id =
        advance_receipt_authority_to_successor(&database.owner_pool, "process-successor").await;
    let successor_route = receipt_expected_route_for_authority(
        &content_hash,
        "process-successor",
        RECEIPT_BUILD_REVISION,
        2,
        7,
    );
    let successor_left =
        RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
            &successor_candidate,
            &successor_route,
            RuntimeInteractionReceiptOpaqueDigestV1::new([21; 32]),
        )
        .unwrap();
    let successor_right =
        RuntimeInteractionReceiptTerminalizeExpiredRequestV1::from_recovery_candidate(
            &successor_candidate,
            &successor_route,
            RuntimeInteractionReceiptOpaqueDigestV1::new([21; 32]),
        )
        .unwrap();
    let (successor_left, successor_right) = tokio::join!(
        store.terminalize_expired_interaction_receipt_v1(successor_left),
        store.terminalize_expired_interaction_receipt_v1(successor_right),
    );
    let successor_dispositions = [
        successor_left.unwrap().disposition(),
        successor_right.unwrap().disposition(),
    ];
    assert_eq!(
        successor_dispositions
            .iter()
            .filter(|disposition| {
                **disposition
                    == RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::PristineClaimAbandoned
            })
            .count(),
        1
    );
    assert_eq!(
        successor_dispositions
            .iter()
            .filter(|disposition| {
                **disposition
                    == RuntimeInteractionReceiptTerminalizeExpiredDispositionV1::TerminalReceipt
            })
            .count(),
        1
    );
    let (root_attestation_id, origin_process_instance_id, successor_event_count): (
        String,
        String,
        i64,
    ) = sqlx::query_as(
        "SELECT root.attestation_id, root.origin_process_instance_id, \
                pg_catalog.count(event.event_revision) \
         FROM public.runtime_interaction_receipt_roots_v1 AS root \
         LEFT JOIN public.runtime_interaction_receipt_events_v1 AS event \
           ON event.application_id = root.application_id \
          AND event.interaction_id = root.interaction_id \
          AND event.outcome_code = 'expired_pristine_claim_abandoned' \
         WHERE root.application_id = $1 AND root.interaction_id = $2 \
         GROUP BY root.attestation_id, root.origin_process_instance_id",
    )
    .bind(RECEIPT_APPLICATION_ID.to_string())
    .bind(successor_id.to_string())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_ne!(root_attestation_id, successor_attestation_id);
    assert_eq!(origin_process_instance_id, RECEIPT_PROCESS_ID);
    assert_eq!(successor_event_count, 1);

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
