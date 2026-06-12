//! `codecache init` handler (M7.3).
//!
//! Resolves the project root to the current working directory and delegates to the
//! [`crate::init`] facade, which creates `<root>/.codecache/{config.toml,index.db}` (non-clobbering
//! config, idempotent schema). `AppError` is surfaced through `anyhow` so `main` maps it to a
//! nonzero exit. The `--db-path` flag is resolved by the facade from the config's `db_path`; for
//! M7.3 the tests exercise the default path, so a non-default `--db-path` is not yet honored
//! (follow-up: thread it into the facade).

use anyhow::{Context, Result};

/// Initialize a CodeCache index in the current working directory.
pub fn run() -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;
    crate::init(&root).map_err(anyhow::Error::new)?;
    println!("Initialized CodeCache index in {}", root.display());
    Ok(())
}
