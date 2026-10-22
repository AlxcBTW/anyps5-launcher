//! Start-up timeline: `startup t=…ms <step>` lines in the log, measured from the first line of
//! `main`. Cheap enough to stay in release builds (a handful of lines per launch).

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Starts the clock; the first call wins.
pub fn begin() {
    let _ = START.set(Instant::now());
}

/// Logs how long the launch has taken up to `step`.
pub fn mark(step: &str) {
    if let Some(start) = START.get() {
        tracing::info!(target: "startup", "t={:.0}ms {step}", start.elapsed().as_secs_f64() * 1000.0);
    }
}
