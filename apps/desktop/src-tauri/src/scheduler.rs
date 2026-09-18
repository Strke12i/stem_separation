//! Conservative shared resource gates for heavyweight local inference.

use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("GPU scheduler is unavailable")]
    Unavailable,
}

/// One GPU-heavy inference runs at a time. CPU-side worker serialization remains
/// owned by each worker manager, so this gate only coordinates separate sidecars.
pub struct ResourceScheduler {
    gpu: Arc<Semaphore>,
}

impl ResourceScheduler {
    #[must_use]
    pub fn new() -> Self {
        Self {
            gpu: Arc::new(Semaphore::new(1)),
        }
    }

    pub async fn acquire_gpu(&self) -> Result<OwnedSemaphorePermit, SchedulerError> {
        self.gpu
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| SchedulerError::Unavailable)
    }
}

impl Default for ResourceScheduler {
    fn default() -> Self {
        Self::new()
    }
}
