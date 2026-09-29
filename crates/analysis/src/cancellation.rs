//! Cooperative cancellation shared by long-running analysis algorithms.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use archaeodash_domain::DomainError;

/// Cloneable flag checked at bounded intervals by analysis loops.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<TokenState>);

#[derive(Debug, Default)]
struct TokenState {
    cancelled: AtomicBool,
    deadline: Mutex<Option<Instant>>,
}

impl CancellationToken {
    /// Creates a token whose cancellation state can be shared with a worker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Repeated calls are harmless.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
    }

    /// Sets an absolute monotonic deadline shared with all token clones.
    pub fn set_deadline(&self, deadline: Instant) {
        if let Ok(mut current) = self.0.deadline.lock() {
            *current = Some(deadline);
        }
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        let expired = self
            .0
            .deadline
            .lock()
            .ok()
            .and_then(|d| *d)
            .is_some_and(|d| Instant::now() >= d);
        if expired {
            self.cancel();
        }
        self.0.cancelled.load(Ordering::Acquire)
    }

    /// Stops a cooperative algorithm at its next checkpoint.
    pub fn check(&self) -> Result<(), DomainError> {
        if self.is_cancelled() {
            Err(DomainError::validation(
                "analysis_cancelled",
                "analysis cancellation requested",
            ))
        } else {
            Ok(())
        }
    }
}
