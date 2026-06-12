//! Configuration: load/validate `.codecache/config.toml`.
//!
//! API anchor: `project_plan.md` §7.3. Owner: `principal-engineering-lead`. Scenarios:
//! `docs/TEST_STRATEGY.md#config`. Implemented at M1.
//!
//! `Config` mirrors the §7.3 schema. Omitted fields fall back to the documented defaults
//! (§6/§7.3) via `#[serde(default = ...)]`. `load` returns a typed [`ConfigError`] for a missing
//! file, an unreadable file, or malformed TOML — never a panic.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::Language;

/// Top-level configuration, mirroring `.codecache/config.toml` (`project_plan.md` §7.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Config schema version string.
    #[serde(default = "default_version")]
    pub version: String,
    /// Paths to index, relative to the project root.
    #[serde(default)]
    pub index_paths: Vec<String>,
    /// Glob patterns to ignore, in addition to `.gitignore`.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    /// Languages to index.
    #[serde(default = "default_languages")]
    pub languages: Vec<Language>,
    /// Storage settings (`[storage]`).
    #[serde(default)]
    pub storage: StorageConfig,
    /// Retrieval settings (`[retrieval]`).
    #[serde(default)]
    pub retrieval: RetrievalConfig,
    /// MCP server settings (`[mcp]`).
    #[serde(default)]
    pub mcp: McpConfig,
}

/// `[storage]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageConfig {
    /// SQLite database location.
    #[serde(default = "default_db_path")]
    pub db_path: String,
    /// Soft cap on database size, in megabytes.
    #[serde(default = "default_max_db_size_mb")]
    pub max_db_size_mb: u64,
}

/// `[retrieval]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalConfig {
    /// Default token budget for a query (§7.3).
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: usize,
    /// Default maximum number of results (§7.3).
    #[serde(default = "default_max_results")]
    pub default_max_results: usize,
    /// BM25 term-saturation parameter `k1` (§7.3).
    #[serde(default = "default_bm25_k1")]
    pub bm25_k1: f64,
    /// BM25 length-normalization parameter `b` (§7.3).
    #[serde(default = "default_bm25_b")]
    pub bm25_b: f64,
}

/// `[mcp]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpConfig {
    /// Transport type (`stdio` or `sse`).
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Port used for the SSE transport.
    #[serde(default = "default_sse_port")]
    pub sse_port: u16,
}

// ───────────────────────── documented defaults (§6/§7.3) ─────────────────────────

fn default_version() -> String {
    "0.1.0".to_string()
}
fn default_languages() -> Vec<Language> {
    vec![Language::Python, Language::TypeScript, Language::Go]
}
fn default_db_path() -> String {
    ".codecache/index.db".to_string()
}
fn default_max_db_size_mb() -> u64 {
    500
}
fn default_max_tokens() -> usize {
    4000
}
fn default_max_results() -> usize {
    20
}
fn default_bm25_k1() -> f64 {
    1.2
}
fn default_bm25_b() -> f64 {
    0.75
}
fn default_transport() -> String {
    "stdio".to_string()
}
fn default_sse_port() -> u16 {
    3000
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            db_path: default_db_path(),
            max_db_size_mb: default_max_db_size_mb(),
        }
    }
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        RetrievalConfig {
            default_max_tokens: default_max_tokens(),
            default_max_results: default_max_results(),
            bm25_k1: default_bm25_k1(),
            bm25_b: default_bm25_b(),
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig {
            transport: default_transport(),
            sse_port: default_sse_port(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: default_version(),
            index_paths: Vec::new(),
            ignore_patterns: Vec::new(),
            languages: default_languages(),
            storage: StorageConfig::default(),
            retrieval: RetrievalConfig::default(),
            mcp: McpConfig::default(),
        }
    }
}

/// A typed configuration error. Carries enough context to tell the user what went wrong without
/// leaking a panic.
#[derive(Debug)]
pub enum ConfigError {
    /// The config file could not be read (missing, unreadable, permissions, …).
    Io {
        /// The path we attempted to read.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The file was read but is not valid TOML / does not match the schema.
    Parse {
        /// The path we attempted to parse.
        path: String,
        /// The underlying TOML deserialization error.
        source: toml::de::Error,
    },
    /// The config could not be serialized to TOML before writing (`Config::save`, D18).
    Serialize {
        /// The path we were about to write.
        path: String,
        /// The underlying TOML serialization error.
        source: toml::ser::Error,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io { path, source } => {
                write!(f, "failed to read config file '{path}': {source}")
            }
            ConfigError::Parse { path, source } => {
                write!(f, "failed to parse config file '{path}': {source}")
            }
            ConfigError::Serialize { path, source } => {
                write!(f, "failed to serialize config for '{path}': {source}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io { source, .. } => Some(source),
            ConfigError::Parse { source, .. } => Some(source),
            ConfigError::Serialize { source, .. } => Some(source),
        }
    }
}

impl Config {
    /// Load and validate a `config.toml` from `path`, applying documented defaults for omitted
    /// fields. Returns a typed [`ConfigError`] (never panics) on a missing/unreadable file or
    /// malformed TOML.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let display = path.display().to_string();
        let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: display.clone(),
            source,
        })?;
        toml::from_str::<Config>(&raw).map_err(|source| ConfigError::Parse {
            path: display,
            source,
        })
    }

    /// Serialize the full config to TOML and write it to `path` (additive, **D18**). Used by the
    /// CLI `config <KEY> <VALUE>` write path to persist a mutated setting without clobbering
    /// unrelated keys (the whole resolved config is re-serialized). Returns a typed [`ConfigError`]
    /// (never panics) on a serialization or I/O failure.
    ///
    /// # Errors
    /// Returns [`ConfigError::Serialize`] if the config cannot be encoded as TOML, and
    /// [`ConfigError::Io`] if the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let display = path.display().to_string();
        let toml = toml::to_string(self).map_err(|source| ConfigError::Serialize {
            path: display.clone(),
            source,
        })?;
        std::fs::write(path, toml).map_err(|source| ConfigError::Io {
            path: display,
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_documented_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.retrieval.default_max_tokens, 4000);
        assert_eq!(cfg.retrieval.default_max_results, 20);
        assert!((cfg.retrieval.bm25_k1 - 1.2).abs() < f64::EPSILON);
        assert!((cfg.retrieval.bm25_b - 0.75).abs() < f64::EPSILON);
        assert_eq!(
            cfg.languages,
            vec![Language::Python, Language::TypeScript, Language::Go]
        );
        assert_eq!(cfg.storage.db_path, ".codecache/index.db");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("cc-config-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("config.toml");

        let mut cfg = Config::default();
        cfg.storage.max_db_size_mb = 1000;
        cfg.save(&path).expect("save config");

        let loaded = Config::load(&path).expect("load saved config");
        assert_eq!(loaded.storage.max_db_size_mb, 1000);
        assert_eq!(loaded, cfg);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
