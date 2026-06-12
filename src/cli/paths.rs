//! Shared path/config resolution helpers for the M7.3 command handlers.
//!
//! The handlers that open `Storage` / `Config` directly (`update`, `query`, `status`, `config`)
//! all resolve the db path and config file relative to the project root (the working dir) the same
//! way; centralizing it keeps that resolution consistent with the [`crate::app`] facade
//! (`<root>/<config.storage.db_path>`, default `<root>/.codecache/index.db`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Config;

/// Directory CodeCache stores its config + database under, inside a project root.
const CODECACHE_DIR: &str = ".codecache";
/// Config file name written inside [`CODECACHE_DIR`].
const CONFIG_FILE: &str = "config.toml";

/// Resolve `path` against `root`. An absolute `path` is returned unchanged by `Path::join`, so the
/// default relative `.codecache/index.db` lands under `root` and an absolute override is honored.
pub fn resolve(root: &Path, path: &Path) -> PathBuf {
    root.join(path)
}

/// The project's `config.toml` path: `<root>/.codecache/config.toml`.
pub fn config_path(root: &Path) -> PathBuf {
    root.join(CODECACHE_DIR).join(CONFIG_FILE)
}

/// Load the project config from `<root>/.codecache/config.toml`, surfacing a missing/malformed
/// config through `anyhow` (typically: run `codecache init` first).
pub fn load_config(root: &Path) -> Result<Config> {
    let path = config_path(root);
    Config::load(&path)
        .map_err(anyhow::Error::new)
        .with_context(|| {
            format!(
                "could not load config at {} (run `codecache init`?)",
                path.display()
            )
        })
}
