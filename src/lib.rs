//! CodeCache — a local-first, AST-driven code-context retrieval engine.
//!
//! Module bodies are filled milestone by milestone per [`docs/ROADMAP.md`]. As of M5, `types`,
//! `config`, `storage`, `hasher`, `parser`, `chunker`, `indexer`, and the `app` facade are
//! implemented; `retriever`, `formatter`, `cli`, and `mcp_server` remain stubs (M6–M8).
//!
//! Module map (build order is bottom-up — see `docs/ENGINEERING_PLAN.md` §2):
//! - [`types`]      shared, dependency-free core types (`Chunk`, `Language`, …) — Decision Log D5
//! - [`config`]     `.codecache/config.toml` load + validation
//! - [`storage`]    SQLite + FTS5 schema, CRUD, BM25 search
//! - [`hasher`]     xxHash3-128 content hashing + change detection
//! - [`parser`]     Tree-sitter integration: grammars, queries, AST nodes
//! - [`chunker`]    AST nodes → enriched `Chunk`s
//! - [`indexer`]    discovery → parse → chunk → hash → store (incremental)
//! - [`app`]        thin `init`/`index` library facade over config/storage/indexer (M5.4)
//! - [`retriever`]  BM25 search + snippet extraction + token budgeting
//! - [`formatter`]  TOON / JSON / plaintext output
//! - [`cli`]        `clap` command parsing + dispatch
//! - [`mcp_server`] stdio JSON-RPC MCP adapter (transport-agnostic core — Decision Log D4)

/// The crate version, sourced from `Cargo.toml` so it has a single source of truth.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod types;

pub mod app;
pub mod chunker;
pub mod cli;
pub mod config;
pub mod formatter;
pub mod hasher;
pub mod indexer;
pub mod mcp_server;
pub mod parser;
pub mod retriever;
pub mod storage;

// ── Public application facade (slice M5.4) ──────────────────────────────────
// The thin `init` → `index` library entry points + their error, re-exported at the crate root so
// callers use `codecache::{init, index, AppError, IndexStats}` rather than reaching into modules.
pub use app::{index, init, AppError};
pub use indexer::IndexStats;
