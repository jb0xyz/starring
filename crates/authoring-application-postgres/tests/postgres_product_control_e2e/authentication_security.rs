#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_authentication_is_exactly_scoped_for_a_non_owner_role() {
    let mut database = isolated_product_control_database("authentication_acl").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let fixture = seed_fixture(&database.pool).await;
    let role_suffix = suffix();
    let owner_role = format!("starring_auth_owner_{role_suffix}");
    let api_role = format!("starring_auth_api_{role_suffix}");
    let denied_role = format!("starring_auth_denied_{role_suffix}");
    let api_password = database_role_password();
    let denied_password = database_role_password();
    for role in [&owner_role, &api_role, &denied_role] {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_')
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
        (&api_role, &api_password),
        (&denied_role, &denied_password),
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
    for relation in ["product_principals", "product_auth_sessions"] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for function in [
        "public.starring_product_session_read_v1(BYTEA)",
        "public.starring_product_session_mutation_read_v1(BYTEA)",
        "public.starring_product_session_touch_v1(BYTEA, TIMESTAMPTZ, TIMESTAMPTZ, TIMESTAMPTZ, DOUBLE PRECISION)",
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
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {api_role}, {denied_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner_role}, {api_role}, {denied_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for function in [
        "public.starring_product_session_read_v1(BYTEA)",
        "public.starring_product_session_mutation_read_v1(BYTEA)",
        "public.starring_product_session_touch_v1(BYTEA, TIMESTAMPTZ, TIMESTAMPTZ, TIMESTAMPTZ, DOUBLE PRECISION)",
    ] {
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    let api_pool = database_role_login_pool(&database.name, &api_role, &api_password).await;
    let denied_pool =
        database_role_login_pool(&database.name, &denied_role, &denied_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let role_identity = sqlx::query_as::<_, (String, String)>(
            "SELECT current_user::TEXT, session_user::TEXT",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(role_identity, (api_role.clone(), api_role.clone()));

        let authentication = PostgresAuthentication::with_config(
            api_pool.clone(),
            PostgresAuthenticationConfig::new(
                Duration::from_secs(30 * 60),
                Duration::from_secs(1),
                Duration::from_secs(2),
            )
            .unwrap(),
        );
        authentication.verify_readiness().await.unwrap();
        authentication
            .authenticate(&fixture.credential)
            .await
            .unwrap();

        let last_seen_before_wrong = sqlx::query_scalar::<_, DateTime<Utc>>(
            "SELECT last_seen_at FROM public.product_auth_sessions \
             WHERE session_digest = $1",
        )
        .bind(fixture.session_digest.as_slice())
        .fetch_one(&database.pool)
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(1_050)).await;
        let wrong_csrf = URL_SAFE_NO_PAD.encode(Sha256::digest(format!(
            "wrong-csrf:{}",
            suffix()
        )));
        assert_eq!(
            authentication
                .authenticate_mutation(&fixture.credential, &wrong_csrf)
                .await,
            Err(AuthenticationError::InvalidCsrf)
        );
        let last_seen_after_wrong = sqlx::query_scalar::<_, DateTime<Utc>>(
            "SELECT last_seen_at FROM public.product_auth_sessions \
             WHERE session_digest = $1",
        )
        .bind(fixture.session_digest.as_slice())
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(last_seen_after_wrong, last_seen_before_wrong);
        authentication
            .authenticate_mutation(&fixture.credential, &fixture.csrf)
            .await
            .unwrap();
        let touched = sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)>(
            "SELECT last_seen_at, idle_expires_at, absolute_expires_at \
             FROM public.product_auth_sessions WHERE session_digest = $1",
        )
        .bind(fixture.session_digest.as_slice())
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(touched.0 > last_seen_before_wrong);
        assert!(touched.0 < touched.1 && touched.1 <= touched.2);
        assert!(touched.1 <= touched.0 + TimeDelta::minutes(30));

        let abusive_touch = sqlx::query_scalar::<_, i64>(
            "SELECT public.starring_product_session_touch_v1( \
             $1, $2, $3, $4, $5)",
        )
        .bind(fixture.session_digest.as_slice())
        .bind(touched.0)
        .bind(touched.1)
        .bind(touched.2)
        .bind(f64::MIN_POSITIVE)
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(abusive_touch, 0);
        let after_abusive_touch =
            sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT last_seen_at, idle_expires_at, absolute_expires_at \
                 FROM public.product_auth_sessions WHERE session_digest = $1",
            )
            .bind(fixture.session_digest.as_slice())
            .fetch_one(&database.pool)
            .await
            .unwrap();
        assert_eq!(after_abusive_touch, touched);

        let invalid_credential = URL_SAFE_NO_PAD.encode(Sha256::digest(format!(
            "invalid-session:{}",
            suffix()
        )));
        assert_eq!(
            authentication.authenticate(&invalid_credential).await,
            Err(AuthenticationError::InvalidCredential)
        );

        assert_database_permission_denied(
            &api_pool,
            "SELECT session_digest FROM public.product_auth_sessions",
        )
        .await;
        assert_database_permission_denied(
            &api_pool,
            "UPDATE public.product_auth_sessions SET last_seen_at = last_seen_at",
        )
        .await;
        assert_database_permission_denied(
            &api_pool,
            "CREATE TABLE public.authentication_escape(value INTEGER)",
        )
        .await;
        assert_database_permission_denied(
            &api_pool,
            "CREATE TEMPORARY TABLE authentication_escape(value INTEGER)",
        )
        .await;
        assert_database_permission_denied(
            &api_pool,
            "SELECT * FROM public.starring_product_installation_authority_read_v1( \
             'missing', 'missing', decode(repeat('00', 32), 'hex'))",
        )
        .await;
        assert_database_permission_denied(
            &denied_pool,
            "SELECT * FROM public.starring_product_session_read_v1( \
             decode(repeat('00', 32), 'hex'))",
        )
        .await;

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION \
             public.starring_product_session_read_v1(BYTEA) FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::CapabilityMissing)
        );
        let unavailable = authentication
            .authenticate(&fixture.credential)
            .await
            .unwrap_err();
        assert!(matches!(
            unavailable,
            AuthenticationError::Backend(
                authoring_application::AuthenticationBackendFailureV1::Unavailable
            )
        ));
        let redacted = format!("{unavailable:?}");
        assert!(!redacted.contains(&api_role));
        assert!(!redacted.contains(&fixture.credential));
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_product_session_read_v1(BYTEA) TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(session_digest) ON public.product_auth_sessions TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(session_digest) ON public.product_auth_sessions FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_product_session_mutation_read_v1(BYTEA) TO {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION \
             public.starring_product_session_mutation_read_v1(BYTEA) FROM {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION public.starring_product_session_read_v1(BYTEA) \
             TO {api_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE GRANT OPTION FOR EXECUTE ON FUNCTION \
             public.starring_product_session_read_v1(BYTEA) FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(
            "GRANT EXECUTE ON FUNCTION public.starring_product_session_read_v1(BYTEA) \
             TO PUBLIC",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "REVOKE EXECUTE ON FUNCTION public.starring_product_session_read_v1(BYTEA) \
             FROM PUBLIC",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(
            "ALTER FUNCTION public.starring_product_session_mutation_read_v1(BYTEA) \
             SECURITY INVOKER",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "ALTER FUNCTION public.starring_product_session_mutation_read_v1(BYTEA) \
             SECURITY DEFINER",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(
            "ALTER FUNCTION public.starring_product_session_read_v1(BYTEA) \
             SET search_path = public",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "ALTER FUNCTION public.starring_product_session_read_v1(BYTEA) \
             SET search_path = pg_catalog",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query("ALTER TABLE public.product_principals ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ContractMismatch)
        );
        sqlx::query("ALTER TABLE public.product_principals DISABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!(
            "REVOKE USAGE ON SCHEMA public FROM {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA public TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!("GRANT {denied_role} TO {api_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!("REVOKE {denied_role} FROM {api_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(&format!("GRANT {api_role} TO {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            authentication.verify_readiness().await,
            Err(AuthenticationReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!("REVOKE {api_role} FROM {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();

        authentication.verify_readiness().await.unwrap();
    })
    .catch_unwind()
    .await;
    api_pool.close().await;
    denied_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    for statement in [
        format!("REVOKE {owner_role} FROM {api_role}, {denied_role}"),
        format!("REVOKE {api_role} FROM {owner_role}, {denied_role}"),
        format!("REVOKE {denied_role} FROM {owner_role}, {api_role}"),
    ] {
        let _ = sqlx::query(&statement)
            .execute(&mut database.administrator)
            .await;
    }
    for role in [&denied_role, &api_role, &owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
