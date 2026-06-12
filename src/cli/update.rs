//! `codecache update <FILE>...` handler (M7.3).
//!
//! Opens `Storage` at the resolved db path, loads the project `Config`, builds an [`Indexer`]
//! rooted at the current working directory, and re-indexes the listed files via
//! [`Indexer::update_files`]. Positional `<FILE>` args are resolved relative to the working dir.
//! Glob handling is intentionally minimal — the given paths are passed through as-is.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::indexer::Indexer;
use crate::storage::Storage;

use super::paths;

/// Re-index the explicitly listed `files` (resolved against the working dir).
pub fn run(files: &[PathBuf], db_path: &Path) -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;

    let config = paths::load_config(&root)?;
    let storage = open_storage(&root, db_path)?;
    let mut indexer = Indexer::new(config, storage, root.clone()).map_err(anyhow::Error::new)?;

    let resolved: Vec<PathBuf> = files.iter().map(|f| paths::resolve(&root, f)).collect();
    let stats = indexer
        .update_files(&resolved)
        .map_err(anyhow::Error::new)?;

    println!(
        "Updated {} file(s), {} chunk(s) in {} ms",
        stats.files_processed, stats.chunks_indexed, stats.duration_ms
    );
    Ok(())
}

/// Open `Storage` at `<root>/<db_path>` (the default `.codecache/index.db` resolves under root).
fn open_storage(root: &Path, db_path: &Path) -> Result<Storage> {
    let resolved = paths::resolve(root, db_path);
    Storage::new(&resolved)
        .map_err(anyhow::Error::new)
        .with_context(|| format!("could not open index database at {}", resolved.display()))
}
