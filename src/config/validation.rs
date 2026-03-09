//! Configuration validation functions.

use cron::Schedule;
use std::str::FromStr;

use super::types::Config;
use super::ConfigError;

/// Validate the entire configuration.
pub fn validate_config(config: &Config) -> Result<(), ConfigError> {
    validate_relays(&config.relays.urls)?;
    validate_cron(&config.schedule.cron)?;
    validate_private_key(config)?;
    validate_templates(&config.messages.templates)?;
    Ok(())
}

/// Ensure at least one relay is configured and URLs are valid.
fn validate_relays(urls: &[String]) -> Result<(), ConfigError> {
    if urls.is_empty() {
        return Err(ConfigError::NoRelays);
    }

    for url in urls {
        if !url.starts_with("ws://") && !url.starts_with("wss://") {
            return Err(ConfigError::InvalidRelayUrl { url: url.clone() });
        }
    }

    Ok(())
}

/// Validate cron expression using the cron crate.
fn validate_cron(expr: &str) -> Result<(), ConfigError> {
    Schedule::from_str(expr).map_err(|e| ConfigError::InvalidCron {
        expr: expr.to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
}

/// Ensure a private key is available (file, inline, or env var).
fn validate_private_key(config: &Config) -> Result<(), ConfigError> {
    let has_key =
        config.identity.private_key.is_some() || config.identity.private_key_file.is_some();

    if !has_key {
        return Err(ConfigError::NoPrivateKey);
    }

    Ok(())
}

/// Ensure at least one message template is configured.
fn validate_templates(templates: &[String]) -> Result<(), ConfigError> {
    if templates.is_empty() {
        return Err(ConfigError::NoTemplates);
    }

    Ok(())
}

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
        assert!(validate_cron("0 0 */6 * * *").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        let result = validate_cron("not a cron");
        assert!(matches!(result, Err(ConfigError::InvalidCron { .. })));
    }

    #[test]
    fn test_validate_templates_empty() {
        let result = validate_templates(&[]);
        assert!(matches!(result, Err(ConfigError::NoTemplates)));
    }

    #[test]
    fn test_validate_templates_valid() {
        let templates = vec!["Hello!".to_string()];
        assert!(validate_templates(&templates).is_ok());
    }
}

