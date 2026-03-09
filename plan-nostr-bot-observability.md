# Implementation Plan: Observability Module for Rust Nostr Bot

## 1. Overview

### Description
A comprehensive observability module using the Rust `tracing` ecosystem that provides structured logging with configurable output formats (JSON for production, pretty-print for development), log level control via environment variables, spans for key operations, and request/operation IDs for tracing.

### Goals and Success Criteria
- ✅ Structured logging using `tracing` crate (similar concept to SLF4J/Logback)
- ✅ All log levels: ERROR, WARN, INFO, DEBUG, TRACE
- ✅ Spans for key operations (startup, posting, relay connection)
- ✅ JSON log format for production environments
- ✅ Pretty-print console format for development
- ✅ `EnvFilter` for log level control via `RUST_LOG` environment variable
- ✅ Request/operation IDs for tracing posting operations
- ✅ `#[instrument]` attribute for automatic span creation

### Scope Boundaries
- **Included**: Logging initialization, format switching, spans, structured fields, operation IDs
- **Excluded**: Metrics collection (Prometheus), distributed tracing (Jaeger/OpenTelemetry)

### Comparison to Java/SLF4J Patterns

| Java/SLF4J | Rust/tracing |
|------------|--------------|
| `Logger logger = LoggerFactory.getLogger(Class)` | `use tracing::{info, debug, ...}` (no per-class logger needed) |
| `logger.info("message")` | `info!("message")` |
| `logger.info("user: {}", userId)` | `info!(user_id = %user_id, "message")` |
| MDC (Mapped Diagnostic Context) | Spans with fields |
| Logback XML configuration | `tracing-subscriber` builder |
| `%X{requestId}` in pattern | `#[instrument(fields(request_id))]` |

---

## 2. Prerequisites

### Dependencies (Cargo.toml)
```toml
[dependencies]
tracing = "0.1"                           # Core instrumentation API
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }
uuid = { version = "1", features = ["v4"] }  # For operation IDs
```

### Project Structure
```
src/
├── main.rs
├── config/           # Existing
├── nostr/            # Existing
├── scheduler/        # Existing
└── observability/
    ├── mod.rs        # Module exports and initialization
    └── spans.rs      # Span utilities and operation ID generation
```

---

## 3. Implementation Steps

### Step 1: Add Dependencies to Cargo.toml

Add the tracing ecosystem dependencies:

```toml
[dependencies]
# ... existing dependencies ...

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }
uuid = { version = "1", features = ["v4"] }
```

---

### Step 2: Create Observability Configuration Types (`src/observability/mod.rs`)

**Description**: Define configuration for log format and level selection.

```rust
// src/observability/mod.rs
pub mod spans;

use tracing::Level;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter,
};

/// Log output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable, colorized output for development
    #[default]
    Pretty,
    /// JSON structured output for production (log aggregators)
    Json,
}

impl LogFormat {
    /// Parse from string (env var or config)
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            _ => LogFormat::Pretty,
        }
    }
}

/// Configuration for the observability/logging system
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Output format: Pretty (dev) or JSON (prod)
    pub format: LogFormat,
    /// Default log level (overridden by RUST_LOG env var)
    pub default_level: Level,
    /// Whether to include span events (enter/exit) - useful for debugging
    pub log_span_events: bool,
    /// Whether to include file/line in log output
    pub include_file_line: bool,
    /// Whether to include target (module path) in output
    pub include_target: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            default_level: Level::INFO,
            log_span_events: false,
            include_file_line: false,
            include_target: true,
        }
    }
}
```

---

### Step 3: Implement Logging Initialization

**Description**: Initialize tracing-subscriber with format switching based on config/environment.

**Key Pattern**: Use `tracing_subscriber::registry()` with layers for composable configuration.

