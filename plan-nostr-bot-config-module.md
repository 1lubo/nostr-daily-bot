# Implementation Plan: Rust Nostr Bot Configuration Module

## 1. Overview

### Description
A configuration module for a Rust Nostr bot that loads settings from a TOML file, validates configuration values, and supports environment variable overrides for sensitive data.

### Goals and Success Criteria
- ✅ Load configuration from `config.toml` with proper error handling
- ✅ Parse relay URLs, cron schedules, private key path/inline, and message templates
- ✅ Validate all configuration values (cron syntax, at least one relay, etc.)
- ✅ Support environment variable overrides for `NOSTR_PRIVATE_KEY`
- ✅ Provide sensible defaults for optional fields
- ✅ Teach idiomatic Rust patterns: derive macros, `Option`/`Result`, `?` operator

### Scope Boundaries
- **Included**: Config structs, loading, validation, env overrides
- **Excluded**: Actual Nostr client implementation, relay connections, message posting

---

## 2. Prerequisites

### Dependencies (Cargo.toml)
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
thiserror = "1.0"           # Idiomatic error handling
cron = "0.12"               # Cron expression parsing & validation
```

### Project Structure
```
src/
├── main.rs
├── config/
│   ├── mod.rs              # Module re-exports
│   ├── types.rs            # Struct definitions
│   ├── loader.rs           # Loading logic
│   └── validation.rs       # Validation functions
└── config.toml             # Example config (in project root)
```

---

## 3. Implementation Steps

### Step 1: Define Configuration Structs (`src/config/types.rs`)

**Description**: Define the configuration data structures with serde derive macros.

**Key Patterns to Teach**:
- `#[derive(Deserialize)]` for automatic TOML parsing
- `#[serde(default)]` for optional fields with defaults
- `Option<T>` for truly optional fields
- Newtype pattern for validated types

```rust
use serde::Deserialize;

/// Root configuration structure
#[derive(Debug, Deserialize)]
pub struct Config {
    pub relays: RelayConfig,
    pub schedule: ScheduleConfig,
    pub identity: IdentityConfig,
    pub messages: MessageConfig,
}

/// Relay connection settings
#[derive(Debug, Deserialize)]
pub struct RelayConfig {
    /// List of relay WebSocket URLs (required, at least one)
    pub urls: Vec<String>,
}

/// Posting schedule settings
#[derive(Debug, Deserialize)]
pub struct ScheduleConfig {
    /// Cron expression for posting schedule (e.g., "0 */6 * * *")
    pub cron: String,
    
    /// Timezone for cron interpretation (default: UTC)
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

/// Identity/key settings
#[derive(Debug, Deserialize)]
pub struct IdentityConfig {
    /// Path to file containing private key (nsec or hex)
    pub private_key_file: Option<String>,

    /// Inline private key (not recommended for production)
    /// Environment variable NOSTR_PRIVATE_KEY takes precedence
    pub private_key: Option<String>,
}

/// Message template settings
#[derive(Debug, Deserialize)]
pub struct MessageConfig {
    /// List of message templates to rotate through
    pub templates: Vec<String>,

    /// Rotation strategy: "sequential" or "random" (default: sequential)
    #[serde(default = "default_rotation")]
    pub rotation: String,
}

fn default_rotation() -> String {
    "sequential".to_string()
}
```

---

### Step 2: Define Custom Errors (`src/config/mod.rs` or `src/config/error.rs`)

**Description**: Create meaningful error types using `thiserror` for idiomatic error handling.

