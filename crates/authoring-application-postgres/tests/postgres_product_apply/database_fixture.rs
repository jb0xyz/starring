fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let options = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let database = options
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        database.starts_with("starring_")
            && database.split('_').any(|segment| segment == "test")
            && database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to use a database outside the strict Starring test namespace"
    );
    url
}

async fn pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

async fn isolated_database(label: &str) -> TestDatabase {
    let name = format!("starring_apply_{label}_test_{}", suffix());
    assert!(
        name.starts_with("starring_")
            && name.split('_').any(|segment| segment == "test")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let base = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    TestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_isolated_database(database: TestDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
}

fn suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
}

fn digest(seed: &str) -> String {
    let bytes = Sha256::digest(seed.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

#[derive(Clone)]
struct Actor {
    principal_id: String,
    user_id: String,
    session_digest: Vec<u8>,
    session_subject: Vec<u8>,
    csrf_digest: Vec<u8>,
    oauth_state: Vec<u8>,
    oauth_nonce: Vec<u8>,
}

impl Actor {
    fn new(suffix: &str, label: &str, user_id: String) -> Self {
        Self {
            principal_id: format!("{label}-{suffix}"),
            user_id,
            session_digest: Sha256::digest(format!("session:{label}:{suffix}")).to_vec(),
            session_subject: Sha256::digest(format!("subject:{label}:{suffix}")).to_vec(),
            csrf_digest: Sha256::digest(format!("csrf:{label}:{suffix}")).to_vec(),
            oauth_state: Sha256::digest(format!("oauth:{label}:{suffix}")).to_vec(),
            oauth_nonce: Sha256::digest(format!("nonce:{label}:{suffix}")).to_vec(),
        }
    }
}

#[derive(Clone)]
struct Fixture {
    tenant_id: String,
    installation_id: String,
    promotion_id: String,
    activation_id: String,
    actor: Actor,
    application_id: String,
    guild_id: String,
    ruleset_key: String,
    payload_digest: String,
    authority_digest: String,
    observation_digest: String,
}

#[derive(Clone)]
struct Operation {
    request_id: String,
    idempotency_digest: String,
    key_id: String,
    key_fingerprint: String,
    semantic_digest: String,
    receipt_id: String,
    audit_event_id: String,
    apply_attempt_id: String,
    deployment_id: String,
}

impl Operation {
    fn new(label: &str) -> Self {
        let suffix = suffix();
        Self {
            request_id: format!("apply-request-{label}-{suffix}"),
            idempotency_digest: digest(&format!("idempotency:{label}:{suffix}")),
            key_id: "apply-key-v1".to_string(),
            key_fingerprint: digest("apply-key-v1-material"),
            semantic_digest: digest(&format!("semantic:{label}:{suffix}")),
            receipt_id: digest(&format!("receipt:{label}:{suffix}")),
            audit_event_id: digest(&format!("audit:{label}:{suffix}")),
            apply_attempt_id: format!("apply_attempt_{suffix}"),
            deployment_id: format!("apply-deployment-{suffix}"),
        }
    }
}

#[derive(Clone)]
struct ApplyLockContext {
    expected_payload_digest: String,
    expected_authority_revision: i64,
    expected_authority_digest: String,
    active_idempotency_digest: String,
    idempotency_candidates: Vec<String>,
    candidate_key_ids: Vec<String>,
    candidate_key_fingerprints: Vec<String>,
    active_key_id: String,
}

impl ApplyLockContext {
    fn single(fixture: &Fixture, operation: &Operation) -> Self {
        Self {
            expected_payload_digest: fixture.payload_digest.clone(),
            expected_authority_revision: 1,
            expected_authority_digest: fixture.authority_digest.clone(),
            active_idempotency_digest: operation.idempotency_digest.clone(),
            idempotency_candidates: vec![operation.idempotency_digest.clone()],
            candidate_key_ids: vec![operation.key_id.clone()],
            candidate_key_fingerprints: vec![operation.key_fingerprint.clone()],
            active_key_id: operation.key_id.clone(),
        }
    }
}

#[derive(Clone)]
struct Call {
    expected_revision: i64,
    capability: String,
    session_digest: Vec<u8>,
    observed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    effective_permissions: String,
    guild_owner: bool,
}

impl Call {
    fn valid(fixture: &Fixture) -> Self {
        let observed_at = Utc::now() - TimeDelta::milliseconds(50);
        Self {
            expected_revision: 2,
            capability: "apply".to_string(),
            session_digest: fixture.actor.session_digest.clone(),
            observed_at,
            expires_at: observed_at + TimeDelta::seconds(5),
            effective_permissions: "32".to_string(),
            guild_owner: false,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LockRow {
    outcome: String,
    exact_replay: bool,
    requires_commit: bool,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
    locked_projection: Option<Json<Value>>,
}

#[derive(Debug, sqlx::FromRow)]
struct FinalizeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct TerminalPersistenceRow {
    state: String,
    product_revision: i64,
    apply_attempt_no: i64,
    apply_attempt_id: Option<String>,
    apply_lease_until: Option<DateTime<Utc>>,
    termination: Option<Json<Value>>,
    receipt_resulting_revision: Option<i64>,
    receipt_resulting_state: Option<String>,
    receipt_result_code: Option<String>,
    receipt_http_disposition_class: Option<i16>,
    receipt_completed_at: Option<DateTime<Utc>>,
    audit_authority_revision: Option<i64>,
    audit_binding_fingerprint: Option<String>,
    audit_policy_revision: Option<i64>,
    audit_baseline_version: Option<i64>,
    audit_baseline_hash: Option<String>,
    audit_occurred_at: Option<DateTime<Utc>>,
    runtime_count: i64,
    alias_count: i64,
    audit_count: i64,
    evidence_count: i64,
}

async fn begin_serializable(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction
}

async fn terminal_persistence(
    pool: &PgPool,
    fixture: &Fixture,
    operation: &Operation,
) -> TerminalPersistenceRow {
    sqlx::query_as::<_, TerminalPersistenceRow>(
        "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
          activation.apply_attempt_id, activation.apply_lease_until, activation.termination, \
          receipt.resulting_revision AS receipt_resulting_revision, \
          receipt.resulting_state AS receipt_resulting_state, \
          receipt.result_code AS receipt_result_code, \
          receipt.http_disposition_class AS receipt_http_disposition_class, \
          receipt.completed_at AS receipt_completed_at, \
          audit.installation_authority_revision AS audit_authority_revision, \
          audit.binding_fingerprint AS audit_binding_fingerprint, \
          audit.policy_revision AS audit_policy_revision, \
          audit.active_baseline_version AS audit_baseline_version, \
          audit.active_baseline_hash AS audit_baseline_hash, \
          audit.occurred_at AS audit_occurred_at, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id) AS runtime_count, \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_idempotency_aliases AS alias \
           WHERE alias.receipt_id = $2) AS alias_count, \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events AS event \
           WHERE event.receipt_id = $2) AS audit_count, \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $2) AS evidence_count \
         FROM public.activation_requests AS activation \
         LEFT JOIN public.product_action_receipts AS receipt ON receipt.receipt_id = $2 \
         LEFT JOIN public.product_audit_events AS audit ON audit.receipt_id = $2 \
         WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn assert_terminal_persistence(row: &TerminalPersistenceRow, result_code: &str) {
    assert_eq!(row.state, "superseded");
    assert_eq!(row.product_revision, 4);
    assert_eq!(row.apply_attempt_no, 1);
    assert!(row.apply_attempt_id.is_none());
    assert!(row.apply_lease_until.is_none());
    assert_eq!(row.receipt_resulting_revision, Some(4));
    assert_eq!(row.receipt_resulting_state.as_deref(), Some("superseded"));
    assert_eq!(row.receipt_result_code.as_deref(), Some(result_code));
    assert_eq!(row.receipt_http_disposition_class, Some(4));
    assert_eq!(row.receipt_completed_at, row.audit_occurred_at);
    let termination_at =
        DateTime::parse_from_rfc3339(row.termination.as_ref().unwrap().0["at"].as_str().unwrap())
            .unwrap()
            .with_timezone(&Utc);
    assert_eq!(Some(termination_at), row.receipt_completed_at);
    assert_eq!(row.runtime_count, 0);
    assert_eq!(row.alias_count, 1);
    assert_eq!(row.audit_count, 1);
    assert_eq!(row.evidence_count, 1);
}

async fn seed_fixture(pool: &PgPool) -> Fixture {
    let suffix = suffix();
    let tail = suffix[suffix.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let tenant_id = format!("apply-tenant-{suffix}");
    let installation_id = format!("apply-installation-{suffix}");
    let promotion_id = digest(&format!("apply-promotion:{suffix}"));
    let activation_id = format!("apply_activation_{suffix}");
    let requester = Actor::new(
        &suffix,
        "apply-requester",
        (1_000_000_000 + tail).to_string(),
    );
    let actor = Actor::new(&suffix, "apply-actor", (2_000_000_000 + tail).to_string());
    let application_id = (3_000_000_000 + tail).to_string();
    let guild_id = (4_000_000_000 + tail).to_string();
    let ruleset_key = format!(
        "apply_ruleset_{}",
        &suffix[suffix.len().saturating_sub(20)..]
    );
    let promotion_request_digest = digest(&format!("apply-promotion-request:{suffix}"));
    let payload_digest = digest(&format!("apply-payload:{suffix}"));
    let target_content_hash = digest(&format!("apply-target:{suffix}"));
    let context_digest = digest(&format!("apply-context:{suffix}"));
    let guild = GuildId(guild_id.parse().unwrap());
    let required_channel_key = ResourceKey("community_hub".to_string());
    let auxiliary_role_key = ResourceKey("automation_operator".to_string());
    let required_channel_id = ChannelId(5_000_000_000 + tail);
    let auxiliary_role_id = RoleId(6_000_000_000 + tail);
    let mut resource_bindings = ResourceBindingMap::default();
    resource_bindings
        .channel_bindings
        .insert(required_channel_key.clone(), required_channel_id);
    resource_bindings
        .role_bindings
        .insert(auxiliary_role_key, auxiliary_role_id);
    let authority_binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings);
    let required_bindings = vec![ResolvedApprovalBinding::Channel {
        key: required_channel_key,
        id: required_channel_id,
    }];
    let approval_binding_fingerprint =
        approval_binding_fingerprint_v1(guild, NonZeroU64::new(1).unwrap(), &required_bindings)
            .unwrap();
    assert_ne!(
        authority_binding_fingerprint.as_str(),
        approval_binding_fingerprint.as_str()
    );
    let stored_resource_bindings = json!({
        "role_bindings": &resource_bindings.role_bindings,
        "channel_bindings": &resource_bindings.channel_bindings
    });
    let policy_digest = digest(&format!("apply-policy:{suffix}"));
    let authority_digest = digest(&format!("apply-authority:{suffix}"));
    let observation_digest = digest(&format!("apply-observation:{suffix}"));
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let created_at = database_now - TimeDelta::minutes(1);
    let expires_at = database_now + TimeDelta::minutes(59);
    let linked_at = database_now - TimeDelta::seconds(30);
    let context = json!({
        "promotion_id": promotion_id,
        "promotion_request_digest": promotion_request_digest,
        "approval_payload_digest": payload_digest,
        "approval_context_digest": context_digest,
        "binding": {
            "revision": 1,
            "fingerprint": approval_binding_fingerprint,
            "required_bindings": required_bindings
        },
        "baseline": {"state": "absent"},
        "policy": {
            "revision": 1,
            "required_approvals": 1,
            "ttl_seconds": 3600,
            "digest": policy_digest
        }
    });
    let approval_context = json!({
        "authority": "product_authoring",
        "context": context
    });
    let promotion_record = json!({
        "id": promotion_id,
        "revision": 3,
        "request_digest": promotion_request_digest,
        "intent": {
            "authority": {
                "tenant_id": tenant_id,
                "principal_id": requester.principal_id,
                "installation_id": installation_id,
                "guild_id": guild_id,
                "ruleset_key": ruleset_key,
                "binding_revision": 1
            },
            "evidence": {
                "context_fingerprint": authority_binding_fingerprint
            }
        },
        "stage": {
            "state": "activation_pending",
            "activation": {
                "request_id": activation_id,
                "target": {
                    "guild_id": guild_id,
                    "ruleset_key": ruleset_key,
                    "version": 1,
                    "content_hash": target_content_hash
                },
                "requester": requester.user_id,
                "required_approvals": 1,
                "created_at": created_at,
                "expires_at": expires_at,
                "request_state_at_journal": "pending",
                "approval_context": context
            }
        }
    });
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{}'::JSONB), ($3, $4, '{}'::JSONB)",
    )
    .bind(&requester.principal_id)
    .bind(&requester.user_id)
    .bind(&actor.principal_id)
    .bind(&actor.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for identity in [&requester, &actor] {
        sqlx::query(
            "INSERT INTO public.product_oauth_flows \
             (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, \
              expires_at, consumed_at, terminal_result_code) \
             VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
              CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
              CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
        )
        .bind(&identity.oauth_state)
        .bind(&identity.oauth_nonce)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.product_auth_sessions \
             (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
              created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
             SELECT $1, $2, $3, $4, captured_at, captured_at, captured_at, \
              captured_at + INTERVAL '20 minutes', captured_at + INTERVAL '1 hour' \
             FROM (SELECT pg_catalog.clock_timestamp() AS captured_at) AS clock",
        )
        .bind(&identity.session_digest)
        .bind(&identity.principal_id)
        .bind(&identity.csrf_digest)
        .bind(&identity.oauth_state)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) VALUES ($1, 'active', $2)",
    )
    .bind(&tenant_id)
    .bind(format!("Apply Tenant {suffix}"))
    .execute(&mut *transaction)
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
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, $3, $4, 1, 1, 3600, $5, $6, $7)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(Json(&stored_resource_bindings))
    .bind(authority_binding_fingerprint.as_str())
    .bind(&authority_digest)
    .bind(&requester.principal_id)
    .bind(digest(&format!("apply-authority-request:{suffix}")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ($1, $2, 2)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .bind(&target_content_hash)
    .bind(&requester.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, principal_id, \
          record) VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5)",
    )
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .bind(&tenant_id)
    .bind(&requester.principal_id)
    .bind(Json(&promotion_record))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest, linked_at) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, \
          'product_authoring', 'linked', $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(&activation_id)
    .bind(&guild_id)
    .bind(&ruleset_key)
    .bind(&target_content_hash)
    .bind(&requester.user_id)
    .bind(created_at)
    .bind(expires_at)
    .bind(Json(&approval_context))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .bind(&payload_digest)
    .bind(&context_digest)
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.product_approval_gate', $1, TRUE)")
        .bind(&context_digest)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_request_approvals \
         (request_id, tenant_id, installation_id, approver_id, approved_at, \
          approval_payload_digest) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&activation_id)
    .bind(&tenant_id)
    .bind(&installation_id)
    .bind(&actor.user_id)
    .bind(linked_at + TimeDelta::seconds(1))
    .bind(&payload_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'approved', product_revision = 2 \
         WHERE id = $1 AND state = 'pending' AND product_revision = 1",
    )
    .bind(&activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    Fixture {
        tenant_id,
        installation_id,
        promotion_id,
        activation_id,
        actor,
        application_id,
        guild_id,
        ruleset_key,
        payload_digest,
        authority_digest,
        observation_digest,
    }
}

#[derive(Clone)]
struct AuthorityHead {
    revision: i64,
    digest: String,
}

struct AuthorityAdvance<'a> {
    binding_revision: i64,
    resource_bindings: &'a Value,
    binding_fingerprint: &'a str,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
}

