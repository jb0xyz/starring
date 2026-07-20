const REJECTION_IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.rejection.idempotency.v1";
const REJECTION_SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.rejection.request.v1";
const REJECTION_RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.rejection.receipt.v1";
const REJECTION_AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.rejection.audit.v1";
const REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.rejection.digest-key-fingerprint.v1";

fn rejection_command(
    fixture: &Fixture,
    idempotency_key: &str,
    reason: &str,
    expected_revision: u64,
) -> RejectProductPromotionV1 {
    RejectProductPromotionV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&fixture.payload_digest).unwrap(),
        expected_revision: ProductRevisionV1::new(expected_revision).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
        reason: RejectionReasonV1::parse(reason).unwrap(),
    }
}

async fn requester_actor_fixture(pool: &PgPool, fixture: &Fixture) -> Fixture {
    let (principal_id, user_id) = sqlx::query_as::<_, (String, String)>(
        "SELECT promotion.principal_id, activation.requester_id \
         FROM public.authoring_promotions AS promotion \
         INNER JOIN public.activation_requests AS activation \
           ON activation.promotion_id = promotion.id \
         WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let credential_secret: [u8; 32] = Sha256::digest(format!(
        "requester-credential:{}:{}",
        fixture.promotion_id.as_str(),
        suffix()
    ))
    .into();
    let csrf_secret: [u8; 32] = Sha256::digest(format!(
        "requester-csrf:{}:{}",
        fixture.promotion_id.as_str(),
        suffix()
    ))
    .into();
    let credential = URL_SAFE_NO_PAD.encode(credential_secret);
    let csrf = URL_SAFE_NO_PAD.encode(csrf_secret);
    let session_digest = digest_opaque_session_credential_v1(&credential)
        .unwrap()
        .into_bytes();
    let csrf_digest = digest_opaque_session_credential_v1(&csrf)
        .unwrap()
        .into_bytes();
    let oauth_state = Sha256::digest(format!(
        "requester-oauth-state:{}:{}",
        fixture.promotion_id.as_str(),
        suffix()
    ))
    .to_vec();
    let oauth_nonce = Sha256::digest(format!(
        "requester-oauth-nonce:{}:{}",
        fixture.promotion_id.as_str(),
        suffix()
    ))
    .to_vec();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.product_oauth_flows \
         (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, \
          expires_at, consumed_at, terminal_result_code) \
         VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
          CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
          CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
    )
    .bind(&oauth_state)
    .bind(&oauth_nonce)
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
    .bind(session_digest.as_slice())
    .bind(&principal_id)
    .bind(csrf_digest.as_slice())
    .bind(&oauth_state)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let mut requester = fixture.clone();
    requester.approver_principal = PrincipalId::parse(&principal_id).unwrap();
    requester.approver_user = UserId(user_id.parse().unwrap());
    requester.credential = credential;
    requester.csrf = csrf;
    requester.session_digest = session_digest;
    requester
}

fn rejection_key(id: &str, seed: u8) -> ProductDecisionDigestKeyV1 {
    ProductDecisionDigestKeyV1::from_bytes(
        id,
        std::array::from_fn(|index| seed.wrapping_add(index as u8)),
    )
    .unwrap()
}

#[derive(sqlx::FromRow)]
struct RejectionReceiptRow {
    receipt_id: String,
    principal_id: String,
    endpoint_domain: String,
    idempotency_key_digest: String,
    idempotency_digest_key_id: String,
    idempotency_digest_key_fingerprint: String,
    request_digest: String,
    resulting_revision: i64,
    resulting_state: String,
    result_code: String,
}

#[derive(sqlx::FromRow)]
struct RejectionAuditRow {
    event_id: String,
    principal_id: String,
    action: String,
    request_id: String,
    receipt_id: String,
    resulting_state: String,
    result_code: String,
}