**Key Patterns to Teach**:
- `thiserror` derive macro for `std::error::Error` impl
- Wrapping underlying errors with context
- `Result<T, ConfigError>` as return type

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    FileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse TOML: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Invalid cron expression '{expr}': {reason}")]
    InvalidCron { expr: String, reason: String },

    #[error("No relays configured - at least one relay URL is required")]
    NoRelays,

    #[error("No private key configured - set via config file or NOSTR_PRIVATE_KEY env var")]
    NoPrivateKey,

    #[error("No message templates configured")]
    NoTemplates,

    #[error("Invalid relay URL '{url}': must start with ws:// or wss://")]
    InvalidRelayUrl { url: String },
}
```

---

### Step 3: Implement Config Loading (`src/config/loader.rs`)

**Description**: Load and parse the configuration file with environment variable overrides.

**Key Patterns to Teach**:
- The `?` operator for error propagation
- `std::fs::read_to_string` for file I/O
- `std::env::var` for environment variables
- Method chaining with `Option`

```rust
use std::fs;
use std::env;
use std::path::Path;

use super::types::Config;
use super::ConfigError;
use super::validation::validate_config;

impl Config {
    /// Load configuration from a TOML file with environment overrides
    ///
    /// # Arguments
    /// * `path` - Path to the config.toml file
    ///
    /// # Returns
    /// * `Ok(Config)` - Successfully loaded and validated config
    /// * `Err(ConfigError)` - Loading, parsing, or validation failed
    ///
    /// # Environment Variables
    /// * `NOSTR_PRIVATE_KEY` - Overrides identity.private_key if set
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();

        // Step 1: Read the file
        let contents = fs::read_to_string(path)
            .map_err(|e| ConfigError::FileRead {
                path: path.display().to_string(),
                source: e,
            })?;

        // Step 2: Parse TOML into our structs
        let mut config: Config = toml::from_str(&contents)?;

        // Step 3: Apply environment variable overrides
        config.apply_env_overrides();

        // Step 4: Validate the configuration
        validate_config(&config)?;

        Ok(config)
    }

    /// Apply environment variable overrides for sensitive values
    fn apply_env_overrides(&mut self) {
        // NOSTR_PRIVATE_KEY overrides any file-based key
        if let Ok(key) = env::var("NOSTR_PRIVATE_KEY") {
            self.identity.private_key = Some(key);
        }
    }

    /// Load configuration with a default path of "config.toml"
    pub fn load_default() -> Result<Self, ConfigError> {
        Self::load("config.toml")
    }
}
```

---

### Step 4: Implement Validation (`src/config/validation.rs`)

**Description**: Validate configuration values before use.

**Key Patterns to Teach**:
- Early returns with `?` for clean validation chains
- Using external crate (`cron`) for parsing validation
- Collecting multiple validation errors vs. fail-fast

```rust
use cron::Schedule;
use std::str::FromStr;

use super::types::Config;
use super::ConfigError;

/// Validate the entire configuration
///
/// Returns Ok(()) if valid, or the first validation error encountered.
pub fn validate_config(config: &Config) -> Result<(), ConfigError> {
    validate_relays(&config.relays.urls)?;
    validate_cron(&config.schedule.cron)?;
    validate_private_key(config)?;
    validate_templates(&config.messages.templates)?;
    Ok(())
}

/// Ensure at least one relay is configured and URLs are valid
fn validate_relays(urls: &[String]) -> Result<(), ConfigError> {
    if urls.is_empty() {
        return Err(ConfigError::NoRelays);
    }

    for url in urls {
        if !url.starts_with("ws://") && !url.starts_with("wss://") {
            return Err(ConfigError::InvalidRelayUrl {
                url: url.clone()
            });
        }
    }

    Ok(())
}

/// Validate cron expression using the cron crate
fn validate_cron(expr: &str) -> Result<(), ConfigError> {
    Schedule::from_str(expr)
        .map_err(|e| ConfigError::InvalidCron {
            expr: expr.to_string(),
            reason: e.to_string(),
        })?;

    Ok(())
}

/// Ensure a private key is available (file, inline, or env var)
fn validate_private_key(config: &Config) -> Result<(), ConfigError> {
    let has_key = config.identity.private_key.is_some()
        || config.identity.private_key_file.is_some();

    if !has_key {
        return Err(ConfigError::NoPrivateKey);
    }

    Ok(())
}

