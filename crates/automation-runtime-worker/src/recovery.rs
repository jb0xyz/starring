#[must_use]
pub struct RuntimeRecoveryPendingV2<E, R> {
    pub source: E,
    pub recovery: R,
}
