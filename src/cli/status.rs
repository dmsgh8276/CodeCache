//! `codecache status` handler (M7.3).
//!
//! Reads the aggregates that genuinely exist in the index: `total_files` / `total_chunks` from
//! `index_state` (decimal strings, default 0 if absent), the on-disk database file size, and a
//! per-language file-count breakdown derived from `files_metadata` (cheap: one `all_indexed_files`
//! + a `get_file_meta` per file). Prints a §7.2-style block including the crate version.
//!
//! Deferred (NOT in the M1 schema — follow-up, no columns added this slice): the §7.2 illustrative
//! Created / Last-index timestamps and the per-`symbol_type` (Functions/Classes/Methods) breakdown.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::storage::Storage;

use super::paths;

/// Print index statistics and health for the project at the working dir.
pub fn run(db_path: &Path) -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;
    let resolved_db = paths::resolve(&root, db_path);
    let storage = Storage::new(&resolved_db)
        .map_err(anyhow::Error::new)
        .with_context(|| format!("could not open index database at {}", resolved_db.display()))?;

    let total_files = read_count(&storage, "total_files")?;
    let total_chunks = read_count(&storage, "total_chunks")?;
    let db_size = std::fs::metadata(&resolved_db)
        .map(|m| m.len())
        .unwrap_or(0);
    let by_language = language_breakdown(&storage)?;

    println!("CodeCache index status");
    println!("  Version:   {}", env!("CARGO_PKG_VERSION"));
    println!("  Database:  {} ({} bytes)", resolved_db.display(), db_size);
    println!("  Files:     {total_files}");
    println!("  Chunks:    {total_chunks}");
    if !by_language.is_empty() {
        println!("  Files by language:");
        for (language, count) in &by_language {
            println!("    {language}: {count}");
        }
    }
    // Follow-up (not stored in the M1 schema): Created / Last-index timestamps and a per-symbol_type
    // breakdown. Omitted this slice rather than adding schema columns.
    Ok(())
}

/// Read an `index_state` decimal counter, defaulting to 0 when absent or unparseable.
fn read_count(storage: &Storage, key: &str) -> Result<usize> {
    let raw = storage.get_index_state(key).map_err(anyhow::Error::new)?;
    Ok(raw
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0))
}

/// Per-language file counts derived from `files_metadata` (deterministic order via `BTreeMap`).
fn language_breakdown(storage: &Storage) -> Result<BTreeMap<&'static str, usize>> {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for path in storage.all_indexed_files().map_err(anyhow::Error::new)? {
        if let Some(meta) = storage.get_file_meta(&path).map_err(anyhow::Error::new)? {
            *counts.entry(meta.language.as_str()).or_insert(0) += 1;
        }
    }
    Ok(counts)
}