async fn authority_binding_material(pool: &PgPool, fixture: &Fixture) -> (Value, String) {
    let (bindings, fingerprint) = sqlx::query_as::<_, (Json<Value>, String)>(
        "SELECT resource_bindings, binding_fingerprint \
         FROM public.automation_installation_authority_versions \
         WHERE tenant_id = $1 AND installation_id = $2 AND revision = 1",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .fetch_one(pool)
    .await
    .unwrap();
    (bindings.0, fingerprint)
}

async fn advance_authority(
    pool: &PgPool,
    fixture: &Fixture,
    advance: AuthorityAdvance<'_>,
) -> AuthorityHead {
    let AuthorityAdvance {
        binding_revision,
        resource_bindings,
        binding_fingerprint,
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
    } = advance;
    let authority_digest = digest(&format!(
        "apply-authority-head:{}:{binding_revision}:{policy_revision}:{required_approvals}:{activation_ttl_seconds}",
        fixture.installation_id
    ));
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 2, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .bind(binding_revision)
    .bind(Json(resource_bindings))
    .bind(binding_fingerprint)
    .bind(policy_revision)
    .bind(required_approvals)
    .bind(activation_ttl_seconds)
    .bind(&authority_digest)
    .bind(&fixture.actor.principal_id)
    .bind(digest(&format!(
        "apply-authority-head-request:{}",
        fixture.installation_id
    )))
    .execute(&mut *transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $1 AND installation_id = $2 AND current_authority_revision = 1",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    transaction.commit().await.unwrap();
    AuthorityHead {
        revision: 2,
        digest: authority_digest,
    }
}

fn apply_context_at_authority(
    fixture: &Fixture,
    operation: &Operation,
    authority: &AuthorityHead,
) -> ApplyLockContext {
    let mut context = ApplyLockContext::single(fixture, operation);
    context.expected_authority_revision = authority.revision;
    context.expected_authority_digest = authority.digest.clone();
    context
}

async fn set_competing_active_baseline(pool: &PgPool, fixture: &Fixture) -> String {
    let content_hash = digest(&format!(
        "apply-competing-baseline:{}",
        fixture.activation_id
    ));
    let mut transaction = pool.begin().await.unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_ruleset_heads SET next_version = 3 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND next_version = 2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&content_hash)
    .bind(&fixture.actor.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ($1, $2, 2)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    content_hash
}
