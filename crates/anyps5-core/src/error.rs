//! Small shared pieces of the core: the event sink for worker threads.

use std::sync::Arc;

pub use crate::events::CancelToken;

/// Where a worker thread delivers its events. The app passes a closure that sends into its
/// channel and requests a repaint; tests pass one that pushes into a vector. Must not block.
pub type EventSink<T> = Arc<dyn Fn(T) + Send + Sync>;
