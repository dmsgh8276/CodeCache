//! `codecache index` handler (M7.3).
//!
//! Delegates to the [`crate::index`] facade (full on a fresh DB, incremental + reconcile on a
//! populated one) over the current working directory and reports the resulting [`IndexStats`].
//! `AppError` surfaces through `anyhow` for a nonzero exit.

use anyhow::{Context, Result};

/// Build or update the index for the project rooted at the current working directory.
pub fn run() -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;
    let stats = crate::index(&root).map_err(anyhow::Error::new)?;
    println!(
        "Indexed {} file(s), {} chunk(s) in {} ms",
        stats.files_processed, stats.chunks_indexed, stats.duration_ms
    );
    Ok(())
}
