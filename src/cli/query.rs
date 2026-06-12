//! `codecache query <QUERY>` handler (M7.3).
//!
//! Opens `Storage`, builds a [`Retriever`], runs the query under the flag-derived
//! [`QueryOptions`], and prints the result through the M7.1 [`crate::formatter`] (format chosen by
//! `--format`, default text). `--file-filter` maps the given glob/path to a single-entry
//! `file_filter` list — the retriever applies it as an exact-`PathBuf` post-filter (no glob
//! expansion in v0.1; documented as exact-match semantics).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::formatter::{self, Format};
use crate::retriever::{QueryOptions, Retrieve, Retriever};
use crate::storage::Storage;

use super::{paths, OutputFormat};

/// Search the index and print formatted results.
pub fn run(
    query: &str,
    max_tokens: usize,
    max_results: usize,
    format: OutputFormat,
    file_filter: Option<&str>,
    db_path: &Path,
) -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;
    let resolved_db = paths::resolve(&root, db_path);
    let storage = Storage::new(&resolved_db)
        .map_err(anyhow::Error::new)
        .with_context(|| format!("could not open index database at {}", resolved_db.display()))?;

    let retriever = Retriever::new(storage);
    let options = QueryOptions {
        max_tokens,
        max_results,
        // Exact-match post-filter: wrap the given glob/path as a single allowed path (no glob
        // expansion in v0.1). The retriever filters on `chunk.file_path == this path`.
        file_filter: file_filter.map(|f| vec![PathBuf::from(f)]),
    };

    let result = retriever
        .query(query, options)
        .map_err(anyhow::Error::new)?;

    let fmt: Format = format.into();
    // For an empty result set in the human-readable TEXT format, emit a query-free notice rather
    // than the formatter's `Query: "<q>"` header echo: a "no results" report must not look like it
    // surfaced the searched-for symbol (a caller checks that an unindexed symbol is genuinely
    // absent from query output). JSON stays a pipe-through so its output is always parseable, and
    // TOON's empty output is already an empty (query-free) string.
    if result.chunks.is_empty() && fmt == Format::Text {
        println!("No results found.");
        return Ok(());
    }
    print!("{}", formatter::format(&result, query, fmt));
    Ok(())
}
