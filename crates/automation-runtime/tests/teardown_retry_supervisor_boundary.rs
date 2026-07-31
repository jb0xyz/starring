fn production_prefix(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap()
}

fn declaration<'a>(source: &'a str, marker: &str, next: &str) -> &'a str {
    source
        .split(marker)
        .nth(1)
        .unwrap()
        .split(next)
        .next()
        .unwrap()
}

#[test]
fn retry_supervisor_is_pure_narrow_and_statically_bounded() {
    let source = production_prefix(include_str!("../src/teardown_retry_supervisor.rs"));
    for required in [
        "pub trait InstanceTeardownRetrySupervisorPortV1",
        "InstanceTeardownRetryScanRequestV1",
        "InstanceTeardownRetryExecutionRequestV1",
        "MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2",
        "MAX_TEARDOWN_RETRY_CONCURRENCY_V1",
        "MAX_TEARDOWN_RETRY_CADENCE_V1",
        "MAX_TEARDOWN_RETRY_SCAN_TIMEOUT_V1",
        "MAX_TEARDOWN_RETRY_INSTANCE_TIMEOUT_V1",
        "config.scan_timeout",
        "buffer_unordered(config.max_concurrency.get())",
        "timeout(",
        "timeout_at(TokioInstant::from_std(deadline), &mut task)",
        "task.abort()",
        "InstanceTeardownRetrySupervisorV1(<redacted>)",
    ] {
        assert!(source.contains(required), "{required}");
    }
    for forbidden in [
        "sqlx",
        "Postgres",
        "twilight",
        "Client",
        "InstanceStoreV1",
        "list_by_guild",
        "list_deleting",
        "transition_to_deleting",
        "mark_deleted",
        "tokio::task::spawn_blocking",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn retry_supervisor_module_is_private_with_deliberate_reexports() {
    let library = include_str!("../src/lib.rs");
    assert!(library.contains("mod teardown_retry_supervisor;"));
    assert!(!library.contains("pub mod teardown_retry_supervisor;"));
    assert!(library.contains("pub use teardown_retry_supervisor::{"));
    assert!(library.contains("InstanceTeardownRetrySupervisorV1,"));
    assert!(library.contains("InstanceTeardownRetrySupervisorPortV1,"));
}

#[test]
fn scan_cursor_survives_errors_and_resets_only_after_a_terminal_page() {
    let source = production_prefix(include_str!("../src/teardown_retry_supervisor.rs"));
    let run = declaration(
        source,
        "async fn run_instance_teardown_retry_supervisor_v1",
        "async fn run_teardown_retry_page_v1",
    );
    let scan = run.find(".scan_retryable_v1(").unwrap();
    let page_match = run.find("match page").unwrap();
    let next = run.find(".next_cursor_v2()").unwrap();
    let reset = run
        .find(".unwrap_or_else(InstanceTeardownRetryScanCursorV2::initial)")
        .unwrap();
    let assign = run.find("cursor = next_cursor").unwrap();
    let scan_failure = run
        .find("Ok(Err(_)) => increment(&mut progress.scan_failed)")
        .unwrap();
    let scan_timeout = run.find("increment(&mut progress.scan_timed_out)").unwrap();
    let cadence = run.find("sleep(config.cadence)").unwrap();
    assert!(scan < page_match);
    assert!(page_match < next);
    assert!(next < reset);
    assert!(reset < assign);
    assert!(assign < scan_failure);
    assert!(scan_failure < scan_timeout);
    assert!(scan_timeout < cadence);
    let before_scan = &run[..scan];
    assert!(!before_scan.contains("sleep(config.cadence)"));
}

#[test]
fn dispatch_and_retry_share_one_owned_teardown_and_http_authority() {
    let source = production_prefix(include_str!("../src/shared_gateway_dispatcher.rs"));
    let dispatch = declaration(
        source,
        "pub async fn dispatch_v3(",
        "pub async fn acknowledge_rejection_v3(",
    );
    let retry = declaration(source, "pub async fn retry_teardown_v1(", "impl<I> Debug");
    assert!(source.contains("teardown: Arc<Teardown<I, OwnedTwilightInstanceDeleter>>"));
    assert_eq!(
        source
            .matches("let teardown = Arc::new(Teardown::new(")
            .count(),
        1
    );
    assert!(dispatch.contains("self.teardown.as_ref()"));
    assert!(retry.contains("self.teardown.teardown(guild_id, instance_id).await"));
    assert!(!retry.contains("Client::builder"));
    assert!(!retry.contains("Teardown::new"));
}
