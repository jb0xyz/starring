use std::future::Future;

use automation_runtime_controller::{RuntimeObserveWriterFenceV1, RuntimeWriterFenceObservationV1};

pub trait RuntimeWriterFenceObservationPortV1 {
    type Error;

    fn observe_writer_fence(
        &self,
        request: RuntimeObserveWriterFenceV1,
    ) -> impl Future<Output = Result<RuntimeWriterFenceObservationV1, Self::Error>> + Send;
}
