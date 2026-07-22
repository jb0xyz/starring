use std::future::{ready, Future};

use automation_runtime_controller::{
    RuntimeObserveWriterFenceV1, RuntimeWriterFenceGenerationV1, RuntimeWriterFenceObservationV1,
};
use automation_runtime_worker::RuntimeWriterFenceObservationPortV1;
use chrono::{DateTime, Utc};

struct FakeWriterFenceObservationPort;

impl RuntimeWriterFenceObservationPortV1 for FakeWriterFenceObservationPort {
    type Error = &'static str;

    fn observe_writer_fence(
        &self,
        _request: RuntimeObserveWriterFenceV1,
    ) -> impl Future<Output = Result<RuntimeWriterFenceObservationV1, Self::Error>> + Send {
        ready(Ok(RuntimeWriterFenceObservationV1::Open {
            generation: RuntimeWriterFenceGenerationV1::new(std::num::NonZeroU64::new(1).unwrap()),
            observed_database_now: at(100),
        }))
    }
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

#[test]
fn writer_fence_port_is_pure_and_observe_only() {
    fn assert_port<T: RuntimeWriterFenceObservationPortV1>() {}
    assert_port::<FakeWriterFenceObservationPort>();

    let pending = FakeWriterFenceObservationPort.observe_writer_fence(RuntimeObserveWriterFenceV1);
    std::mem::drop(pending);
}