```rust
// Continue in src/observability/mod.rs

impl ObservabilityConfig {
    /// Create config from environment variables
    ///
    /// Environment variables:
    /// - `LOG_FORMAT`: "json" or "pretty" (default: pretty)
    /// - `RUST_LOG`: Log level filter (e.g., "info", "nostr_bot=debug")
    pub fn from_env() -> Self {
        let format = std::env::var("LOG_FORMAT")
            .map(|s| LogFormat::from_str(&s))
            .unwrap_or_default();

        Self {
            format,
            ..Default::default()
        }
    }
}

/// Initialize the global tracing subscriber
///
/// Call this once at application startup, before any logging.
///
/// # Example
/// ```rust
/// use observability::{init_logging, ObservabilityConfig, LogFormat};
///
/// // Development
/// init_logging(ObservabilityConfig::default());
///
/// // Production (JSON)
/// init_logging(ObservabilityConfig {
///     format: LogFormat::Json,
///     ..Default::default()
/// });
///
/// // From environment
/// init_logging(ObservabilityConfig::from_env());
/// ```
pub fn init_logging(config: ObservabilityConfig) {
    // Build the EnvFilter with fallback to default level
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            EnvFilter::new(config.default_level.to_string())
        });

    // Determine span events to log
    let span_events = if config.log_span_events {
        FmtSpan::NEW | FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

    match config.format {
        LogFormat::Pretty => {
            // Human-readable format for development
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(span_events)
                .with_target(config.include_target)
                .with_file(config.include_file_line)
                .with_line_number(config.include_file_line)
                .with_thread_ids(false)
                .with_ansi(true)  // Color output
                .pretty()         // Multi-line, indented output
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }
        LogFormat::Json => {
            // JSON format for production log aggregators
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(span_events)
                .with_target(config.include_target)
                .with_file(config.include_file_line)
                .with_line_number(config.include_file_line)
                .json()           // JSON output
                .flatten_event(true)  // Flatten event fields into root
                .with_current_span(true)  // Include current span info
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .expect("Failed to set tracing subscriber");
        }
    }
}
```

---

### Step 4: Create Span Utilities and Operation IDs (`src/observability/spans.rs`)

**Description**: Utilities for creating operation IDs and common span patterns.

**Java Analogy**: This is similar to MDC (Mapped Diagnostic Context) in Logback, where you put a `requestId` into context and it appears in all log lines.

```rust
// src/observability/spans.rs
use tracing::{info_span, Span};
use uuid::Uuid;

/// Generate a unique operation ID for tracing
///
/// Format: 8-character hex string (short enough for logs, unique enough for tracing)
pub fn generate_operation_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

