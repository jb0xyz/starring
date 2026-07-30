#[derive(Clone, Debug, sqlx::FromRow)]
struct IngressAcknowledgementSqlRowV2 {
    outcome_name: String,
    source_acknowledgement_revision: Option<i64>,
    request_digest: Option<Vec<u8>>,
    canonical_request_bytes: Option<Vec<u8>>,
    acknowledgement_revision: Option<i64>,
    acknowledged_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    observed_database_now: DateTime<Utc>,
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn ingress_acknowledgement_is_replay_cas_expiry_lock_and_privilege_safe() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    ingress_acknowledgement_scenario(&mut database).await;
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn ingress_acknowledgement_migration_is_timezone_invariant() {
    let server = PostgresTestServer::start();
    let utc = ingress_acknowledgement_migration_state(
        server.connect_options(),
        "UTC",
    )
    .await;
    let seoul = ingress_acknowledgement_migration_state(
        server.connect_options(),
        "Asia/Seoul",
    )
    .await;
    assert_eq!(utc, seoul);
    assert_eq!(
        utc,
        (
            true,
            "f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63"
                .to_owned(),
            "779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e"
                .to_owned(),
        )
    );
    drop(server);
}

async fn ingress_acknowledgement_scenario(database: &mut IsolatedDatabase) {
    use sha2::Digest as _;

    assert_readiness_definition_sha(
        &database.owner_pool,
        EXPECTED_READINESS_DEFINITION_SHA256_V1,
    )
    .await;
    assert_readiness_identity(
        &database.owner_pool,
        &database.executor_pool,
        &database.name,
        &database.role,
    )
    .await;
    let adapter = verified_execution_adapter(database).await;
    let first_owner = acquire_ingress_owner(&adapter, "ingress-process-1", Duration::from_secs(30))
        .await;
    let first = ingress_acknowledgement_request(
        None,
        first_owner.clone(),
        2,
        3,
        5,
        Duration::from_secs(10),
    );
    let one_byte_request = *b"x";
    let one_byte_digest: [u8; 32] =
        sha2::Sha256::digest(one_byte_request).into();
    let short_request_error = publish_ingress_acknowledgement_payload(
        &database.executor_pool,
        &first,
        &one_byte_digest,
        &one_byte_request,
        false,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&short_request_error, "RX002");
    let infinite_time_error = publish_ingress_acknowledgement_payload(
        &database.executor_pool,
        &first,
        first.request_digest().as_bytes(),
        first.canonical_request_bytes(),
        true,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&infinite_time_error, "RX002");

    let applied = publish_ingress_acknowledgement(&database.executor_pool, &first)
        .await
        .unwrap();
    assert_eq!(applied.outcome_name, "applied");
    assert_eq!(applied.source_acknowledgement_revision, None);
    assert_eq!(applied.acknowledgement_revision, Some(1));
    assert_eq!(
        applied.request_digest.as_deref(),
        Some(first.request_digest().as_bytes().as_slice())
    );
    assert_eq!(
        applied.canonical_request_bytes.as_deref(),
        Some(first.canonical_request_bytes())
    );
    assert!(applied.acknowledged_at.unwrap() <= applied.observed_database_now);
    assert!(applied.expires_at.unwrap() > applied.observed_database_now);

    let replayed = publish_ingress_acknowledgement(&database.executor_pool, &first)
        .await
        .unwrap();
    assert_eq!(replayed.outcome_name, "replayed");
    assert_eq!(replayed.acknowledgement_revision, Some(1));
    assert_eq!(replayed.acknowledged_at, applied.acknowledged_at);
    assert_eq!(replayed.expires_at, applied.expires_at);
    assert!(replayed.observed_database_now >= applied.observed_database_now);

    let conflict = ingress_acknowledgement_request(
        None,
        first_owner.clone(),
        2,
        3,
        5,
        Duration::from_secs(9),
    );
    let conflict_observation =
        publish_ingress_acknowledgement(&database.executor_pool, &conflict)
        .await
        .unwrap();
    assert_eq!(conflict_observation.outcome_name, "not_current");
    assert_eq!(conflict_observation.acknowledgement_revision, Some(1));
    assert_eq!(
        conflict_observation.request_digest.as_deref(),
        Some(first.request_digest().as_bytes().as_slice())
    );

    let successor = ingress_acknowledgement_request(
        NonZeroU64::new(1),
        first_owner.clone(),
        2,
        4,
        7,
        Duration::from_secs(1),
    );
    let advanced = publish_ingress_acknowledgement(&database.executor_pool, &successor)
        .await
        .unwrap();
    assert_eq!(advanced.outcome_name, "applied");
    assert_eq!(advanced.source_acknowledgement_revision, Some(1));
    assert_eq!(advanced.acknowledgement_revision, Some(2));

    let stale = ingress_acknowledgement_request(
        NonZeroU64::new(1),
        first_owner.clone(),
        2,
        5,
        9,
        Duration::from_secs(10),
    );
    let not_current = publish_ingress_acknowledgement(&database.executor_pool, &stale)
        .await
        .unwrap();
    assert_eq!(not_current.outcome_name, "not_current");
    assert_eq!(not_current.acknowledgement_revision, Some(2));

    wait_for_database_time(
        &database.owner_pool,
        advanced.expires_at.expect("successor expiry"),
    )
    .await;
    let expired_replay = publish_ingress_acknowledgement(&database.executor_pool, &successor)
        .await
        .unwrap();
    assert_eq!(expired_replay.outcome_name, "not_current");
    assert_eq!(expired_replay.acknowledgement_revision, Some(2));
    assert!(expired_replay.observed_database_now >= expired_replay.expires_at.unwrap());
    let expired_observation = observe_ingress_acknowledgement(&database.executor_pool)
        .await
        .unwrap();
    assert_eq!(expired_observation.outcome_name, "present");
    assert_eq!(expired_observation.acknowledgement_revision, Some(2));
    assert!(
        expired_observation.observed_database_now >= expired_observation.expires_at.unwrap()
    );

    let release = RuntimeReleaseGatewayOwnerLeaseV1 {
        lease_id: first_owner.lease_id.clone(),
    };
    assert!(matches!(
        RuntimeGatewayOwnerLeasePortV1::release_gateway_owner(&adapter, release)
            .await
            .unwrap(),
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { .. }
    ));
    let capped_owner =
        acquire_ingress_owner(&adapter, "ingress-process-2", Duration::from_secs(2)).await;
    let source_free_restart = ingress_acknowledgement_request(
        None,
        capped_owner.clone(),
        2,
        5,
        11,
        Duration::from_secs(1),
    );
    let predecessor_observation =
        publish_ingress_acknowledgement(&database.executor_pool, &source_free_restart)
            .await
            .unwrap();
    assert_eq!(predecessor_observation.outcome_name, "not_current");
    assert_eq!(
        predecessor_observation.acknowledgement_revision,
        Some(2)
    );
    let capped = ingress_acknowledgement_request(
        NonZeroU64::new(2),
        capped_owner.clone(),
        2,
        5,
        11,
        Duration::from_secs(10),
    );
    let capped_applied = publish_ingress_acknowledgement(&database.executor_pool, &capped)
        .await
        .unwrap();
    assert_eq!(capped_applied.outcome_name, "applied");
    assert_eq!(capped_applied.acknowledgement_revision, Some(3));
    assert_eq!(capped_applied.expires_at, Some(capped_owner.expires_at));

    let renewal = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: capped_owner.lease_id.clone(),
        expected_owner_revision: capped_owner.owner_revision,
        lease_for: RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(30)).unwrap(),
    };
    let RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(renewed_owner) =
        RuntimeGatewayOwnerLeasePortV1::renew_gateway_owner(&adapter, renewal)
            .await
            .unwrap()
    else {
        panic!("exact ingress owner renewal must win")
    };
    let concurrent = ingress_acknowledgement_request(
        NonZeroU64::new(3),
        renewed_owner,
        2,
        6,
        13,
        Duration::from_secs(10),
    );
    let left_pool = database.executor_pool.clone();
    let right_pool = database.executor_pool.clone();
    let left_request = concurrent.clone();
    let right_request = concurrent.clone();
    let (left, right) = tokio::join!(
        publish_ingress_acknowledgement(&left_pool, &left_request),
        publish_ingress_acknowledgement(&right_pool, &right_request)
    );
    let mut outcomes = [left.unwrap().outcome_name, right.unwrap().outcome_name];
    outcomes.sort();
    assert_eq!(outcomes, ["applied", "replayed"]);

    let raw_table_error = sqlx::query(
        "SELECT * FROM public.runtime_ingress_open_acknowledgements_v2",
    )
    .execute(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&raw_table_error, "42501");
    let unauthorized = restricted_readiness_pool(database, "ingress_none", &[]).await;
    let unauthorized_error = sqlx::query(
        "SELECT * FROM public.starring_runtime_ingress_open_acknowledgement_observe_v2('shard:0')",
    )
    .execute(&unauthorized)
    .await
    .unwrap_err();
    assert_sqlstate(&unauthorized_error, "42501");
    unauthorized.close().await;

    let acl = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
         pg_catalog.count(DISTINCT privilege.grantee) FILTER (WHERE privilege.grantee <> function_row.proowner), \
         pg_catalog.count(*) FILTER (WHERE privilege.grantee = 0), \
         pg_catalog.count(*) FILTER (WHERE privilege.is_grantable) \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(function_row.proacl, pg_catalog.acldefault('f', function_row.proowner))) AS privilege \
         WHERE function_row.oid IN ( \
             pg_catalog.to_regprocedure('public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)'), \
             pg_catalog.to_regprocedure('public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)') \
         )",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(acl, (1, 0, 0));
}

