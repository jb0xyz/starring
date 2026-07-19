struct IdentityBoundarySession {
    credential: String,
    csrf: String,
    principal_id: String,
}

async fn assert_identity_boundary_excess(
    store: &authoring_application_postgres::PostgresProductIdentityStore,
) {
    let expected =
        Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ExcessCapability);
    assert_eq!(store.verify_oauth_flow_writer_readiness().await, expected);
    assert_eq!(store.verify_session_issuer_readiness().await, expected);
    assert_eq!(store.verify_session_api_readiness().await, expected);
    assert_eq!(store.verify_security_revoker_readiness().await, expected);
    assert_eq!(store.verify_readiness().await, expected);
}

fn identity_boundary_credential(excluded: &[[u8; 32]]) -> (String, [u8; 32]) {
    loop {
        let mut material = [0_u8; 32];
        getrandom::fill(&mut material).unwrap();
        let credential = URL_SAFE_NO_PAD.encode(material);
        let digest = digest_opaque_session_credential_v1(&credential)
            .unwrap()
            .into_bytes();
        if !excluded.contains(&digest) {
            return (credential, digest);
        }
    }
}

async fn issue_identity_boundary_session(
    store: &authoring_application_postgres::PostgresProductIdentityStore,
    issuer_pool: &PgPool,
    discord_user_id: &str,
) -> IdentityBoundarySession {
    let flow = store.create_oauth_flow("/app").await.unwrap();
    let state = flow.state().expose_secret().to_string();
    let browser_nonce = flow.browser_nonce().expose_secret().to_string();
    let state_digest = digest_opaque_session_credential_v1(&state)
        .unwrap()
        .into_bytes();
    let consumed = store
        .consume_oauth_flow(&state, &browser_nonce)
        .await
        .unwrap();
    assert_eq!(consumed.redirect_uri(), flow.redirect_uri());
    assert_eq!(consumed.return_path(), "/app");
    let (credential, session_digest) = identity_boundary_credential(&[state_digest]);
    let (csrf, csrf_digest) = identity_boundary_credential(&[state_digest, session_digest]);
    let issue_query = "SELECT outcome_code, principal_id, identity_revision \
         FROM public.starring_product_session_issue_v1(\
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";
    let row = sqlx::query_as::<_, (String, Option<String>, Option<i64>)>(issue_query)
        .bind(state_digest.as_slice())
        .bind(consumed.redirect_uri())
        .bind(consumed.return_path())
        .bind(consumed.consumed_at())
        .bind(discord_user_id)
        .bind("Identity Boundary")
        .bind(session_digest.as_slice())
        .bind(csrf_digest.as_slice())
        .bind(1_800_f64)
        .bind(43_200_f64)
        .fetch_one(issuer_pool)
        .await
        .unwrap();
    let principal_id = format!("discord:{discord_user_id}");
    assert_eq!(row.0, "issued");
    assert_eq!(row.1.as_deref(), Some(principal_id.as_str()));
    let identity_revision = row.2.unwrap();
    let replay = sqlx::query_as::<_, (String, Option<String>, Option<i64>)>(issue_query)
        .bind(state_digest.as_slice())
        .bind(consumed.redirect_uri())
        .bind(consumed.return_path())
        .bind(consumed.consumed_at())
        .bind(discord_user_id)
        .bind("Identity Boundary")
        .bind(session_digest.as_slice())
        .bind(csrf_digest.as_slice())
        .bind(1_800_f64)
        .bind(43_200_f64)
        .fetch_one(issuer_pool)
        .await
        .unwrap();
    assert_eq!(replay.0, "exact_replay");
    assert_eq!(replay.1.as_deref(), Some(principal_id.as_str()));
    assert_eq!(replay.2, Some(identity_revision));
    let (_, conflicting_session_digest) =
        identity_boundary_credential(&[state_digest, session_digest, csrf_digest]);
    let (_, conflicting_csrf_digest) = identity_boundary_credential(&[
        state_digest,
        session_digest,
        csrf_digest,
        conflicting_session_digest,
    ]);
    let conflict = sqlx::query_scalar::<_, String>(
        "SELECT outcome_code FROM public.starring_product_session_issue_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(state_digest.as_slice())
    .bind(consumed.redirect_uri())
    .bind(consumed.return_path())
    .bind(consumed.consumed_at())
    .bind(discord_user_id)
    .bind("Identity Boundary")
    .bind(conflicting_session_digest.as_slice())
    .bind(conflicting_csrf_digest.as_slice())
    .bind(1_800_f64)
    .bind(43_200_f64)
    .fetch_one(issuer_pool)
    .await
    .unwrap();
    assert_eq!(conflict, "flow_invalid_or_consumed");
    IdentityBoundarySession {
        credential,
        csrf,
        principal_id,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_identity_lifecycle_is_exactly_scoped_across_four_login_roles() {
    let mut database = isolated_product_control_database("identity_acl").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let role_suffix = suffix();
    let owner_role = format!("starring_identity_owner_{role_suffix}");
    let oauth_role = format!("starring_identity_oauth_{role_suffix}");
    let issuer_role = format!("starring_identity_issuer_{role_suffix}");
    let session_role = format!("starring_identity_session_{role_suffix}");
    let security_role = format!("starring_identity_security_{role_suffix}");
    let hostile_role = format!("starring_identity_hostile_{role_suffix}");
    let oauth_password = database_role_password();
    let issuer_password = database_role_password();
    let session_password = database_role_password();
    let security_password = database_role_password();
    for role in [
        &owner_role,
        &oauth_role,
        &issuer_role,
        &session_role,
        &security_role,
        &hostile_role,
    ] {
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
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for (role, password) in [
        (&oauth_role, &oauth_password),
        (&issuer_role, &issuer_password),
        (&session_role, &session_password),
        (&security_role, &security_password),
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
        "product_oauth_flows",
        "product_principals",
        "product_auth_sessions",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    let authentication_functions = [
        "public.starring_product_session_read_v1(bytea)",
        "public.starring_product_session_mutation_read_v1(bytea)",
        "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
    ];
    let oauth_functions = [
        "public.starring_product_oauth_database_identity_v1()",
        "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
        "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
    ];
    let issuer_functions = [
        "public.starring_product_session_issuer_database_identity_v1()",
        "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
    ];
    let session_functions = [
        "public.starring_product_session_api_database_identity_v1()",
        "public.starring_product_session_read_v1(bytea)",
        "public.starring_product_session_mutation_read_v1(bytea)",
        "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
        "public.starring_product_session_logout_read_v1(bytea)",
        "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
    ];
    let security_functions = [
        "public.starring_product_security_revoker_database_identity_v1()",
        "public.starring_product_session_security_revoke_v1(bytea)",
    ];
    let identity_support_functions = [
        "public.starring_product_oauth_database_identity_v1()",
        "public.starring_product_session_issuer_database_identity_v1()",
        "public.starring_product_session_api_database_identity_v1()",
        "public.starring_product_security_revoker_database_identity_v1()",
        "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
        "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
        "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
        "public.starring_product_session_logout_read_v1(bytea)",
        "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
        "public.starring_product_session_security_revoke_v1(bytea)",
        "public.enforce_product_principal_transition()",
        "public.enforce_product_oauth_flow_transition()",
        "public.enforce_product_auth_session_oauth_binding()",
        "public.enforce_product_auth_session_transition()",
        "public.starring_purge_product_identity_v1(integer)",
    ];
    for function in authentication_functions
        .iter()
        .chain(identity_support_functions.iter())
    {
        sqlx::query(&format!("ALTER FUNCTION {function} OWNER TO {owner_role}"))
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
        "GRANT CONNECT ON DATABASE {} TO {oauth_role}, {issuer_role}, \
         {session_role}, {security_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner_role}, {oauth_role}, {issuer_role}, \
         {session_role}, {security_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for (role, functions) in [
        (&oauth_role, oauth_functions.as_slice()),
        (&issuer_role, issuer_functions.as_slice()),
        (&session_role, session_functions.as_slice()),
        (&security_role, security_functions.as_slice()),
    ] {
        for function in functions {
            sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {function} TO {role}"))
                .execute(&database.pool)
                .await
                .unwrap();
        }
    }
    let oauth_pool = database_role_login_pool(&database.name, &oauth_role, &oauth_password).await;
    let issuer_pool =
        database_role_login_pool(&database.name, &issuer_role, &issuer_password).await;
    let session_pool =
        database_role_login_pool(&database.name, &session_role, &session_password).await;
    let security_pool =
        database_role_login_pool(&database.name, &security_role, &security_password).await;
    let mut mixed_database = isolated_product_control_database("identity_mix").await;
    MIGRATOR.run(&mixed_database.pool).await.unwrap();
    let mixed_role_suffix = suffix();
    let mixed_owner_role = format!("starring_identity_mixed_owner_{mixed_role_suffix}");
    let mixed_security_role = format!("starring_identity_mixed_security_{mixed_role_suffix}");
    let mixed_security_password = database_role_password();
    for role in [&mixed_owner_role, &mixed_security_role] {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
    }
    sqlx::query(&format!(
        "CREATE ROLE {mixed_owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    let mixed_password_literal =
        sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
            .bind(&mixed_security_password)
            .fetch_one(&mixed_database.pool)
            .await
            .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {mixed_security_role} LOGIN PASSWORD {mixed_password_literal} \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
         NOBYPASSRLS CONNECTION LIMIT 4"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    for relation in [
        "product_control_plane_identity",
        "product_oauth_flows",
        "product_principals",
        "product_auth_sessions",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {mixed_owner_role}"
        ))
        .execute(&mixed_database.pool)
        .await
        .unwrap();
    }
    for function in &security_functions {
        sqlx::query(&format!(
            "ALTER FUNCTION {function} OWNER TO {mixed_owner_role}"
        ))
        .execute(&mixed_database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "REVOKE ALL ON DATABASE {} FROM PUBLIC",
        mixed_database.name
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
        .execute(&mixed_database.pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {mixed_security_role}",
        mixed_database.name
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {mixed_owner_role}, {mixed_security_role}"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    for function in &security_functions {
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {mixed_security_role}"
        ))
        .execute(&mixed_database.pool)
        .await
        .unwrap();
    }
    let mixed_security_pool = database_role_login_pool(
        &mixed_database.name,
        &mixed_security_role,
        &mixed_security_password,
    )
    .await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let config = authoring_application_postgres::PostgresProductIdentityConfig::production(
            "https://starring.example/oauth/discord/callback",
            ["/".to_string(), "/app".to_string()],
        )
        .unwrap();
        let store = authoring_application_postgres::PostgresProductIdentityStore::production(
            authoring_application_postgres::ProductIdentityDatabasePoolsV1::new(
                oauth_pool.clone(),
                issuer_pool.clone(),
                session_pool.clone(),
                security_pool.clone(),
            ),
            config,
        );
        store.verify_oauth_flow_writer_readiness().await.unwrap();
        store.verify_session_issuer_readiness().await.unwrap();
        store.verify_session_api_readiness().await.unwrap();
        store.verify_security_revoker_readiness().await.unwrap();
        store.verify_readiness().await.unwrap();

        let hostile_memberships = sqlx::query_scalar::<_, i64>(&format!(
            "SELECT pg_catalog.count(*) FROM pg_catalog.pg_auth_members AS membership \
             WHERE membership.member = (SELECT role.oid FROM pg_catalog.pg_roles AS role \
              WHERE role.rolname = '{hostile_role}') \
              OR membership.roleid = (SELECT role.oid FROM pg_catalog.pg_roles AS role \
               WHERE role.rolname = '{hostile_role}')"
        ))
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(hostile_memberships, 0);
        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.product_oauth_flows TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_identity_boundary_excess(&store).await;
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE public.product_oauth_flows FROM {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        store.verify_readiness().await.unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(principal_id) ON TABLE public.product_principals TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_identity_boundary_excess(&store).await;
        sqlx::query(&format!(
            "REVOKE SELECT(principal_id) ON TABLE public.product_principals FROM {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        store.verify_readiness().await.unwrap();

        let mixed_config =
            authoring_application_postgres::PostgresProductIdentityConfig::production(
                "https://starring.example/oauth/discord/callback",
                ["/".to_string(), "/app".to_string()],
            )
            .unwrap();
        let mixed_store =
            authoring_application_postgres::PostgresProductIdentityStore::production(
                authoring_application_postgres::ProductIdentityDatabasePoolsV1::new(
                    oauth_pool.clone(),
                    issuer_pool.clone(),
                    session_pool.clone(),
                    mixed_security_pool.clone(),
                ),
                mixed_config,
            );
        mixed_store
            .verify_oauth_flow_writer_readiness()
            .await
            .unwrap();
        mixed_store.verify_session_issuer_readiness().await.unwrap();
        mixed_store.verify_session_api_readiness().await.unwrap();
        mixed_store
            .verify_security_revoker_readiness()
            .await
            .unwrap();
        assert_eq!(
            mixed_store.verify_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ContractMismatch)
        );

        let first_user = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        )
        .unwrap()
        .to_string();
        let first = issue_identity_boundary_session(&store, &issuer_pool, &first_user).await;
        store
            .authentication()
            .authenticate(&first.credential)
            .await
            .unwrap();
        let current = store.current_principal(&first.credential).await.unwrap();
        assert_eq!(current.principal_id().as_str(), first.principal_id);
        assert_eq!(current.display_name(), "Identity Boundary");
        assert_eq!(
            store.logout(&first.credential, &first.csrf).await,
            Ok(authoring_application_postgres::ProductLogoutDispositionV1::Revoked)
        );
        assert_eq!(
            store.logout(&first.credential, &first.csrf).await,
            Ok(authoring_application_postgres::ProductLogoutDispositionV1::ExactReplay)
        );

        let second_user = first_user.parse::<u64>().unwrap().wrapping_add(1).to_string();
        let second = issue_identity_boundary_session(&store, &issuer_pool, &second_user).await;
        store.current_principal(&second.credential).await.unwrap();
        assert_eq!(
            store
                .revoke_session(
                    &second.credential,
                    authoring_application_postgres::ProductSessionRevocationReasonV1::SecurityRevocation,
                )
                .await,
            Ok(authoring_application_postgres::ProductLogoutDispositionV1::Revoked)
        );
        assert_eq!(
            store.current_principal(&second.credential).await,
            Err(authoring_application_postgres::ProductIdentityError::Revoked)
        );

        let invalid_digest = "decode(repeat('00', 31), 'hex')";
        let oauth_call = format!(
            "SELECT * FROM public.starring_product_oauth_flow_create_v1(\
             {invalid_digest}, {invalid_digest}, 'invalid', 'invalid', 0)"
        );
        let issuer_call = format!(
            "SELECT * FROM public.starring_product_session_issue_v1(\
             {invalid_digest}, 'invalid', '/', TIMESTAMPTZ '2000-01-01T00:00:00Z', \
             '1', 'x', {invalid_digest}, {invalid_digest}, 1, 1)"
        );
        let session_call = format!(
            "SELECT * FROM public.starring_product_session_read_v1({invalid_digest})"
        );
        let security_call = format!(
            "SELECT * FROM public.starring_product_session_security_revoke_v1({invalid_digest})"
        );
        for (role_pool, denied_calls) in [
            (
                &oauth_pool,
                [issuer_call.as_str(), session_call.as_str(), security_call.as_str()],
            ),
            (
                &issuer_pool,
                [oauth_call.as_str(), session_call.as_str(), security_call.as_str()],
            ),
            (
                &session_pool,
                [oauth_call.as_str(), issuer_call.as_str(), security_call.as_str()],
            ),
            (
                &security_pool,
                [oauth_call.as_str(), issuer_call.as_str(), session_call.as_str()],
            ),
        ] {
            for statement in denied_calls {
                assert_database_permission_denied(role_pool, statement).await;
            }
        }
        let ddl_suffix = suffix();
        for role_pool in [&oauth_pool, &issuer_pool, &session_pool, &security_pool] {
            for statement in [
                "SELECT database_identity FROM public.product_control_plane_identity",
                "SELECT state_digest FROM public.product_oauth_flows",
                "INSERT INTO public.product_principals (principal_id) VALUES ('denied')",
                "UPDATE public.product_auth_sessions SET last_seen_at = last_seen_at",
                "DELETE FROM public.product_auth_sessions",
                "SELECT * FROM public.starring_purge_product_identity_v1(1)",
                "SELECT public.enforce_product_principal_transition()",
            ] {
                assert_database_permission_denied(role_pool, statement).await;
            }
            assert_database_permission_denied(
                role_pool,
                &format!("CREATE TABLE public.identity_escape_{ddl_suffix}(value INTEGER)"),
            )
            .await;
            assert_database_permission_denied(
                role_pool,
                &format!("CREATE TEMPORARY TABLE identity_escape_{ddl_suffix}(value INTEGER)"),
            )
            .await;
            assert_database_permission_denied(
                role_pool,
                &format!("CREATE SCHEMA identity_escape_{ddl_suffix}"),
            )
            .await;
        }

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {} FROM {oauth_role}",
            oauth_functions[0]
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            store.verify_oauth_flow_writer_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::CapabilityMissing)
        );
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {} TO {oauth_role}",
            oauth_functions[0]
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(state_digest) ON public.product_oauth_flows TO {issuer_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            store.verify_session_issuer_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(state_digest) ON public.product_oauth_flows FROM {issuer_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(session_digest) ON public.product_auth_sessions TO {oauth_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            store.verify_oauth_flow_writer_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(session_digest) ON public.product_auth_sessions FROM {oauth_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(state_digest) ON public.product_oauth_flows TO {session_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            store.verify_session_api_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(state_digest) ON public.product_oauth_flows FROM {session_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(principal_id) ON public.product_principals TO {security_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            store.verify_security_revoker_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(principal_id) ON public.product_principals FROM {security_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        store.verify_oauth_flow_writer_readiness().await.unwrap();
        store.verify_session_issuer_readiness().await.unwrap();
        store.verify_session_api_readiness().await.unwrap();
        store.verify_security_revoker_readiness().await.unwrap();
        store.verify_readiness().await.unwrap();
    })
    .catch_unwind()
    .await;
    oauth_pool.close().await;
    issuer_pool.close().await;
    session_pool.close().await;
    security_pool.close().await;
    mixed_security_pool.close().await;
    database.pool.close().await;
    mixed_database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        mixed_database.name
    ))
    .execute(&mut mixed_database.administrator)
    .await
    .unwrap();
    for role in [
        &security_role,
        &session_role,
        &issuer_role,
        &oauth_role,
        &hostile_role,
        &owner_role,
    ] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    for role in [&mixed_security_role, &mixed_owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut mixed_database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