/// Create a span for application startup
pub fn startup_span() -> Span {
    info_span!(
        "app_startup",
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Create a span for a posting operation
///
/// The operation_id allows correlating all logs for a single posting attempt.
pub fn posting_span(operation_id: &str) -> Span {
    info_span!(
        "posting",
        operation_id = %operation_id,
    )
}

/// Create a span for relay connection operations
pub fn relay_connection_span(relay_url: &str) -> Span {
    info_span!(
        "relay_connection",
        relay_url = %relay_url,
    )
}

/// Create a span for scheduled job execution
pub fn scheduled_job_span(job_id: &str, job_name: &str) -> Span {
    info_span!(
        "scheduled_job",
        job_id = %job_id,
        job_name = %job_name,
    )
}
```

---

### Step 5: Using `#[instrument]` Attribute for Automatic Spans

**Description**: The `#[instrument]` attribute automatically creates spans for functions.

**Key Pattern**: This is like AOP (Aspect-Oriented Programming) in Java - you annotate a method and get automatic entry/exit logging.

```rust
// Example usage in src/nostr/mod.rs or any module

use tracing::{instrument, debug, info, warn, error};

impl NostrClient {
    /// Connect to all configured relays
    ///
    /// The #[instrument] attribute:
    /// - Creates a span named "connect" when function is called
    /// - Records the self.config.relays field value
    /// - Records function return value or error
    /// - Span automatically closes when function returns
    #[instrument(
        name = "nostr_connect",
        skip(self),  // Don't try to Debug-print self
        fields(relay_count = self.config.relays.len())
    )]
    pub async fn connect(&self) -> Result<()> {
        info!("Starting relay connections");

        for url in &self.config.relays {
            debug!(relay_url = %url, "Attempting connection");

            match self.client.add_relay(url).await {
                Ok(_) => info!(relay_url = %url, "Relay added successfully"),
                Err(e) => warn!(relay_url = %url, error = %e, "Failed to add relay"),
            }
        }

        let connected = self.connected_relay_count().await;
        info!(connected_relays = connected, "Connection phase complete");

        if connected == 0 {
            error!("No relays connected");
            return Err(NostrError::NoRelaysConnected);
        }

        Ok(())
    }

    /// Publish a text note with operation ID tracking
    #[instrument(
        name = "publish_text_note",
        skip(self, content),
        fields(
            operation_id = %operation_id,
            content_length = content.len()
        )
    )]
    pub async fn publish_text_note_traced(
        &self,
        content: &str,
        operation_id: &str,
    ) -> Result<EventId> {
        info!("Publishing text note");

        let builder = EventBuilder::text_note(content);
        let result = self.publish_event_builder(builder).await;

        match &result {
            Ok(event_id) => {
                info!(
                    event_id = %event_id.to_hex(),
                    "Text note published successfully"
                );
            }
            Err(e) => {
                error!(error = %e, "Failed to publish text note");
            }
        }

        result
    }
}
```

---

### Step 6: Structured Logging Examples

**Description**: How to add structured fields to log events (like SLF4J's `{}` placeholders but named).

```rust
use tracing::{trace, debug, info, warn, error, span, Level};

// Basic logging (like SLF4J)
fn logging_examples() {
    // Simple messages
    trace!("Very detailed trace message");
    debug!("Debug information");
    info!("Application started");
    warn!("Resource running low");
    error!("Operation failed");

    // With structured fields (key-value pairs)
    // This is superior to printf-style logging!
    let user_id = "usr_123";
    let relay_url = "wss://relay.damus.io";
    let latency_ms = 150;

    info!(
        user_id = %user_id,           // %user_id uses Display trait
        relay_url = %relay_url,
        "User connected to relay"
    );

    debug!(
        latency_ms = latency_ms,       // No % for Copy types
        relay_url = %relay_url,
        "Relay response time"
    );

    // With error context
    let error = std::io::Error::new(std::io::ErrorKind::Other, "connection reset");
    error!(
        error = %error,                // %error uses Display
        relay_url = %relay_url,
        "Connection failed"
    );

    // Dynamic log level (rare, but possible)
    tracing::event!(Level::INFO, user_id = %user_id, "Dynamic level log");
}

// Span usage (like MDC in Java)
async fn span_examples() {
    // Create a span for a logical operation
    let span = span!(Level::INFO, "process_message", message_id = "msg_456");
    let _guard = span.enter();  // Span active while _guard is in scope

    info!("Processing started");  // This log includes the span context

    // ... do work ...

    info!("Processing complete");  // Still includes span context

    // _guard dropped here, span ends
}

// Async-safe span usage with .instrument()
async fn async_span_example() {
    use tracing::Instrument;

    async fn do_work() {
        info!("Inside async work");
    }

    let span = span!(Level::INFO, "async_operation", op_id = "op_789");

    // .instrument() attaches span to the future
    do_work().instrument(span).await;
}
```

---

### Step 7: Example Log Output

**Pretty Format (Development)**:

```
  2024-01-15T10:30:00.123Z  INFO app_startup{version="0.1.0"}: nostr_bot: Application starting
  2024-01-15T10:30:00.124Z  INFO app_startup{version="0.1.0"}: nostr_bot::config: Loading configuration
    at src/config/loader.rs:25
  2024-01-15T10:30:00.125Z  INFO nostr_connect{relay_count=3}: nostr_bot::nostr: Starting relay connections
  2024-01-15T10:30:00.126Z DEBUG nostr_connect{relay_count=3}: nostr_bot::nostr: Attempting connection relay_url="wss://relay.damus.io"
  2024-01-15T10:30:00.500Z  INFO nostr_connect{relay_count=3}: nostr_bot::nostr: Relay added successfully relay_url="wss://relay.damus.io"
  2024-01-15T10:30:01.000Z  INFO nostr_connect{relay_count=3}: nostr_bot::nostr: Connection phase complete connected_relays=3
  2024-01-15T10:30:01.001Z  INFO posting{operation_id="a1b2c3d4"}: nostr_bot::scheduler: Executing scheduled posting
  2024-01-15T10:30:01.002Z  INFO publish_text_note{operation_id="a1b2c3d4" content_length=45}: nostr_bot::nostr: Publishing text note
  2024-01-15T10:30:01.150Z  INFO publish_text_note{operation_id="a1b2c3d4" content_length=45}: nostr_bot::nostr: Text note published successfully event_id="abc123..."
```

**JSON Format (Production)**:

```json
{"timestamp":"2024-01-15T10:30:00.123Z","level":"INFO","target":"nostr_bot","message":"Application starting","span":{"name":"app_startup"},"version":"0.1.0"}
{"timestamp":"2024-01-15T10:30:01.001Z","level":"INFO","target":"nostr_bot::scheduler","message":"Executing scheduled posting","span":{"name":"posting"},"operation_id":"a1b2c3d4"}
{"timestamp":"2024-01-15T10:30:01.150Z","level":"INFO","target":"nostr_bot::nostr","message":"Text note published successfully","span":{"name":"publish_text_note"},"operation_id":"a1b2c3d4","content_length":45,"event_id":"abc123..."}
```

---

### Step 8: Update main.rs with Observability

**Description**: Integrate observability into the application entry point.

```rust
// src/main.rs
mod config;
mod nostr;
mod scheduler;
mod observability;

use std::sync::Arc;
use tracing::{info, error, instrument};

use config::Config;
use nostr::NostrClient;
use observability::{init_logging, ObservabilityConfig, LogFormat, spans};
use scheduler::{Scheduler, SchedulerConfig, run_until_shutdown};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize logging FIRST (before any other code)
    let log_config = ObservabilityConfig::from_env();
    init_logging(log_config);

    // 2. Enter startup span
    let _startup_guard = spans::startup_span().entered();

    info!("Nostr Bot starting");

    // 3. Load configuration
    let config = load_config().await?;

    // 4. Initialize Nostr client
    let nostr_client = init_nostr_client(&config).await?;

    // 5. Setup and run scheduler
    run_scheduler(config, nostr_client).await?;

    info!("Application shutdown complete");
    Ok(())
}

#[instrument(name = "load_config")]
async fn load_config() -> anyhow::Result<Config> {
    info!("Loading configuration from config.toml");
    let config = Config::load("config.toml")?;
    info!(
        relay_count = config.relays.urls.len(),
        template_count = config.messages.templates.len(),
        "Configuration loaded successfully"
    );
    Ok(config)
}

#[instrument(name = "init_nostr", skip(config))]
async fn init_nostr_client(config: &Config) -> anyhow::Result<Arc<NostrClient>> {
    info!("Initializing Nostr client");

    let keys = NostrClient::keys_parse(&config.get_private_key()?)?;
    let client = NostrClient::with_keys(keys).await?;
    client.connect().await?;

    let connected = client.connected_relay_count().await;
    info!(connected_relays = connected, "Nostr client initialized");

    Ok(Arc::new(client))
}

#[instrument(name = "run_scheduler", skip_all)]
async fn run_scheduler(
    config: Config,
    nostr_client: Arc<NostrClient>,
) -> anyhow::Result<()> {
    info!("Setting up scheduler");

    let scheduler_config = SchedulerConfig {
        cron_expression: config.schedule.cron.clone(),
        timezone: config.schedule.timezone.clone(),
    };

    let mut scheduler = Scheduler::new(scheduler_config).await?;

    // Setup posting job with operation ID tracking
    let client_for_job = Arc::clone(&nostr_client);
    let templates = Arc::new(config.messages.templates.clone());
    let template_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    scheduler.register_posting_job(Arc::new(move || {
        let client = Arc::clone(&client_for_job);
        let templates = Arc::clone(&templates);
        let index = Arc::clone(&template_index);

        Box::pin(async move {
            // Generate unique operation ID for this posting
            let operation_id = spans::generate_operation_id();
            let _span = spans::posting_span(&operation_id).entered();

            let idx = index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let message = &templates[idx % templates.len()];

            info!(
                message_index = idx % templates.len(),
                "Executing scheduled post"
            );

            match client.publish_text_note(message).await {
                Ok(event_id) => {
                    info!(event_id = %event_id, "Post published successfully");
                }
                Err(e) => {
                    error!(error = %e, "Failed to publish post");
                }
            }
        })
    })).await?;

    info!("Scheduler configured, starting...");
    run_until_shutdown(scheduler).await?;

    Ok(())
}
```

---

### Step 9: Environment Variable Configuration

**Description**: Document the environment variables for controlling logging.

```bash
# Set log level (default: info)
# Options: error, warn, info, debug, trace
export RUST_LOG=info

# Module-specific levels
export RUST_LOG=warn,nostr_bot=debug,nostr_bot::nostr=trace

# Set output format (default: pretty)
# Options: pretty, json
export LOG_FORMAT=json

# Example: Production settings
export RUST_LOG=info
export LOG_FORMAT=json

# Example: Development/debugging
export RUST_LOG=debug,nostr_bot=trace
export LOG_FORMAT=pretty
```

---

## 4. File Changes Summary

### Files to Create

| File | Purpose |
|------|---------|
| `src/observability/mod.rs` | Configuration types and logging initialization |
| `src/observability/spans.rs` | Span utilities and operation ID generation |

### Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Add: `tracing`, `tracing-subscriber` with features, `uuid` |
| `src/main.rs` | Add `mod observability;`, call `init_logging()`, add spans |
| `src/nostr/mod.rs` | Add `#[instrument]` attributes to key methods |
| `src/scheduler/mod.rs` | Add operation ID tracking to job execution |

---

## 5. Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_format_parsing() {
        assert_eq!(LogFormat::from_str("json"), LogFormat::Json);
        assert_eq!(LogFormat::from_str("JSON"), LogFormat::Json);
        assert_eq!(LogFormat::from_str("pretty"), LogFormat::Pretty);
        assert_eq!(LogFormat::from_str("anything_else"), LogFormat::Pretty);
    }

    #[test]
    fn test_operation_id_generation() {
        let id1 = spans::generate_operation_id();
        let id2 = spans::generate_operation_id();

        assert_eq!(id1.len(), 8);
        assert_eq!(id2.len(), 8);
        assert_ne!(id1, id2);  // Should be unique
    }

    #[test]
    fn test_default_config() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.format, LogFormat::Pretty);
        assert_eq!(config.default_level, Level::INFO);
    }
}
```

### Integration Test (Log Capture)

```rust
use tracing_subscriber::fmt::MakeWriter;
use std::sync::{Arc, Mutex};