async fn ingress_acknowledgement_migration_state(
    base: PgConnectOptions,
    timezone: &str,
) -> (bool, String, String) {
    let name = format!("starring_re_timezone_{}", unique_suffix());
    assert!(canonical_identifier(&name));
    let mut administrator = PgConnection::connect_with(
        &base.clone().database("postgres"),
    )
    .await
    .unwrap();
    administrator
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .unwrap();
    let timezone = timezone.to_owned();
    let owner_pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let timezone = timezone.clone();
            Box::pin(async move {
                sqlx::query(
                    "SELECT pg_catalog.set_config('TimeZone', $1, FALSE)",
                )
                .bind(timezone)
                .execute(connection)
                .await?;
                Ok(())
            })
        })
        .connect_with(base.database(&name))
        .await
        .unwrap();
    MIGRATOR.run(&owner_pool).await.unwrap();
    MIGRATOR.run(&owner_pool).await.unwrap();
    let state = sqlx::query_as::<_, (bool, String, String)>(
        "SELECT \
            public.starring_runtime_execution_schema_manifest_v1(), \
            pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
                pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure( \
                    'public.starring_runtime_execution_schema_manifest_v1()' \
                )), 'UTF8' \
            )), 'hex'), \
            pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
                pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure( \
                    'public.starring_runtime_execution_database_readiness_v1()' \
                )), 'UTF8' \
            )), 'hex')",
    )
    .fetch_one(&owner_pool)
    .await
    .unwrap();
    owner_pool.close().await;
    administrator
        .execute(format!("DROP DATABASE {name} WITH (FORCE)").as_str())
        .await
        .unwrap();
    state
}