#[derive(sqlx::FromRow)]
struct RejectionEvidenceRow {
    receipt_id: String,
    event_id: String,
    principal_id: String,
    endpoint_domain: String,
    action: String,
    request_digest: String,
    resulting_revision: i64,
    resulting_state: String,
    result_code: String,
    evidence_version: i16,
    replay_policy_version: i16,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn requester_rejection_is_atomic_private_rotation_safe_and_exactly_replayable() {
    let pool = pool().await;
    let seeded = seed_fixture(&pool).await;
    let fixture = requester_actor_fixture(&pool, &seeded).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let active_material = std::array::from_fn(|index| 53_u8.wrapping_add(index as u8));
    let retired_material = std::array::from_fn(|index| 113_u8.wrapping_add(index as u8));
    let rejections = product_rejections(
        &pool,
        ProductDecisionDigestKeyringV1::new(
            ProductDecisionDigestKeyV1::from_bytes("rejection-e2e-v1", active_material).unwrap(),
            [ProductDecisionDigestKeyV1::from_bytes(
                "rejection-e2e-v2",
                retired_material,
            )
            .unwrap()],
        )
        .unwrap(),
    );
    let application =
        ProductControlApplication::new(&authentication, &authority, &rejections, &deployments);
    let idempotency_key = format!("reject-e2e-{}", suffix());
    let reason = format!("private rejection reason {}", suffix());
    let first_request =
        ProductRequestIdV1::parse(&format!("reject.first.{}", suffix())).unwrap();
    let first = application
        .reject(
            &fixture.credential,
            &fixture.csrf,
            &first_request,
            &selector(&fixture),
            rejection_command(&fixture, &idempotency_key, &reason, 1),
        )
        .await
        .unwrap();
    assert!(!first.exact_replay());
    assert_eq!(first.projection().revision().get(), 2);
    assert_eq!(
        first.projection().phase(),
        &ProductDecisionPhaseV1::Rejected
    );
    assert!(!format!("{first:?}").contains(&reason));
    assert!(!format!("{:?}", first.projection()).contains(&reason));

    let persisted = sqlx::query_as::<_, (String, i64, String, String, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, activation.rejected_by, \
         activation.rejection_reason, \
         (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
          WHERE approval.request_id = activation.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id \
            AND receipt.endpoint_domain = 'product_reject_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases AS alias \
          WHERE alias.tenant_id = activation.tenant_id \
            AND alias.installation_id = activation.installation_id \
            AND alias.principal_id = $2 \
            AND alias.endpoint_domain = 'product_reject_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id \
            AND audit.action = 'promotion.reject'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence AS evidence \
          WHERE evidence.target_resource_id = activation.promotion_id \
            AND evidence.action = 'promotion.reject') \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(fixture.approver_principal.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "rejected".to_string(),
            2,
            fixture.approver_user.to_string(),
            reason.clone(),
            0,
            1,
            2,
            1,
            1
        )
    );

    let idempotency_fields = vec![
        bytes(fixture.tenant_id.as_str()),
        bytes(fixture.installation_id.as_str()),
        bytes(fixture.approver_principal.as_str()),
        bytes("product_reject_v1"),
        bytes(idempotency_key.as_bytes()),
    ];
    let expected_idempotency = keyed_hex(
        &active_material,
        REJECTION_IDEMPOTENCY_DOMAIN,
        &idempotency_fields,
    );
    let expected_semantic = unkeyed_hex(
        REJECTION_SEMANTIC_REQUEST_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.installation_id.as_str()),
            bytes(fixture.approver_principal.as_str()),
            bytes(fixture.promotion_id.as_str()),
            bytes("1"),
            bytes(fixture.payload_digest.as_bytes()),
            bytes(reason.as_bytes()),
        ],
    );
    let identity_fields = vec![
        bytes(fixture.tenant_id.as_str()),
        bytes(fixture.installation_id.as_str()),
        bytes(fixture.approver_principal.as_str()),
        bytes(expected_idempotency.as_bytes()),
        bytes(expected_semantic.as_bytes()),
    ];
    let expected_receipt = keyed_hex(
        &active_material,
        REJECTION_RECEIPT_ID_DOMAIN,
        &identity_fields,
    );
    let expected_audit = keyed_hex(
        &active_material,
        REJECTION_AUDIT_EVENT_ID_DOMAIN,
        &identity_fields,
    );
    let active_fingerprint = unkeyed_hex(
        REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN,
        &[bytes(active_material)],
    );
    let retired_fingerprint = unkeyed_hex(
        REJECTION_KEY_MATERIAL_FINGERPRINT_DOMAIN,
        &[bytes(retired_material)],
    );
    let receipt = sqlx::query_as::<_, RejectionReceiptRow>(
        "SELECT receipt_id, principal_id, endpoint_domain, idempotency_key_digest, \
         idempotency_digest_key_id, idempotency_digest_key_fingerprint, request_digest, \
         resulting_revision, resulting_state, result_code \
         FROM public.product_action_receipts \
         WHERE target_resource_id = $1 AND endpoint_domain = 'product_reject_v1'",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipt.receipt_id, expected_receipt);
    assert_eq!(receipt.principal_id, fixture.approver_principal.as_str());
    assert_eq!(receipt.endpoint_domain, "product_reject_v1");
    assert_eq!(receipt.idempotency_key_digest, expected_idempotency);
    assert_eq!(receipt.idempotency_digest_key_id, "rejection-e2e-v1");
    assert_eq!(
        receipt.idempotency_digest_key_fingerprint,
        active_fingerprint
    );
    assert_eq!(receipt.request_digest, expected_semantic);
    assert_eq!(receipt.resulting_revision, 2);
    assert_eq!(receipt.resulting_state, "rejected");
    assert_eq!(receipt.result_code, "promotion_rejected");

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
    let retired_idempotency = keyed_hex(
        &retired_material,
        REJECTION_IDEMPOTENCY_DOMAIN,
        &idempotency_fields,
    );
    assert_eq!(
        aliases,
        vec![
            (
                expected_idempotency,
                "rejection-e2e-v1".to_string(),
                active_fingerprint,
                expected_receipt.clone()
            ),
            (
                retired_idempotency,
                "rejection-e2e-v2".to_string(),
                retired_fingerprint,
                expected_receipt.clone()
            )
        ]
    );

    let audit = sqlx::query_as::<_, RejectionAuditRow>(
        "SELECT event_id, principal_id, action, request_id, receipt_id, resulting_state, \
         result_code FROM public.product_audit_events \
         WHERE target_resource_id = $1 AND action = 'promotion.reject'",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.event_id, expected_audit);
    assert_eq!(audit.principal_id, fixture.approver_principal.as_str());
    assert_eq!(audit.action, "promotion.reject");
    assert_eq!(audit.request_id, first_request.as_str());
    assert_eq!(audit.receipt_id, expected_receipt);
    assert_eq!(audit.resulting_state, "rejected");
    assert_eq!(audit.result_code, "promotion_rejected");

    let evidence = sqlx::query_as::<_, RejectionEvidenceRow>(
        "SELECT receipt_id, event_id, principal_id, endpoint_domain, action, request_digest, \
         resulting_revision, resulting_state, result_code, evidence_version, \
         replay_policy_version FROM public.product_action_receipt_audit_evidence \
         WHERE target_resource_id = $1 AND action = 'promotion.reject'",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(evidence.receipt_id, receipt.receipt_id);
    assert_eq!(evidence.event_id, audit.event_id);
    assert_eq!(evidence.principal_id, receipt.principal_id);
    assert_eq!(evidence.endpoint_domain, receipt.endpoint_domain);
    assert_eq!(evidence.action, audit.action);
    assert_eq!(evidence.request_digest, receipt.request_digest);
    assert_eq!(evidence.resulting_revision, receipt.resulting_revision);
    assert_eq!(evidence.resulting_state, receipt.resulting_state);
    assert_eq!(evidence.result_code, receipt.result_code);
    assert_eq!(evidence.evidence_version, 1);
    assert_eq!(evidence.replay_policy_version, 1);

    let leaks = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT \
         EXISTS (SELECT 1 FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = $1 \
            AND pg_catalog.strpos(pg_catalog.to_jsonb(receipt)::TEXT, $2) > 0), \
         EXISTS (SELECT 1 FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = $1 \
            AND pg_catalog.strpos(pg_catalog.to_jsonb(audit)::TEXT, $2) > 0), \
         EXISTS (SELECT 1 FROM public.product_action_receipt_audit_evidence AS evidence \
          WHERE evidence.target_resource_id = $1 \
            AND pg_catalog.strpos(pg_catalog.to_jsonb(evidence)::TEXT, $2) > 0)",
    )
    .bind(fixture.promotion_id.as_str())
    .bind(&reason)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaks, (false, false, false));

    let replay = application
        .reject(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("reject.replay.{}", suffix())).unwrap(),
            &selector(&fixture),
            rejection_command(&fixture, &idempotency_key, &reason, 1),
        )
        .await
        .unwrap();
    assert!(replay.exact_replay());
    assert_eq!(replay.projection(), first.projection());
    let conflict = application
        .reject(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("reject.conflict.{}", suffix())).unwrap(),
            &selector(&fixture),
            rejection_command(
                &fixture,
                &idempotency_key,
                &format!("different private reason {}", suffix()),
                1,
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(
        conflict,
        ProductApplicationError::Control(ProductControlPortError::IdempotencyConflict)
    );

    let unknown_rejections = product_rejections(
        &pool,
        ProductDecisionDigestKeyringV1::new(rejection_key("rejection-e2e-v3", 173), []).unwrap(),
    );
    let unknown_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &unknown_rejections,
        &deployments,
    );
    assert!(matches!(
        unknown_application
            .reject(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("reject.unknown.{}", suffix())).unwrap(),
                &selector(&fixture),
                rejection_command(&fixture, &idempotency_key, &reason, 1),
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Control(ProductControlPortError::Backend(message))
            if message == "product rejection idempotency keyring does not cover live receipts"
    ));

    let rolling_rejections = product_rejections(
        &pool,
        ProductDecisionDigestKeyringV1::new(
            ProductDecisionDigestKeyV1::from_bytes("rejection-e2e-v2", retired_material).unwrap(),
            [ProductDecisionDigestKeyV1::from_bytes(
                "rejection-e2e-v1",
                active_material,
            )
            .unwrap()],
        )
        .unwrap(),
    );
    let rolling_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &rolling_rejections,
        &deployments,
    );
    let rolling = rolling_application
        .reject(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("reject.rolling.{}", suffix())).unwrap(),
            &selector(&fixture),
            rejection_command(&fixture, &idempotency_key, &reason, 1),
        )
        .await
        .unwrap();
    assert!(rolling.exact_replay());
    assert_eq!(rolling.projection(), first.projection());
    let final_counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE target_resource_id = $1 AND endpoint_domain = 'product_reject_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE target_resource_id = $1 AND action = 'promotion.reject'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE target_resource_id = $1 AND action = 'promotion.reject')",
    )
    .bind(fixture.promotion_id.as_str())
    .bind(&expected_receipt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(final_counts, (1, 2, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn rejection_preserves_prequorum_approval_and_advances_one_new_revision() {
    let pool = pool().await;
    let fixture = seed_fixture_with_required_approvals(&pool, NonZeroU32::new(2).unwrap()).await;
    let decisions = product_decisions(&pool);
    let authentication = PostgresAuthentication::new(pool.clone());
    let approval_authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let approval_application = ProductControlApplication::new(
        &authentication,
        &approval_authority,
        &decisions,
        &deployments,
    );
    let approval = approval_application
        .approve(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("reject.preapproval.{}", suffix())).unwrap(),
            &selector(&fixture),
            approval_command(&fixture, &format!("reject-preapproval-{}", suffix())),
        )
        .await
        .unwrap();
    assert_eq!(approval.projection().revision().get(), 2);
    assert_eq!(
        approval.projection().phase(),
        &ProductDecisionPhaseV1::PendingApproval
    );

    let requester = requester_actor_fixture(&pool, &fixture).await;
    let rejection_authority = authority_adapter(requester.clone());
    let rejections = product_rejections(
        &pool,
        ProductDecisionDigestKeyringV1::new(rejection_key("rejection-prequorum-v1", 67), [])
            .unwrap(),
    );
    let rejection_application = ProductControlApplication::new(
        &authentication,
        &rejection_authority,
        &rejections,
        &deployments,
    );
    let reason = format!("prequorum rejection {}", suffix());
    let rejection = rejection_application
        .reject(
            &requester.credential,
            &requester.csrf,
            &ProductRequestIdV1::parse(&format!("reject.after-approval.{}", suffix())).unwrap(),
            &selector(&requester),
            rejection_command(
                &requester,
                &format!("reject-after-approval-{}", suffix()),
                &reason,
                2,
            ),
        )
        .await
        .unwrap();
    assert_eq!(rejection.projection().revision().get(), 3);
    assert_eq!(
        rejection.projection().phase(),
        &ProductDecisionPhaseV1::Rejected
    );
    let persisted = sqlx::query_as::<_, (String, i64, String, String, i64, String, i64, i64)>(
        "SELECT activation.state, activation.product_revision, activation.rejected_by, \
         activation.rejection_reason, pg_catalog.count(approval.approver_id), \
         pg_catalog.min(approval.approver_id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id) \
         FROM public.activation_requests AS activation \
         LEFT JOIN public.activation_request_approvals AS approval \
           ON approval.request_id = activation.id \
         WHERE activation.id = $1 \
         GROUP BY activation.id",
    )
    .bind(&requester.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "rejected".to_string(),
            3,
            requester.approver_user.to_string(),
            reason,
            1,
            fixture.approver_user.to_string(),
            2,
            2
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_final_approval_and_rejection_commit_one_terminal_evidence_set() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let requester = requester_actor_fixture(&pool, &fixture).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let deployments = PendingDeployments;
    let approval_authority = authority_adapter(fixture.clone());
    let rejection_authority = authority_adapter(requester.clone());
    let decisions = product_decisions(&pool);
    let rejections = product_rejections(
        &pool,
        ProductDecisionDigestKeyringV1::new(rejection_key("rejection-race-v1", 79), []).unwrap(),
    );
    let approval_application = ProductControlApplication::new(
        &authentication,
        &approval_authority,
        &decisions,
        &deployments,
    );
    let rejection_application = ProductControlApplication::new(
        &authentication,
        &rejection_authority,
        &rejections,
        &deployments,
    );
    let approval_request =
        ProductRequestIdV1::parse(&format!("race.approve.{}", suffix())).unwrap();
    let rejection_request =
        ProductRequestIdV1::parse(&format!("race.reject.{}", suffix())).unwrap();
    let approval_key = format!("race-approve-{}", suffix());
    let rejection_key = format!("race-reject-{}", suffix());
    let rejection_reason = format!("race rejection {}", suffix());
    let installation = selector(&fixture);
    let requester_installation = selector(&requester);
    let (approval, rejection) = tokio::join!(
        approval_application.approve(
            &fixture.credential,
            &fixture.csrf,
            &approval_request,
            &installation,
            approval_command(&fixture, &approval_key),
        ),
        rejection_application.reject(
            &requester.credential,
            &requester.csrf,
            &rejection_request,
            &requester_installation,
            rejection_command(&requester, &rejection_key, &rejection_reason, 1),
        )
    );
    assert_ne!(approval.is_ok(), rejection.is_ok());
    let persisted = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
         (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
          WHERE approval.request_id = activation.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence AS evidence \
          WHERE evidence.target_resource_id = activation.promotion_id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.1, 2);
    assert_eq!((persisted.3, persisted.4, persisted.5), (1, 1, 1));
    match persisted.0.as_str() {
        "approved" => {
            assert!(approval.is_ok());
            assert!(rejection.is_err());
            assert_eq!(persisted.2, 1);
        }
        "rejected" => {
            assert!(approval.is_err());
            assert!(rejection.is_ok());
            assert_eq!(persisted.2, 0);
        }
        state => panic!("unexpected terminal state: {state}"),
    }
    let evidence = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT receipt.endpoint_domain, audit.action, evidence.endpoint_domain, \
         evidence.action, receipt.resulting_state, evidence.resulting_state \
         FROM public.product_action_receipts AS receipt \
         INNER JOIN public.product_audit_events AS audit \
           ON audit.receipt_id = receipt.receipt_id \
         INNER JOIN public.product_action_receipt_audit_evidence AS evidence \
           ON evidence.receipt_id = receipt.receipt_id AND evidence.event_id = audit.event_id \
         WHERE receipt.target_resource_id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let expected = if persisted.0 == "approved" {
        ("product_approve_v1", "promotion.approve", "approved")
    } else {
        ("product_reject_v1", "promotion.reject", "rejected")
    };
    assert_eq!(evidence.0, expected.0);
    assert_eq!(evidence.1, expected.1);
    assert_eq!(evidence.2, expected.0);
    assert_eq!(evidence.3, expected.1);
    assert_eq!(evidence.4, expected.2);
    assert_eq!(evidence.5, expected.2);
}