/// Test writer that captures log output
struct TestWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl std::io::Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

#[test]
fn test_json_output_contains_operation_id() {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = TestWriter { buffer: buffer.clone() };

    // Setup JSON subscriber with test writer
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer.clone())
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("test", operation_id = "test123");
        let _guard = span.enter();
        tracing::info!("Test message");
    });

    let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(output.contains("operation_id"));
    assert!(output.contains("test123"));
}
```

### Manual Testing Steps

1. **Pretty format test**:
   ```bash
   LOG_FORMAT=pretty RUST_LOG=debug cargo run
   # Verify colorized, multi-line output
   ```

2. **JSON format test**:
   ```bash
   LOG_FORMAT=json RUST_LOG=info cargo run
   # Verify single-line JSON output
   # Pipe to `jq` to verify valid JSON: cargo run 2>&1 | jq
   ```

3. **Log level filtering**:
   ```bash
   RUST_LOG=error cargo run  # Should only show errors
   RUST_LOG=trace cargo run  # Should show everything
   ```

4. **Module-specific levels**:
   ```bash
   RUST_LOG=warn,nostr_bot::nostr=debug cargo run
   # Should show debug for nostr module, warn for others
   ```

---

## 6. Rollback Plan

### How to Revert

1. Remove the `src/observability/` directory
2. Remove `mod observability;` from `main.rs`
3. Remove observability dependencies from `Cargo.toml`
4. Remove `#[instrument]` attributes from functions
5. Replace `tracing::*` macros with `println!` or remove logging
6. Run `cargo build` to verify clean state

