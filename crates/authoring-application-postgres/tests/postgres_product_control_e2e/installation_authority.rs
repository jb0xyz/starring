#[derive(Clone, Copy)]
enum AuthorityInvalidation {
    RevokeSession,
    DisablePrincipal,
    SuspendTenant,
    SuspendInstallation,
}

struct InvalidatingAuthentication {
    inner: PostgresAuthentication,
    pool: PgPool,
    fixture: Fixture,
    invalidation: AuthorityInvalidation,
}

struct ClaimsAuthentication {
    claims: authoring_application::AuthenticationClaimsV1,
}

impl authoring_application::AuthenticationPort for ClaimsAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        _credential: &Self::Credential,
    ) -> Result<authoring_application::AuthenticationClaimsV1, AuthenticationError> {
        Ok(self.claims.clone())
    }
}

impl authoring_application::AuthenticationPort for InvalidatingAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<authoring_application::AuthenticationClaimsV1, AuthenticationError> {
        let claims = self.inner.authenticate(credential).await?;
        let result = match self.invalidation {
            AuthorityInvalidation::RevokeSession => sqlx::query(
                "UPDATE public.product_auth_sessions \
                 SET revoked_at = pg_catalog.clock_timestamp(), \
                  revocation_reason = 'authority_revalidation' \
                 WHERE session_digest = $1 AND revoked_at IS NULL",
            )
            .bind(self.fixture.session_digest.as_slice())
            .execute(&self.pool)
            .await,
            AuthorityInvalidation::DisablePrincipal => sqlx::query(
                "UPDATE public.product_principals \
                 SET disabled = TRUE, identity_revision = identity_revision + 1, \
                  updated_at = GREATEST( \
                   pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
                 WHERE principal_id = $1 AND disabled = FALSE",
            )
            .bind(self.fixture.approver_principal.as_str())
            .execute(&self.pool)
            .await,
            AuthorityInvalidation::SuspendTenant => sqlx::query(
                "UPDATE public.product_tenants \
                 SET lifecycle_state = 'suspended', \
                  updated_at = GREATEST( \
                   pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
                 WHERE tenant_id = $1 AND lifecycle_state = 'active'",
            )
            .bind(self.fixture.tenant_id.as_str())
            .execute(&self.pool)
            .await,
            AuthorityInvalidation::SuspendInstallation => sqlx::query(
                "UPDATE public.automation_installations \
                 SET lifecycle_state = 'suspended', \
                  updated_at = GREATEST( \
                   pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
                 WHERE tenant_id = $1 AND installation_id = $2 \
                  AND lifecycle_state = 'active'",
            )
            .bind(self.fixture.tenant_id.as_str())
            .bind(self.fixture.installation_id.as_str())
            .execute(&self.pool)
            .await,
        }
        .map_err(|_| {
            AuthenticationError::Backend(
                authoring_application::AuthenticationBackendFailureV1::Unavailable,
            )
        })?;
        if result.rows_affected() != 1 {
            return Err(AuthenticationError::Backend(
                authoring_application::AuthenticationBackendFailureV1::Unavailable,
            ));
        }
        Ok(claims)
    }
}

struct NeverDecisions;

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for NeverDecisions {
    async fn load_approval_preview(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        panic!("product decisions must not run without installation authority")
    }

    async fn load_product_status(
        &self,
        _request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        panic!("product decisions must not run without installation authority")
    }
}