async fn acquire_ingress_owner(
    adapter: &PostgresRuntimeExecutionV1,
    process: &str,
    lease_for: Duration,
) -> automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1 {
    let request = RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        expected_build_revision: RuntimeBuildRevisionV1::parse("runtime:ingress").unwrap(),
        lease_for: RuntimeGatewayOwnerLeaseDurationV1::new(lease_for).unwrap(),
    };
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(receipt) =
        RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(adapter, request)
            .await
            .unwrap()
    else {
        panic!("ingress owner acquisition must win")
    };
    receipt
}

fn ingress_acknowledgement_request(
    source_acknowledgement_revision: Option<NonZeroU64>,
    owner_receipt: automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
    connection_epoch: u64,
    maintenance_gate_generation: u64,
    admission_revision: u64,
    lease_for: Duration,
) -> RuntimePublishIngressOpenAcknowledgementV2 {
    let connected_event_sequence = admission_revision
        .checked_add(2)
        .and_then(NonZeroU64::new)
        .unwrap();
    let resume_sequence = admission_revision
        .checked_add(4)
        .and_then(NonZeroU64::new)
        .unwrap();
    RuntimePublishIngressOpenAcknowledgementV2::new(
        RuntimePublishIngressOpenAcknowledgementInputV2 {
            source_acknowledgement_revision,
            fence_generation: RuntimeWriterFenceGenerationV1::new(NonZeroU64::new(1).unwrap()),
            maintenance_gate_generation: NonZeroU64::new(maintenance_gate_generation).unwrap(),
            gateway_ready: RuntimeGatewayReadyAttestationV2 {
                process_instance_id: owner_receipt.lease_id.process_instance_id.clone(),
                connection_epoch: NonZeroU64::new(connection_epoch).unwrap(),
                kind: RuntimeGatewayReadyKindV2::Ready,
                admission_revision: NonZeroU64::new(admission_revision).unwrap(),
                connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(
                    connected_event_sequence,
                ),
                resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(resume_sequence),
            },
            owner_receipt,
            lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(lease_for)
                .unwrap(),
        },
    )
    .unwrap()
}

