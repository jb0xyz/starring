async fn assert_drift_supersession(rotation: AuthorityRotation, expected_reason: &str) {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let rotated = rotate_authority(&pool, &fixture, rotation).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(rotated.clone());
    let deployments = PendingDeployments;
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let error = application
        .apply(
            &rotated.credential,
            &rotated.csrf,
            &ProductRequestIdV1::parse(&format!("apply.drift.{}", suffix())).unwrap(),
            &selector(&rotated),
            apply_command(&rotated, &format!("apply-drift-{}", suffix())),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::Control(ProductControlPortError::Superseded)
    );
    assert_eq!(
        application
            .get_product_status(
                &rotated.credential,
                &selector(&rotated),
                status_query(&rotated)
            )
            .await
            .unwrap(),
        ProductStatusV1::Superseded
    );
    let persisted = sqlx::query_as::<_, (String, i64, String, i64)>(
        "SELECT activation.state, activation.product_revision, \
         activation.termination #>> '{reason,reason}', \
         (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
          WHERE deployment.activation_request_id = activation.id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&rotated.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        ("superseded".to_string(), 4, expected_reason.to_string(), 0)
    );
    let stale_authority = authority_adapter(fixture.clone());
    let stale_application =
        ProductControlApplication::new(&authentication, &stale_authority, &decisions, &deployments);
    assert_eq!(
        stale_application
            .get_product_status(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture)
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Control(ProductControlPortError::InvalidState)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn binding_drift_is_terminal_and_readable_with_current_authority() {
    assert_drift_supersession(AuthorityRotation::Binding, "binding_drift").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn policy_drift_is_terminal_and_readable_with_current_authority() {
    assert_drift_supersession(AuthorityRotation::Policy, "policy_drift").await;
}

async fn assert_applied_history_survives_authority_rotation(rotation: AuthorityRotation) {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let original_authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let original = ProductControlApplication::new(
        &authentication,
        &original_authority,
        &decisions,
        &deployments,
    );
    let applied = original
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.history.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-history-{}", suffix())),
        )
        .await
        .unwrap();
    let rotated = rotate_authority(&pool, &fixture, rotation).await;
    let current_authority = authority_adapter(rotated.clone());
    let current = ProductControlApplication::new(
        &authentication,
        &current_authority,
        &decisions,
        &deployments,
    );
    let preview = current
        .get_approval_preview(
            &rotated.credential,
            &selector(&rotated),
            status_query(&rotated),
        )
        .await
        .unwrap();
    assert_eq!(
        preview.phase(),
        &ProductDecisionPhaseV1::Applied {
            exact_deployment: applied.exact_deployment().clone(),
        }
    );
    assert_eq!(
        original
            .get_approval_preview(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture),
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Control(ProductControlPortError::InvalidState)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn applied_history_survives_later_binding_rotation_for_current_readers() {
    assert_applied_history_survives_authority_rotation(AuthorityRotation::Binding).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn applied_history_survives_later_policy_rotation_for_current_readers() {
    assert_applied_history_survives_authority_rotation(AuthorityRotation::Policy).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn corrupted_historical_generation_hash_returns_redacted_integrity_failure() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let installation = selector(&fixture);
    let query = status_query(&fixture);
    application
        .get_approval_preview(&fixture.credential, &installation, query.clone())
        .await
        .unwrap();

    let original_hash = sqlx::query_scalar::<_, String>(
        "SELECT generation.candidate_hash FROM public.authoring_session_generations AS generation \
         INNER JOIN public.authoring_promotions AS promotion \
           ON promotion.tenant_id = generation.tenant_id \
           AND promotion.installation_id = generation.installation_id \
           AND promotion.record #>> '{intent,authority,session_id}' = generation.session_id \
           AND (promotion.record #>> '{intent,authority,session_generation}')::BIGINT \
             = generation.generation \
         WHERE promotion.id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let corrupted_hash = sha256_hex(&format!(
        "corrupted-historical-generation:{}",
        fixture.promotion_id.as_str()
    ));
    assert_ne!(corrupted_hash, original_hash);

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.authoring_session_generations \
         DISABLE TRIGGER authoring_generations_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let update = sqlx::query(
        "UPDATE public.authoring_session_generations AS generation \
         SET candidate_hash = $1 \
         FROM public.authoring_promotions AS promotion \
         WHERE promotion.id = $2 \
           AND promotion.tenant_id = generation.tenant_id \
           AND promotion.installation_id = generation.installation_id \
           AND promotion.record #>> '{intent,authority,session_id}' = generation.session_id \
           AND (promotion.record #>> '{intent,authority,session_generation}')::BIGINT \
             = generation.generation",
    )
    .bind(&corrupted_hash)
    .bind(fixture.promotion_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(update.rows_affected(), 1);
    sqlx::query(
        "ALTER TABLE public.authoring_session_generations \
         ENABLE TRIGGER authoring_generations_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let expected = ProductApplicationError::Control(ProductControlPortError::Backend(
        "persisted product decision violates its integrity contract".to_string(),
    ));
    let preview_error = application
        .get_approval_preview(&fixture.credential, &installation, query.clone())
        .await
        .unwrap_err();
    assert_eq!(preview_error, expected);
    assert!(!format!("{preview_error:?}").contains(&corrupted_hash));

    let status_error = application
        .get_product_status(&fixture.credential, &installation, query)
        .await
        .unwrap_err();
    assert_eq!(status_error, expected);
    assert!(!format!("{status_error:?}").contains(&corrupted_hash));
}
