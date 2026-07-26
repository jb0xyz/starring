#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn writer_fence_observation_is_function_only_and_expiry_stays_closed() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    writer_fence_observation_scenario(&database).await;
    cleanup(database).await;
    drop(server);
}

async fn writer_fence_observation_scenario(database: &IsolatedDatabase) {
    type WriterFenceRow = (
        String,
        i64,
        Option<String>,
        Option<i64>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );

    let open = sqlx::query_as::<_, WriterFenceRow>(
        "SELECT * FROM public.starring_runtime_writer_fence_observe_v1()",
    )
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(open.0, "open");
    assert_eq!(open.1, 1);
    assert_eq!(open.2, None);
    assert_eq!(open.3, None);
    assert_eq!(open.5, None);
    let adapter = verified_execution_adapter(database).await;
    let RuntimeWriterFenceObservationV1::Open { generation, .. } =
        RuntimeWriterFenceObservationPortV1::observe_writer_fence(
            &adapter,
            RuntimeObserveWriterFenceV1,
        )
        .await
        .unwrap()
    else {
        panic!("initial writer fence must be open")
    };
    assert_eq!(generation.get(), 1);

    for statement in [
        "INSERT INTO public.runtime_writer_fence DEFAULT VALUES",
        "UPDATE public.runtime_writer_fence SET fence_generation = fence_generation",
        "DELETE FROM public.runtime_writer_fence",
        "TRUNCATE public.runtime_writer_fence",
    ] {
        let rejected = sqlx::query(statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&rejected, "23514");
    }

    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'closed', fence_generation = 2, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = '00112233445566778899aabbccddeeff', \
             cutover_expires_at = pg_catalog.clock_timestamp() - INTERVAL '1 second' \
         WHERE singleton",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let closed = sqlx::query_as::<_, WriterFenceRow>(
        "SELECT * FROM public.starring_runtime_writer_fence_observe_v1()",
    )
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(closed.0, "closed");
    assert_eq!(closed.1, 2);
    assert_eq!(
        closed.2.as_deref(),
        Some("00112233445566778899aabbccddeeff")
    );
    assert_eq!(closed.3, Some(1));
    assert!(closed.5.unwrap() <= closed.4);
    let RuntimeWriterFenceObservationV1::Closed(observed) =
        RuntimeWriterFenceObservationPortV1::observe_writer_fence(
            &adapter,
            RuntimeObserveWriterFenceV1,
        )
        .await
        .unwrap()
    else {
        panic!("expired writer fence must remain closed")
    };
    assert_eq!(observed.lease_id.generation.get(), 2);
    assert_eq!(observed.lease_id.lease_epoch.get(), 1);
    assert!(observed.current_lease().is_none());

    for statement in [
        "SELECT * FROM public.runtime_writer_fence",
        "UPDATE public.runtime_writer_fence SET fence_generation = fence_generation",
        "DELETE FROM public.runtime_writer_fence",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "42501");
    }
}
