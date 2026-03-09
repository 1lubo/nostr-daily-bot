//! Span utilities and operation ID generation.

use tracing::{info_span, Span};
use uuid::Uuid;

/// Generate a unique operation ID for tracing.
pub fn generate_operation_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

/// Create a span for application startup.
pub fn startup_span() -> Span {
    info_span!("startup", version = env!("CARGO_PKG_VERSION"))
}

/// Create a span for a posting operation.
pub fn posting_span(operation_id: &str) -> Span {
    info_span!("posting", operation_id = %operation_id)
}

/// Create a span for relay connection operations.
pub fn relay_connection_span(relay_url: &str) -> Span {
    info_span!("relay_connection", relay_url = %relay_url)
}

/// Create a span for scheduled job execution.
pub fn scheduled_job_span(job_id: &str, job_name: &str) -> Span {
    info_span!("scheduled_job", job_id = %job_id, job_name = %job_name)
}