### No Data Migrations Required

This is a pure code addition with no persistent state.

---

## 7. Estimated Effort

| Component | Time Estimate | Complexity |
|-----------|---------------|------------|
| Dependencies and config types | 20 min | Low |
| Logging initialization | 30 min | Low |
| Span utilities | 20 min | Low |
| main.rs integration | 30 min | Low |
| Adding #[instrument] to existing code | 45 min | Low |
| Unit tests | 30 min | Low |
| Integration tests | 30 min | Medium |
| Documentation and examples | 30 min | Low |
| **Total** | **~3.5 hours** | **Low-Medium** |

---

## 8. Quick Reference: tracing Macros

```rust
// Log levels (from most to least verbose)
trace!("Very detailed debug info");
debug!("Debug information");
info!("General information");
warn!("Warning conditions");
error!("Error conditions");

// With structured fields
info!(field = "value", number = 42, "Message");
info!(field = %display_value, "Using Display trait");
info!(field = ?debug_value, "Using Debug trait");

// Spans
let span = span!(Level::INFO, "span_name", field = "value");
let _guard = span.enter();

// #[instrument] attribute
#[instrument(skip(self), fields(custom = "value"))]
fn my_function(&self, arg: &str) -> Result<()> { ... }
```

---

## 9. Next Steps After Implementation

1. **Add metrics** - Integrate `tracing-opentelemetry` for metrics export
2. **Add distributed tracing** - Export to Jaeger/Zipkin for request tracing
3. **Add log rotation** - Use `tracing-appender` for file-based logging with rotation
4. **Add sampling** - Implement trace sampling for high-volume scenarios
5. **Add custom layers** - Create custom tracing layers for specific needs (e.g., alerting)
