use std::borrow::Cow;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceRegistrarV1, InstanceResources,
    InstanceRouteReaderV1, InstanceRuleSetVersion, InstanceStatus, InstanceStoreError,
    InstanceTeardownClaimOutcomeV1, InstanceTeardownMarkOutcomeV1,
    InstanceTeardownRetryScanCursorV2, InstanceTeardownRetryScannerV2, InstanceTeardownStoreV1,
    MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1, MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2,
};
use automation_ruleset_dispatch::{PinnedInstanceResolverErrorV1, PinnedInstanceResolverV1};
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1,
    RuntimeInteractionRouteTimeoutV1, MIGRATOR,
};
use discord_model::{GuildId, RoleId, UserId};
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
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
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
