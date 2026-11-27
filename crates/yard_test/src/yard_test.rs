//! Headless application and logging helpers for extension tests.

pub mod harness;
pub mod logs;

pub use harness::{TestApp, TestAppBuilder};
pub use logs::{DiagnosticRecord, Trace, TraceGroup, TraceLine, capture_logs, capture_trace};