/// Ensure at least one message template is configured
fn validate_templates(templates: &[String]) -> Result<(), ConfigError> {
    if templates.is_empty() {
        return Err(ConfigError::NoTemplates);
    }

    Ok(())
}
```

---

### Step 5: Create Module Exports (`src/config/mod.rs`)

**Description**: Clean module organization with re-exports.

```rust
mod types;
mod loader;
mod validation;

// Re-export public types at the module level
pub use types::{Config, RelayConfig, ScheduleConfig, IdentityConfig, MessageConfig};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    // ... error variants from Step 2
}
```

---

### Step 6: Example Usage in `main.rs`

**Description**: Demonstrate how to use the config module.

```rust
mod config;

use config::{Config, ConfigError};

fn main() {
    // Using match for explicit error handling
    match Config::load("config.toml") {
        Ok(config) => {
            println!("Loaded configuration successfully!");
            println!("Relays: {:?}", config.relays.urls);
            println!("Schedule: {}", config.schedule.cron);
            println!("Templates: {} configured", config.messages.templates.len());

            // Start the bot with config...
        }
        Err(e) => {
            eprintln!("Failed to load configuration: {e}");
            std::process::exit(1);
        }
    }
}

// Alternative: Using ? operator in a fallible main
fn main() -> Result<(), ConfigError> {
    let config = Config::load("config.toml")?;

    println!("Bot starting with {} relay(s)", config.relays.urls.len());

    Ok(())
}
```

---

## 4. Example `config.toml`

Create this file in the project root:

```toml
# Nostr Bot Configuration
# =======================

[relays]
# List of relay WebSocket URLs (at least one required)
urls = [
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.nostr.band",
]

[schedule]
# Cron expression for posting schedule
# Format: sec min hour day_of_month month day_of_week year
# This example: every 6 hours at minute 0
cron = "0 0 */6 * * * *"

# Timezone for schedule (default: UTC)
timezone = "UTC"

[identity]
# Option 1: Path to file containing private key (nsec or hex format)
private_key_file = "~/.nostr/private.key"

# Option 2: Inline key (NOT recommended for production!)
# private_key = "nsec1..."

# Note: Environment variable NOSTR_PRIVATE_KEY overrides both options

[messages]
# Message templates to rotate through
# Use ${date} and ${time} for dynamic substitution (future feature)
templates = [
    "🏋️ Daily reminder: Consistency beats intensity. Show up today!",
    "💪 Progress is progress, no matter how small. Keep pushing!",
    "🎯 Set your intention for today's training. What's your focus?",
    "⚡ Energy flows where attention goes. Stay focused on your goals!",
]

# Rotation strategy: "sequential" or "random"
rotation = "sequential"
```

---

## 5. File Changes Summary

### Files to Create

| File | Purpose |
|------|---------|
| `src/config/mod.rs` | Module definition and error types |
| `src/config/types.rs` | Config struct definitions |
| `src/config/loader.rs` | File loading and env override logic |
| `src/config/validation.rs` | Validation functions |
| `config.toml` | Example configuration file |

### Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Add dependencies: serde, toml, thiserror, cron |
| `src/main.rs` | Add `mod config;` and usage |

---

## 6. Testing Strategy

### Unit Tests (`src/config/validation.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_relays_empty() {
        let result = validate_relays(&[]);
        assert!(matches!(result, Err(ConfigError::NoRelays)));
    }

    #[test]
    fn test_validate_relays_valid() {
        let urls = vec!["wss://relay.damus.io".to_string()];
        assert!(validate_relays(&urls).is_ok());
    }

    #[test]
    fn test_validate_relays_invalid_url() {
        let urls = vec!["http://invalid.com".to_string()];
        let result = validate_relays(&urls);
        assert!(matches!(result, Err(ConfigError::InvalidRelayUrl { .. })));
    }

    #[test]
    fn test_validate_cron_valid() {
        assert!(validate_cron("0 0 */6 * * * *").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        let result = validate_cron("not a cron");
        assert!(matches!(result, Err(ConfigError::InvalidCron { .. })));
    }
}
```

### Integration Test (`tests/config_integration.rs`)

```rust
use std::fs;
use std::env;
use tempfile::NamedTempFile;

