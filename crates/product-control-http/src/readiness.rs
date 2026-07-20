use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use thiserror::Error;

const UNCLAIMED_UNREADY: u8 = 0;
const CLAIMED_UNREADY: u8 = 1;
const CLAIMED_READY: u8 = 2;
const IMMUTABLE_READY: u8 = 3;

#[derive(Clone, Debug)]
pub struct ProductApiReadinessGate {
    state: Arc<AtomicU8>,
}

impl ProductApiReadinessGate {
    pub fn initially_unready() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(UNCLAIMED_UNREADY)),
        }
    }

    pub fn claim(&self) -> Result<ProductApiReadinessLeaseV1, ProductApiReadinessClaimErrorV1> {
        match self.state.compare_exchange(
            UNCLAIMED_UNREADY,
            CLAIMED_UNREADY,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(ProductApiReadinessLeaseV1 {
                state: self.state.clone(),
            }),
            Err(CLAIMED_UNREADY) => Err(ProductApiReadinessClaimErrorV1::AlreadyClaimed),
            Err(CLAIMED_READY | IMMUTABLE_READY) => {
                Err(ProductApiReadinessClaimErrorV1::AlreadyReady)
            }
            Err(_) => Err(ProductApiReadinessClaimErrorV1::InvalidState),
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(
            self.state.load(Ordering::Acquire),
            CLAIMED_READY | IMMUTABLE_READY
        )
    }

    pub(crate) fn always_ready() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(IMMUTABLE_READY)),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ProductApiReadinessClaimErrorV1 {
    #[error("the readiness gate already has an owner")]
    AlreadyClaimed,
    #[error("the readiness gate is already ready")]
    AlreadyReady,
    #[error("the readiness gate is in an invalid state")]
    InvalidState,
}

#[derive(Debug)]
pub struct ProductApiReadinessLeaseV1 {
    state: Arc<AtomicU8>,
}

impl ProductApiReadinessLeaseV1 {
    pub fn mark_ready(&self) {
        self.set_claimed_state(CLAIMED_READY);
    }

    pub fn mark_unready(&self) {
        self.set_claimed_state(CLAIMED_UNREADY);
    }

    fn set_claimed_state(&self, next: u8) {
        let mut current = self.state.load(Ordering::Acquire);
        while matches!(current, CLAIMED_UNREADY | CLAIMED_READY) {
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for ProductApiReadinessLeaseV1 {
    fn drop(&mut self) {
        let mut current = self.state.load(Ordering::Acquire);
        while matches!(current, CLAIMED_UNREADY | CLAIMED_READY) {
            match self.state.compare_exchange_weak(
                current,
                UNCLAIMED_UNREADY,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_exclusively_controls_readiness_and_drop_releases_claim() {
        let gate = ProductApiReadinessGate::initially_unready();
        let lease = gate.claim().unwrap();
        assert!(!gate.is_ready());
        assert!(matches!(
            gate.claim(),
            Err(ProductApiReadinessClaimErrorV1::AlreadyClaimed)
        ));

        lease.mark_ready();
        assert!(gate.is_ready());
        assert!(matches!(
            gate.claim(),
            Err(ProductApiReadinessClaimErrorV1::AlreadyReady)
        ));

        lease.mark_unready();
        assert!(!gate.is_ready());
        drop(lease);

        let replacement = gate.claim().unwrap();
        replacement.mark_ready();
        assert!(gate.is_ready());
        drop(replacement);
        assert!(!gate.is_ready());
    }

    #[test]
    fn immutable_ready_gate_rejects_claim_without_losing_readiness() {
        let gate = ProductApiReadinessGate::always_ready();
        assert!(matches!(
            gate.claim(),
            Err(ProductApiReadinessClaimErrorV1::AlreadyReady)
        ));
        assert!(gate.is_ready());
    }
}
