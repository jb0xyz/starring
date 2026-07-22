use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceRegistrarV1, InstanceResources,
    InstanceRouteReaderV1, InstanceRuleSetVersion, InstanceStatus, InstanceStoreError,
};
use automation_ruleset_dispatch::{PinnedInstanceResolverErrorV1, PinnedInstanceResolverV1};
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1,
    RuntimeInteractionRouteTimeoutV1, MIGRATOR,
};
use discord_model::{GuildId, RoleId, UserId};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::{Connection, Executor, PgPool};

const READINESS_FUNCTION: &str = "public.starring_runtime_interaction_database_readiness_v1()";
const ROUTE_FUNCTION: &str = "public.starring_runtime_interaction_route_read_v1(TEXT,TEXT)";
const PINNED_FUNCTION: &str = "public.starring_runtime_interaction_pinned_read_v1(TEXT,TEXT)";
const REGISTER_FUNCTION: &str =
    "public.starring_runtime_interaction_instance_register_v1(TEXT,TEXT,TEXT,BIGINT,TEXT,TEXT,JSONB)";

struct IsolatedDatabase {
    name: String,
    role: String,
    administrator: PgConnection,
    owner_pool: PgPool,
    executor_pool: PgPool,
    deadline_pool: PgPool,
}

fn function_grant(function: &str, role: &str) -> String {
    format!("GRANT EXECUTE ON FUNCTION {function} TO {role}")
}

async fn isolated_database() -> IsolatedDatabase {
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
    let password = format!("ri_test_password_{suffix}");
    for identifier in [&name, &role] {
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
    MIGRATOR.run(&owner_pool).await.unwrap();
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON DATABASE {name} FROM PUBLIC"),
        "REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC".to_string(),
        format!("GRANT CONNECT ON DATABASE {name} TO {role}"),
        format!("GRANT USAGE ON SCHEMA public TO {role}"),
        function_grant(READINESS_FUNCTION, &role),
        function_grant(ROUTE_FUNCTION, &role),
        function_grant(PINNED_FUNCTION, &role),
        function_grant(REGISTER_FUNCTION, &role),
    ] {
        owner_pool.execute(statement.as_str()).await.unwrap();
    }

    let executor_options = base.database(&name).username(&role).password(&password);
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
    IsolatedDatabase {
        name,
        role,
        administrator,
        owner_pool,
        executor_pool,
        deadline_pool,
    }
}

async fn cleanup(mut database: IsolatedDatabase) {
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
fn interaction_migration_is_private_bounded_and_comment_free() {
    let migration =
        include_str!("../../../migrations/202607220027_scope_runtime_interaction_database.sql");
    for function in [
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
async fn exact_capabilities_preserve_binding_inactivity_and_least_privilege() {
    let database = isolated_database().await;
    let owner_pool = database.owner_pool.clone();
    let executor_pool = database.executor_pool.clone();
    let deadline_pool = database.deadline_pool.clone();
    let database_name = database.name.clone();
    let executor_role = database.role.clone();
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
            owner_pool.execute(revoke.as_str()).await.unwrap();
            store.verify_database_v1().await.unwrap();
        }

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