struct CapturingPreviewDecisions {
    fixture: Fixture,
    calls: Arc<AtomicUsize>,
}

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for CapturingPreviewDecisions {
    async fn load_approval_preview(
        &self,
        request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        assert_eq!(request.actor().principal_id(), &self.fixture.approver_principal);
        assert_eq!(
            request.actor().session_fingerprint().as_bytes(),
            &self.fixture.session_digest
        );
        assert_eq!(request.scope().tenant_id(), &self.fixture.tenant_id);
        assert_eq!(
            request.scope().installation_id(),
            &self.fixture.installation_id
        );
        assert_eq!(request.scope().guild_id(), self.fixture.guild_id);
        assert_eq!(request.scope().acting_user_id(), self.fixture.approver_user);
        assert_eq!(request.evidence().tenant_id(), &self.fixture.tenant_id);
        assert_eq!(
            request.evidence().installation_id(),
            &self.fixture.installation_id
        );
        assert_eq!(request.evidence().application_id(), self.fixture.application_id);
        assert_eq!(request.evidence().guild_id(), self.fixture.guild_id);
        assert_eq!(
            request.evidence().acting_user_id(),
            self.fixture.approver_user
        );
        assert_eq!(request.evidence().capability(), CapabilityV1::Read);
        assert_eq!(
            request.evidence().installation_authority_revision(),
            self.fixture.authority_revision
        );
        assert_eq!(
            request.evidence().installation_authority_digest(),
            self.fixture.authority_digest
        );
        assert_eq!(
            request.promotion().promotion_id(),
            &self.fixture.promotion_id
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ProductApprovalPreviewV1::from_server_projection(
            self.fixture.installation_id.clone(),
            self.fixture.guild_id,
            self.fixture.payload.clone(),
            ApprovalPayloadDigestV1::parse(&self.fixture.payload_digest).unwrap(),
            ProductRevisionV1::new(1).unwrap(),
            ProductDecisionPhaseV1::PendingApproval,
        ))
    }

    async fn load_product_status(
        &self,
        _request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        panic!("status lookup is outside this authority projection test")
    }
}

async fn authority_shadow_search_path_pool(setup_pool: &PgPool, database_name: &str) -> PgPool {
    sqlx::query("CREATE SCHEMA IF NOT EXISTS installation_authority_shadow")
        .execute(setup_pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION installation_authority_shadow.clock_timestamp() \
         RETURNS TIMESTAMPTZ LANGUAGE SQL IMMUTABLE SET search_path = pg_catalog \
         AS 'SELECT ''2000-01-01T00:00:00Z''::TIMESTAMPTZ'",
    )
    .execute(setup_pool)
    .await
    .unwrap();
    PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET search_path = installation_authority_shadow, pg_catalog")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(
            database_url()
                .parse::<PgConnectOptions>()
                .unwrap()
                .database(database_name),
        )
        .await
        .unwrap()
}

fn postgres_authority_adapter(
    pool: PgPool,
    fixture: Fixture,
    client_calls: Arc<AtomicUsize>,
) -> DiscordGuildAuthorityAdapter<
    PostgresInstallationAuthoritySource,
    Client,
    SubmicrosecondClock,
