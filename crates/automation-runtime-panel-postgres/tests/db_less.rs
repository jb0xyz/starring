use std::time::Duration;

use automation_runtime_panel_postgres::{
    RuntimePanelErrorClassV1, RuntimePanelPersistenceErrorV1, MAX_RUNTIME_PANEL_LEASE_HEADROOM,
};

#[test]
fn stable_error_classes_keep_authority_and_ownership_distinct() {
    let cases = [
        (
            RuntimePanelPersistenceErrorV1::OwnershipLost,
            RuntimePanelErrorClassV1::OwnershipLost,
        ),
        (
            RuntimePanelPersistenceErrorV1::AuthorityChanged,
            RuntimePanelErrorClassV1::AuthorityChanged,
        ),
        (
            RuntimePanelPersistenceErrorV1::Conflict,
            RuntimePanelErrorClassV1::Conflict,
        ),
        (
            RuntimePanelPersistenceErrorV1::Indeterminate,
            RuntimePanelErrorClassV1::Indeterminate,
        ),
    ];
    for (error, class) in cases {
        assert_eq!(error.class(), class);
        assert!(error.to_string().len() <= 128);
    }
}

#[test]
fn external_call_headroom_has_a_small_fixed_upper_bound() {
    assert_eq!(MAX_RUNTIME_PANEL_LEASE_HEADROOM, Duration::from_secs(30));
    assert!(MAX_RUNTIME_PANEL_LEASE_HEADROOM < Duration::from_secs(60));
}

#[test]
fn fenced_store_is_send_and_sync_without_clone_requirement() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<automation_runtime_panel_postgres::PostgresFencedStrictPanelStoreV1>();
}
