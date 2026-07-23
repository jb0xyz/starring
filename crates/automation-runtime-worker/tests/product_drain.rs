use automation_runtime_controller::{
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
};
use automation_runtime_worker::RuntimeProductDrainObservationPortV2;
use automation_runtime_worker::{
    RuntimeProductDrainRecoveryOutcomeV2, RuntimeProductDrainUnknownRecoveryPortV2,
    RuntimeRecoveryPendingV2,
};

fn assert_port<T>()
where
    T: RuntimeProductDrainObservationPortV2<Error = ()>,
{
}

fn assert_signature<T>(port: &T, lookup: RuntimeProductDrainScopeLookupV2)
where
    T: RuntimeProductDrainObservationPortV2<Error = ()>,
{
    let future = port.observe_product_drain_scope(lookup);
    std::mem::drop(future);
}

#[test]
fn product_drain_observation_port_is_scope_only() {
    let _ = assert_port::<NeverPort>;
    let _ = assert_signature::<NeverPort>;
}

fn assert_recovery_port<T>()
where
    T: RuntimeProductDrainUnknownRecoveryPortV2<Error = (), TransactionEnded = Ended>,
{
}

fn assert_recovery_signature<T>(port: T)
where
    T: RuntimeProductDrainUnknownRecoveryPortV2<Error = (), TransactionEnded = Ended>,
{
    let _ = port.lookup();
    let future = port.quiesce_and_observe(std::time::Duration::from_secs(1));
    std::mem::drop(future);
}

#[test]
fn product_drain_unknown_recovery_is_handle_only_and_resumable() {
    let _ = assert_recovery_port::<NeverRecovery>;
    let _ = assert_recovery_signature::<NeverRecovery>;
    let _ = assert_outcome_carrier;
}

#[test]
fn recovery_pending_preserves_owned_error_and_handle() {
    let pending = RuntimeRecoveryPendingV2 {
        source: "timeout",
        recovery: "quarantined-handle",
    };
    let RuntimeRecoveryPendingV2 { source, recovery } = pending;
    assert_eq!(source, "timeout");
    assert_eq!(recovery, "quarantined-handle");
}

struct NeverPort;

impl RuntimeProductDrainObservationPortV2 for NeverPort {
    type Error = ();

    async fn observe_product_drain_scope(
        &self,
        _lookup: RuntimeProductDrainScopeLookupV2,
    ) -> Result<RuntimeProductDrainScopeObservationV2, Self::Error> {
        std::future::pending().await
    }
}

struct Ended;

struct NeverRecovery(RuntimeProductDrainScopeLookupV2);

impl RuntimeProductDrainUnknownRecoveryPortV2 for NeverRecovery {
    type Error = ();
    type TransactionEnded = Ended;

    fn lookup(&self) -> &RuntimeProductDrainScopeLookupV2 {
        &self.0
    }

    fn quiesce_and_observe(
        self,
        _timeout: std::time::Duration,
    ) -> impl std::future::Future<
        Output = Result<
            RuntimeProductDrainRecoveryOutcomeV2<Self::TransactionEnded>,
            RuntimeRecoveryPendingV2<Self::Error, Self>,
        >,
    > + Send {
        std::future::pending()
    }
}

fn assert_outcome_carrier(
    observation: RuntimeProductDrainScopeObservationV2,
    transaction_ended: Ended,
) -> (Ended, RuntimeProductDrainScopeObservationV2) {
    let outcome = RuntimeProductDrainRecoveryOutcomeV2 {
        transaction_ended,
        observation,
    };
    let RuntimeProductDrainRecoveryOutcomeV2 {
        transaction_ended,
        observation,
    } = outcome;
    (transaction_ended, observation)
}