async fn publish_ingress_acknowledgement(
    pool: &PgPool,
    request: &RuntimePublishIngressOpenAcknowledgementV2,
) -> Result<IngressAcknowledgementSqlRowV2, sqlx::Error> {
    publish_ingress_acknowledgement_payload(
        pool,
        request,
        request.request_digest().as_bytes(),
        request.canonical_request_bytes(),
        false,
    )
    .await
}

async fn publish_ingress_acknowledgement_payload(
    pool: &PgPool,
    request: &RuntimePublishIngressOpenAcknowledgementV2,
    request_digest: &[u8],
    canonical_request_bytes: &[u8],
    use_infinite_owner_observed_at: bool,
) -> Result<IngressAcknowledgementSqlRowV2, sqlx::Error> {
    let owner = request.owner_receipt();
    let ready = request.gateway_ready();
    sqlx::query_as::<_, IngressAcknowledgementSqlRowV2>(
        "SELECT outcome_name, source_acknowledgement_revision, request_digest, \
         canonical_request_bytes, acknowledgement_revision, acknowledged_at, \
         expires_at, observed_database_now \
         FROM public.starring_runtime_ingress_open_acknowledgement_publish_v2( \
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
             CASE WHEN $18 THEN '-infinity'::TIMESTAMPTZ ELSE $11 END, \
             $12, $13, $14, $15, $16, $17 \
         )",
    )
    .bind(owner.lease_id.gateway_shard_id.as_str())
    .bind(
        request
            .source_acknowledgement_revision()
            .map(|revision| i64::try_from(revision.get()).unwrap()),
    )
    .bind(request_digest)
    .bind(canonical_request_bytes)
    .bind(i64::try_from(request.fence_generation().get()).unwrap())
    .bind(i64::try_from(request.maintenance_gate_generation().get()).unwrap())
    .bind(owner.lease_id.process_instance_id.as_str())
    .bind(i64::try_from(owner.lease_id.lease_epoch.get()).unwrap())
    .bind(owner.lease_id.expected_build_revision.as_str())
    .bind(i64::try_from(owner.owner_revision.get()).unwrap())
    .bind(owner.database_now)
    .bind(owner.expires_at)
    .bind(i64::try_from(ready.connection_epoch.get()).unwrap())
    .bind(i64::try_from(ready.admission_revision.get()).unwrap())
    .bind(i64::try_from(ready.connected_event_sequence.get()).unwrap())
    .bind(i64::try_from(ready.resume_sequence.get()).unwrap())
    .bind(i64::try_from(request.lease_for().milliseconds()).unwrap())
    .bind(use_infinite_owner_observed_at)
    .fetch_one(pool)
    .await
}

async fn observe_ingress_acknowledgement(
    pool: &PgPool,
) -> Result<IngressAcknowledgementSqlRowV2, sqlx::Error> {
    sqlx::query_as::<_, IngressAcknowledgementSqlRowV2>(
        "SELECT outcome_name, source_acknowledgement_revision, request_digest, \
         canonical_request_bytes, acknowledgement_revision, acknowledged_at, \
         expires_at, observed_database_now \
         FROM public.starring_runtime_ingress_open_acknowledgement_observe_v2('shard:0')",
    )
    .fetch_one(pool)
    .await
}