#[test]
fn test_load_valid_config() {
    let config_content = r#"
        [relays]
        urls = ["wss://relay.damus.io"]

        [schedule]
        cron = "0 0 */6 * * * *"

        [identity]
        private_key = "test_key"

        [messages]
        templates = ["Hello, Nostr!"]
    "#;

    let tmp = NamedTempFile::new().unwrap();
    fs::write(tmp.path(), config_content).unwrap();

    let config = Config::load(tmp.path()).unwrap();
    assert_eq!(config.relays.urls.len(), 1);
}

#[test]
fn test_env_override() {
    env::set_var("NOSTR_PRIVATE_KEY", "env_secret_key");

    // Load config without inline key...
    // Assert that identity.private_key == "env_secret_key"

    env::remove_var("NOSTR_PRIVATE_KEY");
}
```

### Manual Testing Steps

1. Create `config.toml` with valid values → expect success
2. Remove `[relays]` section → expect `NoRelays` error
3. Set invalid cron expression → expect `InvalidCron` error
4. Set `NOSTR_PRIVATE_KEY` env var → verify override works
5. Use only `private_key_file` option → verify file-based key loading

---

## 7. Rollback Plan

### How to Revert
1. Remove the `src/config/` directory
2. Remove the `mod config;` line from `main.rs`
3. Remove dependencies from `Cargo.toml`
4. Run `cargo build` to verify clean state

### No Data Migrations Required
This is a pure code addition with no persistent state.

---

## 8. Estimated Effort

| Component | Time Estimate | Complexity |
|-----------|---------------|------------|
| Struct definitions | 30 min | Low |
| Error types | 20 min | Low |
| Loading logic | 45 min | Medium |
| Validation | 45 min | Medium |
| Unit tests | 45 min | Medium |
| Integration tests | 30 min | Medium |
| **Total** | **~3.5 hours** | **Medium** |

---

## 9. Idiomatic Rust Patterns Summary

### Pattern 1: Derive Macros
```rust
#[derive(Debug, Deserialize)]  // Automatic trait implementations
pub struct Config { ... }
```

### Pattern 2: The ? Operator
```rust
// Instead of:
let contents = match fs::read_to_string(path) {
    Ok(c) => c,
    Err(e) => return Err(ConfigError::from(e)),
};

// Write:
let contents = fs::read_to_string(path)?;
```

### Pattern 3: Option Handling
```rust
// Check if value exists
if config.identity.private_key.is_some() { ... }

// Get value or default
let key = config.identity.private_key.unwrap_or_default();

// Transform if present
let key = config.identity.private_key.as_ref().map(|k| k.trim());
```

### Pattern 4: Result Handling
```rust
// Propagate errors with ?
let config = Config::load("config.toml")?;

// Handle errors explicitly
match Config::load("config.toml") {
    Ok(c) => use_config(c),
    Err(e) => handle_error(e),
}
```

### Pattern 5: Default Values with Serde
```rust
#[serde(default = "default_timezone")]
pub timezone: String,

fn default_timezone() -> String {
    "UTC".to_string()
}
```

---

## 10. Next Steps After Implementation

1. **Add private key file reading** - Implement `load_private_key_from_file()`
2. **Add template variable substitution** - Support `${date}`, `${time}` placeholders
3. **Add config hot-reloading** - Watch for file changes and reload
4. **Add config schema validation** - Generate JSON schema for IDE support

