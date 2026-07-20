use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ProductApiReadinessGate {
    ready: Arc<AtomicBool>,
}

impl ProductApiReadinessGate {
    pub fn initially_unready() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn mark_unready(&self) {
        self.ready.store(false, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub(crate) fn always_ready() -> Self {
        let gate = Self::initially_unready();
        gate.mark_ready();
        gate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_transition_is_explicit_and_reversible() {
        let gate = ProductApiReadinessGate::initially_unready();
        assert!(!gate.is_ready());
        gate.mark_ready();
        assert!(gate.is_ready());
        gate.mark_unready();
        assert!(!gate.is_ready());
    }
}
