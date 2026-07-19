fn selector(fixture: &Fixture) -> InstallationSelectorV1 {
    InstallationSelectorV1::new(fixture.installation_id.clone())
}

fn status_query(fixture: &Fixture) -> ProductStatusQueryV1 {
    ProductStatusQueryV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
    }
}

fn approval_command(fixture: &Fixture, idempotency_key: &str) -> ApproveProductPromotionV1 {
    ApproveProductPromotionV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&fixture.payload_digest).unwrap(),
        expected_revision: ProductRevisionV1::new(1).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
    }
}

fn apply_command(fixture: &Fixture, idempotency_key: &str) -> ApplyProductPromotionV1 {
    ApplyProductPromotionV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&fixture.payload_digest).unwrap(),
        expected_revision: ProductRevisionV1::new(2).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
    }
}

async fn corrupt_e2e_target_with_baseline_drift(pool: &PgPool, fixture: &Fixture) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_activations \
         DISABLE TRIGGER automation_ruleset_activations_assert_product_slot",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         SELECT activation.guild_id, activation.ruleset_key, 2, 1, \
          pg_catalog.jsonb_build_object('version', 2, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $2, activation.requester_id \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind("91d936ba08910497f8f31e16e7f2b1ffce5ee9447a4636d47ddddc5c79fb0103")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) \
         SELECT guild_id, ruleset_key, 2 FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_activations \
         ENABLE TRIGGER automation_ruleset_activations_assert_product_slot",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DISABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DROP CONSTRAINT arv_content_integrity",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE public.automation_ruleset_versions AS version \
         SET definition = pg_catalog.jsonb_build_object(\
          'version', 2, 'panels', '[]'::JSONB, 'modals', '[]'::JSONB, 'rules', '[]'::JSONB) \
         FROM public.activation_requests AS activation \
         WHERE activation.id = $1 AND version.guild_id = activation.guild_id \
           AND version.ruleset_key = activation.ruleset_key \
           AND version.version = activation.target_version",
    )
    .bind(&fixture.activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ADD CONSTRAINT arv_content_integrity CHECK (canonical_content_hash IS NOT NULL \
          AND canonical_content_hash = content_hash) NOT VALID",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ENABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[derive(sqlx::FromRow)]
struct ReceiptRow {
    receipt_id: String,
    idempotency_key_digest: String,
    idempotency_digest_key_id: String,
    idempotency_digest_key_fingerprint: String,
    request_digest: String,
    resulting_revision: i64,
    resulting_state: String,
    result_code: String,
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    event_id: String,
    session_subject_digest: Vec<u8>,
    request_id: String,
    authority_observation_digest: String,
    effective_permission_bits: String,
    authority_observed_at: DateTime<Utc>,
    payload_digest: String,
    binding_fingerprint: String,
    policy_revision: i64,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_control_application_approves_and_replays_through_all_trust_boundaries() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let client_calls = Arc::new(AtomicUsize::new(0));
    let authority = DiscordGuildAuthorityAdapter::with_clock(
        PostgresInstallationAuthoritySource::new(pool.clone()),
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
    let authentication = PostgresAuthentication::new(pool.clone());
    let key_material = std::array::from_fn(|index| 41_u8.wrapping_add(index as u8));
    let keyring = ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes("product-e2e-v1", key_material).unwrap(),
        [ProductDecisionDigestKeyV1::from_bytes(
            "product-e2e-v2",
            std::array::from_fn(|index| 97_u8.wrapping_add(index as u8)),
        )
        .unwrap()],
    )
    .unwrap();
    let decisions = PostgresProductDecisions::new(product_decision_pools(&pool), keyring).unwrap();
    decisions.verify_keyring_coverage().await.unwrap();
    let deployments =
        PostgresProductDeploymentStatuses::new(PostgresRuntimeConvergence::new(pool.clone()));
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let installation = selector(&fixture);

    let preview = application
        .get_approval_preview(&fixture.credential, &installation, status_query(&fixture))
        .await
        .unwrap();
    assert_eq!(preview.installation_id(), &fixture.installation_id);
    assert_eq!(preview.guild_id(), fixture.guild_id);
    assert_eq!(preview.payload(), &fixture.payload);
    assert_eq!(preview.payload_digest().as_str(), fixture.payload_digest);
    assert_eq!(preview.revision().get(), 1);
    assert_eq!(preview.phase(), &ProductDecisionPhaseV1::PendingApproval);
    assert_eq!(
        application
            .get_product_status(&fixture.credential, &installation, status_query(&fixture))
            .await
            .unwrap(),
        ProductStatusV1::PendingApproval
    );

    let idempotency_key = format!("approve-e2e-{}", suffix());
    let first_request = ProductRequestIdV1::parse(&format!("approve.first.{}", suffix())).unwrap();
    let wrong_csrf = URL_SAFE_NO_PAD.encode([201_u8; 32]);
    let calls_before_invalid_csrf = client_calls.load(Ordering::SeqCst);
    assert_eq!(
        application
            .approve(
                &fixture.credential,
                &wrong_csrf,
                &first_request,
                &installation,
                approval_command(&fixture, &idempotency_key)
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Authentication(AuthenticationError::InvalidCsrf)
    );
    assert_eq!(
        client_calls.load(Ordering::SeqCst),
        calls_before_invalid_csrf
    );

    let first = application
        .approve(
            &fixture.credential,
            &fixture.csrf,
            &first_request,
            &installation,
            approval_command(&fixture, &idempotency_key),
        )
        .await
        .unwrap();
    assert!(!first.exact_replay());
    assert_eq!(first.projection().revision().get(), 2);
    assert_eq!(
        first.projection().phase(),
        &ProductDecisionPhaseV1::Approved
    );
    assert_eq!(
        application
            .get_product_status(&fixture.credential, &installation, status_query(&fixture))
            .await
            .unwrap(),
        ProductStatusV1::Approved
    );
    decisions.verify_keyring_coverage().await.unwrap();
    let unknown_only = PostgresProductDecisions::new(
        product_decision_pools(&pool),
        ProductDecisionDigestKeyringV1::new(
            ProductDecisionDigestKeyV1::from_bytes(
                "product-e2e-v3",
                std::array::from_fn(|index| 151_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
            [],
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        unknown_only.verify_keyring_coverage().await.unwrap_err(),
        ProductDecisionReadinessErrorV1::IncompleteCoverage
    );
    let next_key_material = std::array::from_fn(|index| 97_u8.wrapping_add(index as u8));
    let rolling = PostgresProductDecisions::new(
        product_decision_pools(&pool),
        ProductDecisionDigestKeyringV1::new(
            ProductDecisionDigestKeyV1::from_bytes("product-e2e-v2", next_key_material).unwrap(),
            [ProductDecisionDigestKeyV1::from_bytes("product-e2e-v1", key_material).unwrap()],
        )
        .unwrap(),
    )
    .unwrap();
    rolling.verify_keyring_coverage().await.unwrap();

    let replay_request =
        ProductRequestIdV1::parse(&format!("approve.replay.{}", suffix())).unwrap();
    let replay = application
        .approve(
            &fixture.credential,
            &fixture.csrf,
            &replay_request,
            &installation,
            approval_command(&fixture, &idempotency_key),
        )
        .await
        .unwrap();
    assert!(replay.exact_replay());
    assert_eq!(replay.projection(), first.projection());

    let persisted = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
         (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
          WHERE approval.request_id = activation.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, ("approved".to_string(), 2, 1, 1, 1));

    let receipt = sqlx::query_as::<_, ReceiptRow>(
        "SELECT receipt_id, idempotency_key_digest, idempotency_digest_key_id, \
         idempotency_digest_key_fingerprint, request_digest, resulting_revision, \
         resulting_state, result_code FROM public.product_action_receipts \
         WHERE target_resource_id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let idempotency_fields = vec![
        bytes(fixture.tenant_id.as_str()),
        bytes(fixture.installation_id.as_str()),
        bytes(fixture.approver_principal.as_str()),
        bytes("product_approve_v1"),
        bytes(idempotency_key.as_bytes()),
    ];
    let expected_idempotency = keyed_hex(&key_material, IDEMPOTENCY_DOMAIN, &idempotency_fields);
    let expected_semantic = unkeyed_hex(
        SEMANTIC_REQUEST_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.installation_id.as_str()),
            bytes(fixture.approver_principal.as_str()),
            bytes(fixture.promotion_id.as_str()),
            bytes("1"),
            bytes(fixture.payload_digest.as_bytes()),
        ],
    );
    let identity_fields = vec![
        bytes(fixture.tenant_id.as_str()),
        bytes(fixture.installation_id.as_str()),
        bytes(fixture.approver_principal.as_str()),
        bytes(expected_idempotency.as_bytes()),
        bytes(expected_semantic.as_bytes()),
    ];
    let expected_receipt = keyed_hex(&key_material, RECEIPT_ID_DOMAIN, &identity_fields);
    let expected_audit = keyed_hex(&key_material, AUDIT_EVENT_ID_DOMAIN, &identity_fields);
    let expected_key_fingerprint =
        unkeyed_hex(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[bytes(key_material)]);
    assert_eq!(receipt.receipt_id, expected_receipt);
    assert_eq!(receipt.idempotency_key_digest, expected_idempotency);
    assert_eq!(receipt.idempotency_digest_key_id, "product-e2e-v1");
    assert_eq!(
        receipt.idempotency_digest_key_fingerprint,
        expected_key_fingerprint
    );
    assert_eq!(receipt.request_digest, expected_semantic);
    assert_eq!(receipt.resulting_revision, 2);
    assert_eq!(receipt.resulting_state, "approved");
    assert_eq!(receipt.result_code, "approval_quorum_reached");

    let aliases = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT idempotency_key_digest, idempotency_digest_key_id, \
         idempotency_digest_key_fingerprint, receipt_id \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE receipt_id = $1 ORDER BY idempotency_digest_key_id",
    )
    .bind(&expected_receipt)
    .fetch_all(&pool)
    .await
    .unwrap();
    let next_idempotency = keyed_hex(&next_key_material, IDEMPOTENCY_DOMAIN, &idempotency_fields);
    let next_key_fingerprint =
        unkeyed_hex(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[bytes(next_key_material)]);
    assert_eq!(
        aliases,
        vec![
            (
                expected_idempotency,
                "product-e2e-v1".to_string(),
                expected_key_fingerprint,
                expected_receipt.clone()
            ),
            (
                next_idempotency,
                "product-e2e-v2".to_string(),
                next_key_fingerprint,
                expected_receipt
            )
        ]
    );

    let audit = sqlx::query_as::<_, AuditRow>(
        "SELECT event_id, session_subject_digest, request_id, authority_observation_digest, \
         effective_permission_bits::TEXT, authority_observed_at, payload_digest, \
         binding_fingerprint, policy_revision FROM public.product_audit_events \
         WHERE target_resource_id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let expected_session_subject = unkeyed_bytes(
        SESSION_SUBJECT_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.approver_principal.as_str()),
            bytes(fixture.session_digest),
        ],
    );
    let effective_permissions = Permissions::VIEW_CHANNEL | Permissions::MANAGE_GUILD;
    let authority_expiry = audit.authority_observed_at + TimeDelta::seconds(5);
    let expected_observation = unkeyed_hex(
        AUTHORITY_OBSERVATION_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.installation_id.as_str()),
            bytes(fixture.application_id.to_string()),
            bytes(fixture.guild_id.to_string()),
            bytes(fixture.approver_user.to_string()),
            bytes("approve"),
            bytes(effective_permissions.bits().to_string()),
            bytes("member"),
            bytes("1"),
            bytes(fixture.authority_digest.as_bytes()),
            bytes(fixture.guild_id.to_string()),
            bytes(Permissions::VIEW_CHANNEL.bits().to_string()),
            bytes(fixture.manager_role_id.to_string()),
            bytes(Permissions::MANAGE_GUILD.bits().to_string()),
            bytes(audit.authority_observed_at.timestamp_millis().to_string()),
            bytes(authority_expiry.timestamp_millis().to_string()),
        ],
    );
    assert_eq!(audit.event_id, expected_audit);
    assert_eq!(audit.session_subject_digest, expected_session_subject);
    assert_ne!(audit.session_subject_digest, fixture.session_digest);
    assert_eq!(audit.request_id, first_request.as_str());
    assert_eq!(audit.authority_observation_digest, expected_observation);
    assert_eq!(
        audit.effective_permission_bits,
        effective_permissions.bits().to_string()
    );
    assert_eq!(audit.payload_digest, fixture.payload_digest);
    assert_eq!(
        audit.binding_fingerprint,
        fixture.authority_binding_fingerprint
    );
    assert_eq!(audit.policy_revision, 1);
    assert_eq!(client_calls.load(Ordering::SeqCst), 5);

    let apply_key = format!("apply-e2e-{}", suffix());
    let apply_request = ProductRequestIdV1::parse(&format!("apply.first.{}", suffix())).unwrap();
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &apply_request,
            &installation,
            apply_command(&fixture, &apply_key),
        )
        .await
        .unwrap();
    assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
    assert!(!applied.exact_replay());
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &installation,
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );

    let replay_request = ProductRequestIdV1::parse(&format!("apply.replay.{}", suffix())).unwrap();
    let replay = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &replay_request,
            &installation,
            apply_command(&fixture, &apply_key),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), ProductStatusV1::RuntimePending);
    assert!(replay.exact_replay());
    assert_eq!(replay.exact_deployment(), applied.exact_deployment());

    let applied_state = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations AS active \
          WHERE active.guild_id = activation.guild_id \
            AND active.ruleset_key = activation.ruleset_key \
            AND active.active_version = activation.target_version), \
         (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
          WHERE deployment.activation_request_id = activation.id \
            AND deployment.phase = 'requested'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id \
            AND receipt.endpoint_domain = 'product_apply_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id \
            AND audit.action = 'promotion.apply') \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied_state, ("applied".to_string(), 4, 1, 1, 1, 1));

    let rotated = rotate_authority(&pool, &fixture, AuthorityRotation::Safe).await;
    assert_eq!(
        application
            .get_product_status(
                &rotated.credential,
                &selector(&rotated),
                status_query(&rotated)
            )
            .await
            .unwrap(),
        ProductStatusV1::RuntimePending
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_apply_maps_corrupt_drift_target_without_persisting_apply_evidence() {
    let database = isolated_product_control_database("artifact_integrity").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        let fixture = seed_fixture(pool).await;
        let authority = DiscordGuildAuthorityAdapter::with_clock(
            Source {
                fixture: fixture.clone(),
                calls: Arc::new(AtomicUsize::new(0)),
            },
            Client {
                fixture: fixture.clone(),
                calls: Arc::new(AtomicUsize::new(0)),
            },
            SubmicrosecondClock,
            DiscordAuthorityConfigV1::new(
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(30),
            )
            .unwrap(),
        );
        let authentication = PostgresAuthentication::new(pool.clone());
        let key_material = std::array::from_fn(|index| 151_u8.wrapping_add(index as u8));
        let decisions = PostgresProductDecisions::new(
            product_decision_pools(pool),
            ProductDecisionDigestKeyringV1::new(
                ProductDecisionDigestKeyV1::from_bytes("product-integrity-v1", key_material)
                    .unwrap(),
                [],
            )
            .unwrap(),
        )
        .unwrap();
        let deployments =
            PostgresProductDeploymentStatuses::new(PostgresRuntimeConvergence::new(pool.clone()));
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let installation = selector(&fixture);
        application
            .approve(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("integrity.approve.{}", suffix())).unwrap(),
                &installation,
                approval_command(&fixture, &format!("integrity-approve-{}", suffix())),
            )
            .await
            .unwrap();
        corrupt_e2e_target_with_baseline_drift(pool, &fixture).await;
        let error = application
            .apply(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("integrity.apply.{}", suffix())).unwrap(),
                &installation,
                apply_command(&fixture, &format!("integrity-apply-{}", suffix())),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::Control(ProductControlPortError::InvalidServerCandidate(
                ProductCandidateErrorCodeV1::TargetCorrupt
            ))
        );
        let persisted = sqlx::query_as::<
            _,
            (String, i64, i64, Option<Json<Value>>, i64, i64, i64, i64),
        >(
            "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
             activation.termination, \
             (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
              WHERE activation_request_id = activation.id), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
              WHERE endpoint_domain = 'product_apply_v1' AND target_resource_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_audit_events \
              WHERE action = 'promotion.apply' AND target_resource_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
              WHERE action = 'promotion.apply' AND target_resource_id = $2) \
             FROM public.activation_requests AS activation WHERE activation.id = $1",
        )
        .bind(&fixture.activation_id)
        .bind(fixture.promotion_id.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            persisted,
            ("approved".to_string(), 2, 0, None, 0, 0, 0, 0)
        );
    }
    drop_isolated_product_control_database(database).await;
}
