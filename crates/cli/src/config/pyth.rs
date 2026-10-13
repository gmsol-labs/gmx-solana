//! Pyth Hermes settings for CLI commands that fetch Pyth price updates.
//!
//! Owns: the `[pyth]` config section and resolving its API key against the
//! `PYTH_API_KEY` env var. Does NOT own: building the client (see
//! `Config::hermes` and `Config::optional_hermes`).
//!
//! Sources, highest priority first:
//!   - `[pyth] api_key` in the config file, or `GMSOL_PYTH__API_KEY`
//!   - `PYTH_API_KEY`
//!
//! An empty string counts as unset, so `PYTH_API_KEY=` does not produce an
//! authenticated client with a blank key.

use std::fmt;

/// Error message for a Pyth operation that runs without an API key.
pub(crate) const MISSING_PYTH_API_KEY: &str = "Pyth Hermes API key is not configured: \
    set `api_key` under `[pyth]` in the config file, or set `GMSOL_PYTH__API_KEY` \
    or `PYTH_API_KEY`";

/// `[pyth]` config section. Loaded from the config file and `GMSOL_PYTH__*` env vars.
#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct PythConfig {
    /// Hermes base URL. Defaults to the public Hermes endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Hermes API key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

// Manual impl so the API key never reaches logs.
impl fmt::Debug for PythConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PythConfig")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

/// Picks the API key: config value first, then env value. Empty strings are skipped.
///
/// ```ignore
/// assert_eq!(resolve_api_key(Some("cfg"), Some("env")), Some("cfg".to_string()));
/// assert_eq!(resolve_api_key(Some(""), Some("env")), Some("env".to_string()));
/// assert_eq!(resolve_api_key(None, Some("")), None);
/// ```
pub(crate) fn resolve_api_key(config_key: Option<&str>, env_key: Option<&str>) -> Option<String> {
    [config_key, env_key]
        .into_iter()
        .flatten()
        .find(|key| !key.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_key_wins_over_env_key() {
        assert_eq!(
            resolve_api_key(Some("cfg"), Some("env")),
            Some("cfg".to_string())
        );
    }

    #[test]
    fn env_key_is_used_when_config_key_is_missing() {
        assert_eq!(resolve_api_key(None, Some("env")), Some("env".to_string()));
    }

    #[test]
    fn empty_keys_count_as_unset() {
        assert_eq!(
            resolve_api_key(Some(""), Some("env")),
            Some("env".to_string())
        );
        assert_eq!(resolve_api_key(Some(""), Some("")), None);
        assert_eq!(resolve_api_key(None, None), None);
    }

    #[test]
    fn debug_redacts_api_key() {
        let config = PythConfig {
            endpoint: Some("https://hermes.example".to_string()),
            api_key: Some("secret-key".to_string()),
        };
        let printed = format!("{config:?}");
        assert!(!printed.contains("secret-key"));
        assert!(printed.contains("[redacted]"));
        assert!(printed.contains("https://hermes.example"));
    }

    #[test]
    fn missing_key_message_names_every_source() {
        for source in ["[pyth]", "GMSOL_PYTH__API_KEY", "PYTH_API_KEY"] {
            assert!(MISSING_PYTH_API_KEY.contains(source), "missing {source}");
        }
    }
}
