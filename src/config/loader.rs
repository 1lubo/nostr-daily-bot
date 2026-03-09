//! Configuration loading logic.

use std::env;
use std::fs;
use std::path::Path;

use super::types::Config;
use super::validation::validate_config;
use super::ConfigError;

impl Config {
    /// Load configuration from a TOML file with environment overrides.
    ///
    /// # Arguments
    /// * `path` - Path to the config.toml file
    ///
    /// # Environment Variables
    /// * `NOSTR_PRIVATE_KEY` - Overrides identity.private_key if set
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();

        // Read the file
        let contents = fs::read_to_string(path).map_err(|e| ConfigError::FileRead {
            path: path.display().to_string(),
            source: e,
        })?;

        // Parse TOML
        let mut config: Config = toml::from_str(&contents)?;

        // Apply environment variable overrides
        config.apply_env_overrides();

        // Validate
        validate_config(&config)?;

        Ok(config)
    }

    /// Apply environment variable overrides for sensitive values.
    fn apply_env_overrides(&mut self) {
        if let Ok(key) = env::var("NOSTR_PRIVATE_KEY") {
            self.identity.private_key = Some(key);
        }
    }

    /// Get the private key from config or environment.
    ///
    /// Priority:
    /// 1. Environment variable NOSTR_PRIVATE_KEY
    /// 2. Inline private_key in config
    /// 3. Read from private_key_file
    pub fn get_private_key(&self) -> Result<String, ConfigError> {
        // Check inline key first (may have been set by env override)
        if let Some(ref key) = self.identity.private_key {
            return Ok(key.clone());
        }

        // Try to read from file
        if let Some(ref file_path) = self.identity.private_key_file {
            let expanded = shellexpand::tilde(file_path);
            let contents = fs::read_to_string(expanded.as_ref()).map_err(|e| {
                ConfigError::FileRead {
                    path: file_path.clone(),
                    source: e,
                }
            })?;
            return Ok(contents.trim().to_string());
        }

        Err(ConfigError::NoPrivateKey)
    }
}

