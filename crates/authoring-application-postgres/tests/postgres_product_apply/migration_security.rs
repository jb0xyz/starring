#[derive(Clone)]
struct SlotRaceFixture {
    tenant_id: String,
    installation_id: String,
    principal_id: String,
    user_id: String,
    application_id: String,
    guild_id: String,
    ruleset_key: String,
    activation_id: String,
}

async fn seed_slot_race_fixture(pool: &PgPool, label: &str) -> SlotRaceFixture {
    let unique = suffix();
    let tail = unique[unique.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let fixture = SlotRaceFixture {
        tenant_id: format!("race-tenant-{label}-{unique}"),
        installation_id: format!("race-installation-{label}-{unique}"),
        principal_id: format!("race-principal-{label}-{unique}"),
        user_id: (10_000_000_000 + tail).to_string(),
        application_id: (11_000_000_000 + tail).to_string(),
        guild_id: (12_000_000_000 + tail).to_string(),
        ruleset_key: format!(
            "race_{label}_{}",
            &unique[unique.len().saturating_sub(20)..]
        ),
        activation_id: format!("race_activation_{label}_{unique}"),
    };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) VALUES ($1, $2, '{}'::JSONB)",
    )
    .bind(&fixture.principal_id)
    .bind(&fixture.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) VALUES ($1, 'active', $2)",
    )
    .bind(&fixture.tenant_id)
    .bind(format!("Race {label} {unique}"))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ($1, $2, 2)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, \
          pg_catalog.jsonb_build_object('version', 1, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind("9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6")
    .bind(&fixture.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', \
          pg_catalog.clock_timestamp(), pg_catalog.clock_timestamp() + INTERVAL '1 hour')",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind("9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6")
    .bind(&fixture.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    fixture
}

async fn insert_slot_race_installation_rows(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &SlotRaceFixture,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .bind(&fixture.application_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, '{}'::JSONB, $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .bind(digest(&format!("race-binding:{}", fixture.installation_id)))
    .bind(digest(&format!("race-authority:{}", fixture.installation_id)))
    .bind(&fixture.principal_id)
    .bind(digest(&format!("race-request:{}", fixture.installation_id)))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn wait_for_advisory_lock_wait(pool: &PgPool, process_id: i32) {
    for _ in 0..500 {
        let waiting = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(\
              SELECT 1 FROM pg_catalog.pg_locks \
              WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
        )
        .bind(process_id)
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected PostgreSQL advisory lock wait");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_preflight_refuses_ambiguous_applied_history() {
    let database = isolated_database("upgrade").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_007)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let fixture = seed_fixture(&database.pool).await;
        sqlx::query(
            "UPDATE public.activation_requests \
             SET state = 'applied', applied_at = pg_catalog.clock_timestamp(), applied_by = $2, \
              completion_kind = 'already_active', activation_notices = '[]'::JSONB, \
              product_revision = product_revision + 1 \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .bind(&fixture.actor.user_id)
        .execute(&database.pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_008)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject Applied history without a deployment");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected migration preflight database error");
        };
        assert_eq!(
            database_error.constraint(),
            Some("atomic_product_apply_upgrade_deployment_complete")
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn artifact_integrity_upgrade_atomically_rejects_self_consistent_shadow_drift() {
    let database = isolated_database("artifact_upgrade").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_011)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let fixture = seed_fixture(&database.pool).await;
        let changed = sqlx::query(
            "UPDATE public.automation_ruleset_versions AS version \
             SET definition = pg_catalog.jsonb_build_object(\
              'version', 2, 'panels', '[]'::JSONB, 'modals', '[]'::JSONB, 'rules', '[]'::JSONB), \
              content_hash = $2 \
             FROM public.activation_requests AS activation \
             WHERE activation.id = $1 AND version.guild_id = activation.guild_id \
               AND version.ruleset_key = activation.ruleset_key \
               AND version.version = activation.target_version",
        )
        .bind(&fixture.activation_id)
        .bind("91d936ba08910497f8f31e16e7f2b1ffce5ee9447a4636d47ddddc5c79fb0103")
        .execute(&database.pool)
        .await?;
        assert_eq!(changed.rows_affected(), 1);
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_012)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject self-consistent artifact shadow drift");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected migration preflight database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("23514"));
        assert_eq!(
            database_error.constraint(),
            Some("ruleset_shadow_target_integrity")
        );
        let generated_column_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
             SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'automation_ruleset_versions' \
               AND column_name = 'canonical_content_hash')",
        )
        .fetch_one(&database.pool)
        .await?;
        assert!(!generated_column_exists);
        let mutation_guard_exists = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.to_regprocedure(\
              'public.reject_ruleset_artifact_mutation()') IS NOT NULL",
        )
        .fetch_one(&database.pool)
        .await?;
        assert!(!mutation_guard_exists);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_slot_upgrade_rejects_committed_product_applying_residue() {
    let database = isolated_database("slot_applying").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_012)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let fixture = seed_fixture(&database.pool).await;
        let mut transaction = database.pool.begin().await?;
        sqlx::query(
            "SELECT pg_catalog.set_config(\
              'starring.product_approval_context_digest', approval_context_digest, TRUE) \
             FROM public.activation_requests WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .execute(&mut *transaction)
        .await?;
        let changed = sqlx::query(
            "UPDATE public.activation_requests \
             SET state = 'applying', apply_attempt_id = $2, apply_attempt_no = 1, \
              apply_lease_until = pg_catalog.clock_timestamp() + INTERVAL '1 minute' \
             WHERE id = $1 AND state = 'approved'",
        )
        .bind(&fixture.activation_id)
        .bind(format!("upgrade_applying_{}", suffix()))
        .execute(&mut *transaction)
        .await?;
        assert_eq!(changed.rows_affected(), 1);
        transaction.commit().await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_013)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject committed product Applying residue");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected migration preflight database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("23514"));
        assert_eq!(
            database_error.constraint(),
            Some("product_activation_applying_residue_absent")
        );
        let helper_exists = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.to_regprocedure(\
              'public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)') \
             IS NOT NULL",
        )
        .fetch_one(&database.pool)
        .await?;
        assert!(!helper_exists);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_slot_upgrade_rejects_pointer_without_deployment_lineage() {
    let database = isolated_database("slot_pointer").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_012)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let fixture = seed_fixture(&database.pool).await;
        sqlx::query(
            "INSERT INTO public.automation_ruleset_activations \
             (guild_id, ruleset_key, active_version) VALUES ($1, $2, 1)",
        )
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .execute(&database.pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_013)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject product pointer without deployment lineage");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected migration preflight database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("23514"));
        assert_eq!(
            database_error.constraint(),
            Some("product_ruleset_slot_pointer_exact")
        );
        let trigger_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(\
              SELECT 1 FROM pg_catalog.pg_trigger \
              WHERE tgname = 'automation_ruleset_activations_assert_product_slot')",
        )
        .fetch_one(&database.pool)
        .await?;
        assert!(!trigger_exists);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_slot_takeover_requires_atomic_deployment_lineage() {
    let pool = pool().await;
    let unique = suffix();
    let tail = unique[unique.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let tenant_id = format!("takeover-tenant-{unique}");
    let installation_id = format!("takeover-installation-{unique}");
    let principal_id = format!("takeover-principal-{unique}");
    let user_id = (7_000_000_000 + tail).to_string();
    let application_id = (8_000_000_000 + tail).to_string();
    let guild_id = (9_000_000_000 + tail).to_string();
    let ruleset_key = format!(
        "takeover_ruleset_{}",
        &unique[unique.len().saturating_sub(20)..]
    );
    let mut pointer_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ($1, $2, 2)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, \
          pg_catalog.jsonb_build_object('version', 1, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .bind("9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6")
    .bind(&user_id)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ($1, $2, 1)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    pointer_transaction.commit().await.unwrap();

    let mut takeover = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *takeover)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) VALUES ($1, $2, '{}'::JSONB)",
    )
    .bind(&principal_id)
    .bind(&user_id)
    .execute(&mut *takeover)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) VALUES ($1, 'active', $2)",
    )
    .bind(&tenant_id)
    .bind(format!("Takeover {unique}"))
    .execute(&mut *takeover)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(&application_id)
    .bind(&guild_id)
    .bind(&ruleset_key)
    .execute(&mut *takeover)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, '{}'::JSONB, $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(digest(&format!("takeover-binding:{unique}")))
    .bind(digest(&format!("takeover-authority:{unique}")))
    .bind(&principal_id)
    .bind(digest(&format!("takeover-request:{unique}")))
    .execute(&mut *takeover)
    .await
    .unwrap();
    let error = takeover
        .commit()
        .await
        .expect_err("legacy pointer takeover must require atomic deployment lineage");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint() == Some("product_ruleset_slot_pointer_exact")
    ));
    let installation_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(\
          SELECT 1 FROM public.automation_installations WHERE installation_id = $1)",
    )
    .bind(&installation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!installation_exists);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_slot_database_guard_rejects_raw_legacy_applying_transition() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let legacy_id = format!("legacy_raw_{}", suffix());
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at) \
         SELECT $1, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, 'pending', pg_catalog.clock_timestamp(), \
          pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
         FROM public.activation_requests WHERE id = $2",
    )
    .bind(&legacy_id)
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await
    .unwrap();
    let error = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applying', apply_attempt_id = $2, apply_attempt_no = 1, \
          apply_lease_until = pg_catalog.clock_timestamp() + INTERVAL '1 minute' \
         WHERE id = $1",
    )
    .bind(&legacy_id)
    .bind(format!("legacy_raw_attempt_{}", suffix()))
    .execute(&pool)
    .await
    .expect_err("raw legacy Applying transition must respect product slot ownership");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_ruleset_slot_legacy_apply_forbidden")
    ));
    let persisted = sqlx::query_as::<_, (String, i64, Option<String>)>(
        "SELECT state, apply_attempt_no, apply_attempt_id \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&legacy_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, ("pending".to_string(), 0, None));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn legacy_apply_lock_wins_then_product_installation_waits_and_fails() {
    let pool = pool().await;
    let fixture = seed_slot_race_fixture(&pool, "legacy_first").await;
    let mut legacy = pool.begin().await.unwrap();
    let changed = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applying', apply_attempt_id = $2, apply_attempt_no = 1, \
          apply_lease_until = pg_catalog.clock_timestamp() + INTERVAL '1 minute' \
         WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(format!("legacy_first_{}", suffix()))
    .execute(&mut *legacy)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    let (started_sender, started_receiver) = futures::channel::oneshot::channel();
    let install_pool = pool.clone();
    let install_fixture = fixture.clone();
    let installation = tokio::spawn(async move {
        let mut transaction = install_pool.begin().await?;
        let process_id = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut *transaction)
            .await?;
        let _ = started_sender.send(process_id);
        if let Err(error) =
            insert_slot_race_installation_rows(&mut transaction, &install_fixture).await
        {
            let _ = transaction.rollback().await;
            return Err(error);
        }
        transaction.commit().await
    });
    let process_id = started_receiver.await.unwrap();
    wait_for_advisory_lock_wait(&pool, process_id).await;
    legacy.commit().await.unwrap();
    let error = installation
        .await
        .unwrap()
        .expect_err("installation must recheck committed legacy Applying state");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_ruleset_slot_legacy_apply_absent")
    ));
    let installation_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(\
          SELECT 1 FROM public.automation_installations WHERE installation_id = $1)",
    )
    .bind(&fixture.installation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!installation_exists);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_installation_lock_wins_then_legacy_apply_waits_and_fails() {
    let pool = pool().await;
    let fixture = seed_slot_race_fixture(&pool, "product_first").await;
    let mut installation = pool.begin().await.unwrap();
    insert_slot_race_installation_rows(&mut installation, &fixture)
        .await
        .unwrap();
    let (started_sender, started_receiver) = futures::channel::oneshot::channel();
    let legacy_pool = pool.clone();
    let activation_id = fixture.activation_id.clone();
    let legacy = tokio::spawn(async move {
        let mut connection = legacy_pool.acquire().await?;
        let process_id = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut *connection)
            .await?;
        let _ = started_sender.send(process_id);
        sqlx::query(
            "UPDATE public.activation_requests \
             SET state = 'applying', apply_attempt_id = $2, apply_attempt_no = 1, \
              apply_lease_until = pg_catalog.clock_timestamp() + INTERVAL '1 minute' \
             WHERE id = $1",
        )
        .bind(&activation_id)
        .bind(format!("product_first_{}", suffix()))
        .execute(&mut *connection)
        .await
    });
    let process_id = started_receiver.await.unwrap();
    wait_for_advisory_lock_wait(&pool, process_id).await;
    installation.commit().await.unwrap();
    let error = legacy
        .await
        .unwrap()
        .expect_err("legacy Apply must recheck committed product ownership");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_ruleset_slot_legacy_apply_forbidden")
    ));
    let persisted = sqlx::query_as::<_, (String, i64, Option<String>)>(
        "SELECT state, apply_attempt_no, apply_attempt_id \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, ("pending".to_string(), 0, None));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_activation_cannot_commit_applying_residue() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.set_config(\
          'starring.product_approval_context_digest', approval_context_digest, TRUE) \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applying', apply_attempt_id = $2, apply_attempt_no = 1, \
          apply_lease_until = pg_catalog.clock_timestamp() + INTERVAL '1 minute' \
         WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(format!("product_residue_{}", suffix()))
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    let error = transaction
        .commit()
        .await
        .expect_err("product Applying state must not survive commit");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_activation_applying_residue_absent")
    ));
    let persisted = sqlx::query_as::<_, (String, i64, Option<String>)>(
        "SELECT state, apply_attempt_no, apply_attempt_id \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, ("approved".to_string(), 0, None));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_apply_waits_at_writer_fence_before_product_row_locks() {
    let database = isolated_database("apply_writer_fence_order").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await.unwrap();
        let fixture = seed_fixture(&database.pool).await;
        let operation = Operation::new("writer-fence-order");
        let mut fence = database.pool.begin().await?;
        sqlx::query(
            "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0))",
        )
        .execute(&mut *fence)
        .await?;
        let (started_sender, started_receiver) = futures::channel::oneshot::channel();
        let apply_pool = database.pool.clone();
        let apply_fixture = fixture.clone();
        let apply_operation = operation.clone();
        let apply = tokio::spawn(async move {
            let mut transaction = begin_serializable(&apply_pool).await;
            let process_id = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
                .fetch_one(&mut *transaction)
                .await?;
            let _ = started_sender.send(process_id);
            let call = Call::valid(&apply_fixture);
            let locked = lock_apply(
                &mut transaction,
                &apply_fixture,
                &apply_operation,
                &call,
            )
            .await?;
            transaction.rollback().await?;
            Ok::<_, sqlx::Error>(locked.outcome)
        });
        let process_id = started_receiver.await.unwrap();
        wait_for_advisory_lock_wait(&database.pool, process_id).await;
        let mut row_probe = database.pool.begin().await?;
        let locked_activation = sqlx::query_scalar::<_, String>(
            "SELECT id FROM public.activation_requests WHERE id = $1 FOR UPDATE NOWAIT",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&mut *row_probe)
        .await?;
        assert_eq!(locked_activation, fixture.activation_id);
        row_probe.rollback().await?;
        fence.rollback().await?;
        assert_eq!(apply.await.unwrap()?, "ready");
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_apply_writer_fence_closed_and_missing_are_stable_failures() {
    let database = isolated_database("apply_writer_fence_state").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await.unwrap();
        let fixture = seed_fixture(&database.pool).await;
        sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
            .execute(&database.pool)
            .await?;
        sqlx::query(
            "UPDATE public.runtime_writer_fence \
             SET fence_state = 'closed', fence_generation = 2, \
              cutover_lease_epoch_high_water = 1, \
              cutover_coordinator_id = '0123456789abcdef0123456789abcdef', \
              cutover_expires_at = pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
             WHERE singleton",
        )
        .execute(&database.pool)
        .await?;
        sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
            .execute(&database.pool)
            .await?;

        let mut closed = begin_serializable(&database.pool).await;
        let closed_result = lock_apply(
            &mut closed,
            &fixture,
            &Operation::new("writer-fence-closed"),
            &Call::valid(&fixture),
        )
        .await?;
        assert_eq!(closed_result.outcome, "runtime_writer_fenced");
        closed.rollback().await?;

        sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
            .execute(&database.pool)
            .await?;
        sqlx::query("DELETE FROM public.runtime_writer_fence")
            .execute(&database.pool)
            .await?;
        sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
            .execute(&database.pool)
            .await?;

        let mut missing = begin_serializable(&database.pool).await;
        let missing_result = lock_apply(
            &mut missing,
            &fixture,
            &Operation::new("writer-fence-missing"),
            &Call::valid(&fixture),
        )
        .await?;
        assert_eq!(missing_result.outcome, "runtime_writer_fence_invalid");
        missing.rollback().await?;

        let unchanged = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT activation.state, activation.product_revision, \
              (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
               WHERE activation_request_id = activation.id) \
             FROM public.activation_requests AS activation WHERE activation.id = $1",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(unchanged, ("approved".to_string(), 2, 0));
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

async fn product_apply_writer_fence_catalog_state(pool: &PgPool) -> (String, bool, i64) {
    sqlx::query_as(
        "SELECT \
          pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
           pg_catalog.pg_get_functiondef(core.oid), 'UTF8')), 'hex'), \
          pg_catalog.to_regprocedure(\
           'public.starring_product_apply_lock_core_unfenced_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)') IS NULL, \
          (SELECT pg_catalog.count(*) \
           FROM pg_catalog.aclexplode(active.proacl) AS privilege \
           WHERE privilege.grantee = 0 \
            AND privilege.privilege_type = 'EXECUTE') \
         FROM pg_catalog.pg_proc AS core \
         CROSS JOIN pg_catalog.pg_proc AS active \
         WHERE core.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)') \
          AND active.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)')",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_apply_writer_fence_migration_rejects_public_capability_atomically() {
    let database = isolated_database("apply_writer_acl").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_240_001)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        for function in [
            "public.starring_product_apply_executor_database_identity_v1()",
            "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
            "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)",
            "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
            "public.starring_product_apply_keyring_coverage_v1(text[],text[])",
        ] {
            sqlx::query(&format!(
                "GRANT EXECUTE ON FUNCTION {function} TO PUBLIC"
            ))
            .execute(&database.pool)
            .await?;
        }
        let before = product_apply_writer_fence_catalog_state(&database.pool).await;
        assert!(before.1);
        assert_eq!(before.2, 1);
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_240_002)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("PUBLIC Apply capability must reject writer fence migration");
        assert!(matches!(
            error,
            sqlx::Error::Database(database_error)
                if database_error.code().as_deref() == Some("PA001")
                    && database_error.message()
                        == "product_apply_writer_fence_postflight_drift"
        ));
        let after = product_apply_writer_fence_catalog_state(&database.pool).await;
        assert_eq!(after, before);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

