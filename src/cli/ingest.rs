//! `codecache ingest <CHUNKS_JSON>` handler (R2.3a / **D25**, hidden research seam).
//!
//! Inserts caller-supplied chunks straight into the index (bypassing discover→parse→chunk) so the
//! R2 chunker ablation can feed any chunker's output through the same storage + BM25 + retriever.
//! Delegates to the [`crate::ingest_chunks`] facade over the current working directory and reports
//! the resulting [`crate::IngestStats`]; `AppError` surfaces through `anyhow` for a nonzero exit.

use std::path::Path;

use anyhow::{Context, Result};

/// Ingest the pre-made chunks in `chunks_json` into the index rooted at the current directory.
pub fn run(chunks_json: &Path) -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;
    let stats = crate::ingest_chunks(&root, chunks_json).map_err(anyhow::Error::new)?;
    println!(
        "Ingested {} file(s), {} chunk(s)",
        stats.files_ingested, stats.chunks_ingested
    );
    Ok(())
}