> {
    DiscordGuildAuthorityAdapter::with_clock(
        PostgresInstallationAuthoritySource::new(pool),
        Client {
            fixture,
            calls: client_calls,
        },
        SubmicrosecondClock,
        DiscordAuthorityConfigV1::new(
            Duration::from_secs(2),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn installation_authority_projects_exact_rotated_head_under_hostile_search_path() {
    let database = isolated_product_control_database("authority_search").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let fixture = seed_fixture(&database.pool).await;
        let rotated = rotate_authority(&database.pool, &fixture, AuthorityRotation::Safe).await;
        let hostile_pool =
            authority_shadow_search_path_pool(&database.pool, &database.name).await;
        let client_calls = Arc::new(AtomicUsize::new(0));
        let decision_calls = Arc::new(AtomicUsize::new(0));
        let authentication = PostgresAuthentication::new(hostile_pool.clone());
        let authority = postgres_authority_adapter(
            hostile_pool.clone(),
            rotated.clone(),
            client_calls.clone(),
        );
        let decisions = CapturingPreviewDecisions {
            fixture: rotated.clone(),
            calls: decision_calls.clone(),
        };
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);

        let preview = application
            .get_approval_preview(
                &rotated.credential,
                &selector(&rotated),
                status_query(&rotated),
            )
            .await
            .unwrap();

        assert_eq!(preview.installation_id(), &rotated.installation_id);
        assert_eq!(preview.guild_id(), rotated.guild_id);
        assert_eq!(client_calls.load(Ordering::SeqCst), 1);
        assert_eq!(decision_calls.load(Ordering::SeqCst), 1);
    }
    drop_isolated_product_control_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn installation_authority_revalidation_is_non_enumerating_and_stops_downstream_work() {
    let pool = pool().await;
    let expected = ProductApplicationError::FreshAuthority(
        authoring_application::FreshGuildAuthorityError::InstallationNotFound,
    );
    let missing_fixture = seed_fixture(&pool).await;
    let missing_calls = Arc::new(AtomicUsize::new(0));
    let missing_authentication = PostgresAuthentication::new(pool.clone());
    let missing_authority = postgres_authority_adapter(
        pool.clone(),
        missing_fixture.clone(),
        missing_calls.clone(),
    );
    let decisions = NeverDecisions;
    let deployments = PendingDeployments;
    let missing_application = ProductControlApplication::new(
        &missing_authentication,
        &missing_authority,
        &decisions,
        &deployments,
    );
    let missing_installation = InstallationSelectorV1::new(
        AutomationInstallationId::parse(&format!("missing-installation-{}", suffix())).unwrap(),
    );
    let missing_error = missing_application
        .get_approval_preview(
            &missing_fixture.credential,
            &missing_installation,
            status_query(&missing_fixture),
        )
        .await
        .unwrap_err();
    assert_eq!(missing_error, expected);
    assert_eq!(missing_calls.load(Ordering::SeqCst), 0);

    for claims in [
        authoring_application::AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse(&format!("wrong-principal-{}", suffix())).unwrap(),
            authoring_application::AuthenticatedSessionFingerprintV1::from_sha256_digest(
                missing_fixture.session_digest,
            ),
        ),
        authoring_application::AuthenticationClaimsV1::from_authentication(
            missing_fixture.approver_principal.clone(),
            authoring_application::AuthenticatedSessionFingerprintV1::from_sha256_digest(
                [255_u8; 32],
            ),
        ),
    ] {
        let client_calls = Arc::new(AtomicUsize::new(0));
        let authentication = ClaimsAuthentication { claims };
        let authority = postgres_authority_adapter(
            pool.clone(),
            missing_fixture.clone(),
            client_calls.clone(),
        );
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let error = application
            .get_approval_preview(
                &missing_fixture.credential,
                &selector(&missing_fixture),
                status_query(&missing_fixture),
            )
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert_eq!(client_calls.load(Ordering::SeqCst), 0);
    }

    for invalidation in [
        AuthorityInvalidation::RevokeSession,
        AuthorityInvalidation::DisablePrincipal,
        AuthorityInvalidation::SuspendTenant,
        AuthorityInvalidation::SuspendInstallation,
    ] {
        let fixture = seed_fixture(&pool).await;
        let client_calls = Arc::new(AtomicUsize::new(0));
        let authentication = InvalidatingAuthentication {
            inner: PostgresAuthentication::new(pool.clone()),
            pool: pool.clone(),
            fixture: fixture.clone(),
            invalidation,
        };
        let authority = postgres_authority_adapter(
            pool.clone(),
            fixture.clone(),
            client_calls.clone(),
        );
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let error = application
            .get_approval_preview(&fixture.credential, &selector(&fixture), status_query(&fixture))
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert_eq!(client_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn installation_authority_database_wait_is_bounded_and_redacted() {
    let database = isolated_product_control_database("authority_timeout").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        let fixture = seed_fixture(pool).await;
        let mut blocker = pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE public.automation_installations IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let client_calls = Arc::new(AtomicUsize::new(0));
        let authentication = PostgresAuthentication::new(pool.clone());
        let source = PostgresInstallationAuthoritySource::with_config(
            pool.clone(),
            PostgresInstallationAuthoritySourceConfig::new(Duration::from_millis(25)).unwrap(),
        );
        let authority = DiscordGuildAuthorityAdapter::with_clock(
            source,
            Client {
                fixture: fixture.clone(),
                calls: client_calls.clone(),
            },
            SubmicrosecondClock,
            DiscordAuthorityConfigV1::new(
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(30),
            )
            .unwrap(),
        );
        let decisions = NeverDecisions;
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            application.get_approval_preview(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture),
            ),
        )
        .await;
        let elapsed = started.elapsed();
        blocker.rollback().await.unwrap();
        let error = result.expect("authority lookup exceeded its outer test deadline").unwrap_err();

        assert_eq!(
            error,
            ProductApplicationError::FreshAuthority(
                authoring_application::FreshGuildAuthorityError::Backend(
                    "installation_authority_unavailable".to_string()
                )
            )
        );
        assert!(elapsed < Duration::from_secs(2));
        assert_eq!(client_calls.load(Ordering::SeqCst), 0);
        let rendered = error.to_string();
        for sensitive in [
            fixture.tenant_id.as_str().to_string(),
            fixture.installation_id.as_str().to_string(),
            fixture.approver_principal.as_str().to_string(),
            fixture.approver_user.to_string(),
            fixture.application_id.to_string(),
            fixture.guild_id.to_string(),
            fixture.authority_digest.clone(),
            lower_hex(&fixture.session_digest),
            fixture.credential.clone(),
        ] {
            assert!(!rendered.contains(&sensitive));
        }
    }
    drop_isolated_product_control_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn installation_authority_rejects_a_missing_current_head_before_discord() {
    let database = isolated_product_control_database("authority_head").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        let fixture = seed_fixture(pool).await;
        sqlx::query("ALTER TABLE public.automation_installations DISABLE TRIGGER ALL")
            .execute(pool)
            .await
            .unwrap();
        let changed = sqlx::query(
            "UPDATE public.automation_installations \
             SET current_authority_revision = 2, \
              updated_at = pg_catalog.clock_timestamp() + INTERVAL '1 microsecond' \
             WHERE tenant_id = $1 AND installation_id = $2",
        )
        .bind(fixture.tenant_id.as_str())
        .bind(fixture.installation_id.as_str())
        .execute(pool)
        .await
        .unwrap();
        assert_eq!(changed.rows_affected(), 1);
        sqlx::query("ALTER TABLE public.automation_installations ENABLE TRIGGER ALL")
            .execute(pool)
            .await
            .unwrap();
        let client_calls = Arc::new(AtomicUsize::new(0));
        let authentication = PostgresAuthentication::new(pool.clone());
        let authority = postgres_authority_adapter(
            pool.clone(),
            fixture.clone(),
            client_calls.clone(),
        );
        let decisions = NeverDecisions;
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);

        let error = application
            .get_approval_preview(&fixture.credential, &selector(&fixture), status_query(&fixture))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            ProductApplicationError::FreshAuthority(
                authoring_application::FreshGuildAuthorityError::Backend(
                    "installation_authority_invalid".to_string()
                )
            )
        );
        assert_eq!(client_calls.load(Ordering::SeqCst), 0);
        assert!(!error.to_string().contains(fixture.installation_id.as_str()));
    }
    drop_isolated_product_control_database(database).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn installation_authority_rejects_a_missing_tenant_parent_before_discord() {
    let database = isolated_product_control_database("authority_tenant").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        let fixture = seed_fixture(pool).await;
        sqlx::query("ALTER TABLE public.product_tenants DISABLE TRIGGER ALL")
            .execute(pool)
            .await
            .unwrap();
        let deleted = sqlx::query(
            "DELETE FROM public.product_tenants WHERE tenant_id = $1",
        )
        .bind(fixture.tenant_id.as_str())
        .execute(pool)
        .await
        .unwrap();
        assert_eq!(deleted.rows_affected(), 1);
        sqlx::query("ALTER TABLE public.product_tenants ENABLE TRIGGER ALL")
            .execute(pool)
            .await
            .unwrap();
        let client_calls = Arc::new(AtomicUsize::new(0));
        let authentication = PostgresAuthentication::new(pool.clone());
        let authority = postgres_authority_adapter(
            pool.clone(),
            fixture.clone(),
            client_calls.clone(),
        );
        let decisions = NeverDecisions;
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);

        let error = application
            .get_approval_preview(&fixture.credential, &selector(&fixture), status_query(&fixture))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            ProductApplicationError::FreshAuthority(
                authoring_application::FreshGuildAuthorityError::Backend(
                    "installation_authority_invalid".to_string()
                )
            )
        );
        assert_eq!(client_calls.load(Ordering::SeqCst), 0);
        assert!(!error.to_string().contains(fixture.tenant_id.as_str()));
    }
    drop_isolated_product_control_database(database).await;
}