async fn function_execute(pool: &PgPool, role: &str, identity: &str) -> bool {
    sqlx::query_scalar("SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')")
        .bind(role)
        .bind(identity)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn function_grantable(pool: &PgPool, role: &str, identity: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (\
         SELECT 1 \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(\
          COALESCE(function_row.proacl, pg_catalog.acldefault('f', function_row.proowner))\
         ) AS privilege \
         INNER JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee \
         WHERE function_row.oid = pg_catalog.to_regprocedure($2) \
          AND role.rolname = $1 \
          AND privilege.privilege_type = 'EXECUTE' \
          AND privilege.is_grantable)",
    )
    .bind(role)
    .bind(identity)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn public_function_execute(pool: &PgPool, identity: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (\
         SELECT 1 \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(\
          COALESCE(function_row.proacl, pg_catalog.acldefault('f', function_row.proowner))\
         ) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
          AND privilege.grantee = 0 \
          AND privilege.privilege_type = 'EXECUTE')",
    )
    .bind(identity)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn run_migration_as(
    pool: &PgPool,
    migration_sql: &str,
    role: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(&format!("SET LOCAL ROLE {role}"))
        .execute(&mut *transaction)
        .await?;
    if let Err(error) = sqlx::raw_sql(migration_sql)
        .execute(&mut *transaction)
        .await
    {
        transaction.rollback().await?;
        return Err(error);
    }
    transaction.commit().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_requires_explicit_apply_authority_intersection() {
    let database = isolated_database("acl").await;
    let role_suffix = suffix();
    let migration_role = format!("starring_apply_migrator_{role_suffix}");
    let lock_only_role = format!("starring_apply_lock_only_{role_suffix}");
    let finalizer_only_role = format!("starring_apply_final_only_{role_suffix}");
    let intersection_role = format!("starring_apply_intersect_{role_suffix}");
    let grantable_role = format!("starring_apply_grantable_{role_suffix}");
    let mismatch_role = format!("starring_apply_mismatch_{role_suffix}");
    let post_lock_only_role = format!("starring_apply_post_lock_{role_suffix}");
    let roles = [
        &migration_role,
        &lock_only_role,
        &finalizer_only_role,
        &intersection_role,
        &grantable_role,
        &mismatch_role,
        &post_lock_only_role,
    ];
    let owner = sqlx::query_scalar::<_, String>("SELECT current_user")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let quoted_owner = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_ident($1)")
        .bind(&owner)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    for role in roles {
        sqlx::query(&format!("CREATE ROLE {role} NOLOGIN"))
            .execute(&database.pool)
            .await
            .unwrap();
    }
    sqlx::query(&format!("GRANT {quoted_owner} TO {migration_role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    let outcome = async {
        let lock_identity = "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)";
        let core_identity = "public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)";
        let finalizer_identity = "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)";
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_009)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let original = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT lock.oid::BIGINT, finalizer.oid::BIGINT, lock.proowner::BIGINT \
             FROM pg_catalog.pg_proc AS lock \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             WHERE lock.oid = pg_catalog.to_regprocedure($1) \
              AND finalizer.oid = pg_catalog.to_regprocedure($2)",
        )
        .bind(lock_identity)
        .bind(finalizer_identity)
        .fetch_one(&database.pool)
        .await?;
        for (identity, role, grantable) in [
            (lock_identity, lock_only_role.as_str(), false),
            (finalizer_identity, finalizer_only_role.as_str(), false),
            (lock_identity, intersection_role.as_str(), false),
            (finalizer_identity, intersection_role.as_str(), true),
            (lock_identity, grantable_role.as_str(), true),
            (finalizer_identity, grantable_role.as_str(), true),
            (lock_identity, mismatch_role.as_str(), true),
            (finalizer_identity, mismatch_role.as_str(), false),
        ] {
            sqlx::raw_sql(&format!(
                "GRANT EXECUTE ON FUNCTION {identity} TO {role}{}",
                if grantable { " WITH GRANT OPTION" } else { "" }
            ))
            .execute(&database.pool)
            .await?;
        }
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {finalizer_identity} TO PUBLIC"
        ))
        .execute(&database.pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_010)
            .unwrap();
        let error = run_migration_as(&database.pool, migration.sql.as_ref(), &migration_role)
            .await
            .expect_err("lock-only authority must block the upgrade");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(
            database_error.message(),
            "product apply lock grantee lacks explicit finalizer authorization"
        );
        let rolled_back = sqlx::query_as::<_, (i64, Option<i64>, i64, i64)>(
            "SELECT lock.oid::BIGINT, core.oid::BIGINT, finalizer.oid::BIGINT, \
              lock.proowner::BIGINT \
             FROM pg_catalog.pg_proc AS lock \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             LEFT JOIN pg_catalog.pg_proc AS core \
              ON core.oid = pg_catalog.to_regprocedure($2) \
             WHERE lock.oid = pg_catalog.to_regprocedure($1) \
              AND finalizer.oid = pg_catalog.to_regprocedure($3)",
        )
        .bind(lock_identity)
        .bind(core_identity)
        .bind(finalizer_identity)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(rolled_back, (original.0, None, original.1, original.2));
        assert!(public_function_execute(&database.pool, finalizer_identity).await);
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {finalizer_identity} TO {lock_only_role}"
        ))
        .execute(&database.pool)
        .await?;
        let error = run_migration_as(&database.pool, migration.sql.as_ref(), &migration_role)
            .await
            .expect_err("inconsistent grant option must block the upgrade");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(
            database_error.message(),
            "product apply grant option lacks explicit finalizer authorization"
        );
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {finalizer_identity} TO {mismatch_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await?;
        run_migration_as(&database.pool, migration.sql.as_ref(), &migration_role).await?;
        assert!(function_execute(&database.pool, &intersection_role, lock_identity).await);
        assert!(!function_grantable(&database.pool, &intersection_role, lock_identity).await);
        assert!(function_execute(&database.pool, &grantable_role, lock_identity).await);
        assert!(function_grantable(&database.pool, &grantable_role, lock_identity).await);
        assert!(function_execute(&database.pool, &mismatch_role, lock_identity).await);
        assert!(function_grantable(&database.pool, &mismatch_role, lock_identity).await);
        assert!(function_execute(&database.pool, &finalizer_only_role, finalizer_identity).await);
        assert!(!function_execute(&database.pool, &finalizer_only_role, lock_identity).await);
        assert!(!function_execute(&database.pool, &finalizer_only_role, core_identity).await);
        assert!(!public_function_execute(&database.pool, lock_identity).await);
        assert!(!public_function_execute(&database.pool, core_identity).await);
        assert!(!public_function_execute(&database.pool, finalizer_identity).await);
        let upgraded = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64)>(
            "SELECT wrapper.oid::BIGINT, core.oid::BIGINT, finalizer.oid::BIGINT, \
              wrapper.proowner::BIGINT, core.proowner::BIGINT, finalizer.proowner::BIGINT \
             FROM pg_catalog.pg_proc AS wrapper \
             CROSS JOIN pg_catalog.pg_proc AS core \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             WHERE wrapper.oid = pg_catalog.to_regprocedure($1) \
              AND core.oid = pg_catalog.to_regprocedure($2) \
              AND finalizer.oid = pg_catalog.to_regprocedure($3)",
        )
        .bind(lock_identity)
        .bind(core_identity)
        .bind(finalizer_identity)
        .fetch_one(&database.pool)
        .await?;
        assert_ne!(upgraded.0, original.0);
        assert_eq!(upgraded.1, original.0);
        assert_eq!(upgraded.2, original.1);
        assert_eq!(
            (upgraded.3, upgraded.4, upgraded.5),
            (original.2, original.2, original.2)
        );
        sqlx::raw_sql(&format!(
            "GRANT EXECUTE ON FUNCTION {core_identity} TO {post_lock_only_role}"
        ))
        .execute(&database.pool)
        .await?;
        assert!(function_execute(&database.pool, &post_lock_only_role, core_identity).await);
        assert!(!function_execute(&database.pool, &post_lock_only_role, lock_identity).await);
        let fixture = seed_fixture(&database.pool).await;
        let before = sqlx::query_as::<_, (String, i64)>(
            "SELECT state, product_revision \
             FROM public.activation_requests \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&database.pool)
        .await?;
        let operation = Operation::new("acl-core-only-denied");
        let mut transaction = begin_serializable(&database.pool).await;
        sqlx::query(&format!("SET LOCAL ROLE {post_lock_only_role}"))
            .execute(&mut *transaction)
            .await?;
        let error = lock_apply(
            &mut transaction,
            &fixture,
            &operation,
            &Call::valid(&fixture),
        )
        .await
        .expect_err("core-only role must not execute the terminal wrapper");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("42501"));
        transaction.rollback().await?;
        let after = sqlx::query_as::<_, (String, i64)>(
            "SELECT state, product_revision \
             FROM public.activation_requests \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(after, before);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
    sqlx::query(&format!("REVOKE {quoted_owner} FROM {migration_role}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    for role in roles {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut administrator)
            .await
            .unwrap();
    }
    outcome.unwrap();
}
