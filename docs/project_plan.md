# CodeCache: Local Codebase Context Engine for Terminal AI Workflows

## Comprehensive Technical Specification & Implementation Guide

***

## 1. Executive Summary & Project Goals

### 1.1 Problem Statement

**Modern AI CLI agents suffer from inefficient context management.** Current workflows dump entire files or directories into LLM context windows, causing:

* **Token waste**: A single 2,000-line file consumes \~500K tokens in a multi-turn conversation
* **Economic burden**: Agent workflows burn 1–3.5M tokens/day; at $15/MTok (Claude Sonnet), this costs $1,095–$3,833/year per agent
* **Latency penalties**: Large context windows slow inference (quadratic attention complexity)
* **Relevance dilution**: Irrelevant code overwhelms the model, reducing task completion rates

**The core insight:** Most agent queries need <5% of a codebase at any given time. A function-level retrieval system can achieve 50–70% token reduction while improving precision.

### 1.2 Solution: CodeCache

<!-- Copilot-Researcher-Visualization -->

![Solution overview](assets/solution-overview.png)

CodeCache is a **local-first, AST-driven context retrieval engine** that:

1. **Parses** source code into semantic units (functions, classes, methods) using Tree-sitter AST
2. **Indexes** these units in SQLite with FTS5 full-text search, maintaining file hashes for incremental updates
3. **Retrieves** only relevant snippets at query time, ranked by BM25
4. **Injects** concentrated context into AI agents via CLI stdout or MCP protocol

**Primary differentiators:**

* **Deterministic**: AST boundaries never drift (unlike embeddings)
* **Incremental**: Re-index only changed files using xxHash (10×+ faster than SHA-256)
* **Hybrid-ready**: Pure AST for v0.1; optional embeddings in v0.2 for semantic queries
* **CLI-native**: Zero-config `codecache query "find auth logic"` command, not an IDE plugin

> **Positioning update (2026-06-11 — ROADMAP D12; full analysis in
> [`../project_overview.md`](../project_overview.md)).** The product sentence is now:
> *CodeCache is a zero-dependency, deterministic code index that coding agents call as a tool —
> replacing N rounds of grep with one structured lookup, with no embedding model, vector
> database, language server, or cloud account.* Three consequences: (1) **the agent is the
> user**, not the human — tool descriptions, output ordering, and result granularity are
> optimized for the agent's next action (D13); (2) **freshness is answered structurally** via
> self-healing search (D14); (3) we **compose with grep-in-a-loop rather than replace it** —
> the evaluation baseline is agentic search, not "context dumping" (D16). The architecture is
> unchanged; the framing and the evaluation design (§1.3, §9.3) are what changed.

### 1.3 Success Criteria (v0.1)

| **Metric**           | **Target**                    | **Validation**                                      |
| -------------------- | ----------------------------- | --------------------------------------------------- |
| Token/turn economy   | Fewer tokens + tool turns than grep-only agentic search at matched retrieval recall, with CIs (D16) | ContextBench-Lite gold-context scoring + agent-in-loop study (research track R1–R3; `project_overview.md` §5) |
| Query latency        | <500ms (p95)                  | 100K LOC monorepo, cold SQLite cache                |
| Index overhead       | <100MB                        | Django codebase (2,910 files, \~450K LOC)           |
| Incremental re-index | <2s for 10-file change        | Modify 10 Python files, measure total re-index time |
| Language coverage    | Python, TypeScript, Go        | Tree-sitter grammars with production test suites    |

### 1.4 Non-Goals (Explicit Scope Limits)

* ❌ **Not a semantic search engine**: No embeddings in v0.1 (AST + BM25 only)
* ❌ **Not a code intelligence server**: No refactoring, go-to-definition, or call graphs
* ❌ **Not universal language support**: Focus on 3 languages with best Tree-sitter grammars
* ❌ **Not real-time**: Index updates triggered explicitly (not file-watcher based)
* ❌ **Not a replacement for ripgrep/ast-grep**: Different use case (context retrieval vs search)

***

## 2. System Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI Interface                            │
│  $ codecache index .                                             │
│  $ codecache query "authenticate user" --max-tokens 4000         │
│  $ codecache update src/auth.py                                  │
└────────────────┬────────────────────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Query Coordinator                             │
│  - Parse user query                                              │
│  - Apply token budget                                            │
│  - Format output (JSON/text/TOON)                                │
└────────────┬───────────────────────────────┬────────────────────┘
             │                               │
             ▼                               ▼
┌────────────────────────┐    ┌─────────────────────────────────┐
│   Retrieval Engine     │    │    Indexing Pipeline            │
│  - BM25 ranking        │    │  - Tree-sitter parser           │
│  - Snippet extraction  │    │  - AST chunker                  │
│  - Deduplication       │    │  - Hash computer (xxHash)       │
└──────────┬─────────────┘    └──────────┬──────────────────────┘
           │                              │
           ▼                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Storage Layer (SQLite)                       │
│                                                                  │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │  symbols (FTS5)  │  │  files_metadata  │  │  index_state │  │
│  │  - symbol_name   │  │  - file_path     │  │  - version   │  │
│  │  - chunk_text    │  │  - content_hash  │  │  - timestamp │  │
│  │  - file_path     │  │  - mtime         │  │  - stats     │  │
│  │  - start_byte    │  │  - language      │  │              │  │
│  │  - end_byte      │  │                  │  │              │  │
│  │  - symbol_type   │  │                  │  │              │  │
│  └──────────────────┘  └──────────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Agent Integration Layer                        │
│  - MCP Server (stdio/SSE transport)                              │
│  - Claude Code hooks                                             │
│  - Stdout formatter (pipe to LLM)                                │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Data Flow: Indexing Pipeline

<!-- Copilot-Researcher-Visualization -->

![Indexing pipeline](assets/indexing-pipeline.png)

### 2.3 Data Flow: Retrieval Pipeline

```
User Query: "find authentication logic"
         │
         ▼
┌────────────────────────────────────┐
│  1. Query Preprocessing            │
│  - Tokenize: ["find", "authentication", "logic"]  │
│  - Remove stopwords: ["authentication", "logic"]  │
│  - Stem (optional): ["authent", "logic"]          │
└────────────┬───────────────────────┘
             │
             ▼
┌────────────────────────────────────┐
│  2. FTS5 Search                    │
│  SELECT *, bm25(symbols) AS score  │
│  FROM symbols                      │
│  WHERE symbols MATCH 'authentication OR logic'  │
│  ORDER BY bm25(symbols)            │
│  LIMIT 100                         │
└────────────┬───────────────────────┘
             │
             ▼
┌────────────────────────────────────┐
│  3. Re-Ranking (Future: Embeddings)│
│  For v0.1: Use BM25 scores as-is   │
│  For v0.2: Re-rank top-100 with    │
│            CodeBERT cosine similarity │
└────────────┬───────────────────────┘
             │
             ▼
┌────────────────────────────────────┐
│  4. Token Budget Enforcement       │
│  - User specifies --max-tokens 4000│
│  - Greedily pack top-ranked chunks │
│  - Stop when budget exhausted      │
└────────────┬───────────────────────┘
             │
             ▼
┌────────────────────────────────────┐
│  5. Output Formatting              │
│  - TOON format (file:line pairs)   │
│  - JSON (for programmatic use)     │
│  - Plain text (for stdout pipe)    │
└────────────────────────────────────┘
```

***

## 3. Module Breakdown

### 3.1 Module Responsibility Table

| **Module**   | **Responsibility**                                                       | **Primary Dependencies**      | **Estimated LOC** |
| ------------ | ------------------------------------------------------------------------ | ----------------------------- | ----------------- |
| `cli`        | Argument parsing, command dispatch, user-facing errors                   | `clap` (Rust CLI parser)      | 300               |
| `indexer`    | Orchestrate indexing: file discovery → parsing → storage                 | `parser`, `hasher`, `storage` | 400               |
| `parser`     | Tree-sitter integration: load grammars, run queries, extract AST nodes   | `tree-sitter` (Rust bindings) | 600               |
| `chunker`    | Extract semantic units from AST: function/class boundaries, text content | `parser`                      | 250               |
| `hasher`     | Compute file content hashes (xxHash3-128), compare against cache         | `xxhash-rust`                 | 150               |
| `storage`    | SQLite interface: create tables, insert/query/delete chunks, FTS5 config | `rusqlite`                    | 500               |
| `retriever`  | Query execution: BM25 search, snippet extraction, token counting         | `storage`, `tokenizer`        | 400               |
| `formatter`  | Output serialization: TOON, JSON, plaintext                              | `serde_json`                  | 200               |
| `mcp_server` | MCP protocol adapter: stdio transport, tool registration                 | `mcp-sdk` (hypothetical)      | 350               |
| `config`     | Load .codecache.toml: index paths, ignored patterns, language settings   | `toml`                        | 150               |

**Total estimated LOC:** \~3,300 (reasonable for a Rust CLI tool)

### 3.2 Core Module APIs (Pseudocode)

#### 3.2.1 Parser Module

```rust
// parser/mod.rs
pub struct Parser {
    ts_parser: tree_sitter::Parser,
    language_configs: HashMap<Language, LanguageConfig>,
}

pub enum Language {
    Python,
    TypeScript,
    Go,
}

pub struct LanguageConfig {
    grammar: tree_sitter::Language,
    queries: &'static str,  // combined .scm: function/class/method + decorated_definition (M3)
}

impl Parser {
    pub fn new() -> Result<Self>;
    
    // Parse file and extract AST
    pub fn parse_file(&mut self, path: &Path, content: &str, lang: Language) 
        -> Result<tree_sitter::Tree>;
    
    // Extract semantic chunks (functions, classes) from AST
    pub fn extract_chunks(&self, tree: &tree_sitter::Tree, source: &str, lang: Language) 
        -> Result<Vec<Chunk>>;
}

// Defined in `crate::types` (see §4.3); re-exported for convenience.
pub struct Chunk {
    pub symbol_name: String,      // e.g., "authenticate_user"
    pub symbol_type: SymbolType,  // Function, Class, Method
    pub file_path: PathBuf,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,        // 1-based; stored UNINDEXED for line-range output (D7)
    pub end_line: usize,          // 1-based, inclusive
    pub chunk_text: String,       // Full source text of function/class
    pub language: Language,

    // Metadata enrichment (Decision Log D3) — indexed in FTS5 to lift recall:
    pub parent_symbol: Option<String>,  // enclosing class/fn for methods/nested defs
    pub file_docstring: Option<String>, // module/file-level docstring
    pub imports: Vec<String>,           // import statements visible in the file
    pub cross_references: Vec<String>,  // referenced symbol names within the chunk

    // Graceful degradation (Decision Log D2) — set true when the M4 chunker fell back to the
    // line heuristic because the parser's ERROR rate exceeded HEURISTIC_FALLBACK_THRESHOLD.
    pub is_heuristic: bool,
}
```

**ERROR-rate / graceful-degradation API** (M3-introduced, Decision Log D2). The parser walks the
tree and reports the syntactic error density so M4/M5 can route badly-broken files to the
heuristic chunker. v0.1 *only reports* here; the heuristic chunker and its `heuristic` flag are
owned by M4.

```rust
// parser/mod.rs (free functions + const)

/// (ERROR + MISSING) node count over the **named-node** count, clamped to [0, 1].
/// Named-node denominator: anonymous literal tokens (`(`, `)`, `:`, `+`, `def`, …) carry no
/// independent syntactic meaning and dilute the signal, so the honest measure is
/// "broken syntactic units / meaningful syntactic units". `error_rate(valid) == 0.0`;
/// any malformed file reports `> 0.0`.
pub fn error_rate(tree: &tree_sitter::Tree) -> f32;

/// `rate >= HEURISTIC_FALLBACK_THRESHOLD` ⇒ route to the M4 heuristic chunker.
pub fn should_fall_back(rate: f32) -> bool;

/// Fallback threshold (~20% of named nodes broken). In [0, 1).
pub const HEURISTIC_FALLBACK_THRESHOLD: f32 = 0.20;

// Typed error surface (no reachable unwrap/expect/panic on library paths):
pub enum ParserError {
    UnsupportedLanguage(Language),        // e.g. Go/TS at M3 ⇒ typed Err
    Language(tree_sitter::LanguageError), // set_language failed
    Query(tree_sitter::QueryError),       // an embedded `.scm` failed to compile
    ParseFailed { path: PathBuf },        // tree-sitter returned no tree
}
// impl std::error::Error for ParserError { fn source() … } chains the underlying TS error.
```

**Chunk span conventions** (pinned by the M3 parser tests, byte-exact
`&source[start_byte..end_byte] == chunk_text`):
- A **decorated** definition is spanned from its `decorated_definition` wrapper, so the
  `@decorator` lines are *inside* the chunk span (`start_line` is the first decorator line).
- A `function_definition` whose nearest *definition* ancestor is a `class_definition` is a
  `SymbolType::Method` with `parent_symbol = <class>`; a function nested in a function stays a
  `Function` with `parent_symbol = <enclosing fn>`.
- The span is extended to include the single trailing line terminator (`\n`, or `\r\n` for CRLF —
  preserved verbatim) that closes the definition's last line. `start_line`/`end_line` are 1-based
  inclusive (D7); the appended terminator does not advance `end_line`.

**Tree-sitter Query Examples** (for extraction):

```scheme
; Python functions
(function_definition
  name: (identifier) @func.name
  body: (_) @func.body) @func.def

; Python classes
(class_definition
  name: (identifier) @class.name
  body: (_) @class.body) @class.def

; TypeScript functions
(function_declaration
  name: (identifier) @func.name
  body: (_) @func.body) @func.def

; Go functions
(function_declaration
  name: (identifier) @func.name
  body: (_) @func.body) @func.def
```

#### 3.2.2 Storage Module

```rust
// storage/mod.rs
use rusqlite::{Connection, params};

pub struct Storage {
    // Decision Log D8: shared, since `rusqlite::Connection` is not `Clone`. Cloning `Storage`
    // clones the Arc, so the MCP server lends one connection to both Retriever and Indexer.
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    // Initialize database with schema
    pub fn new(db_path: &Path) -> Result<Self>;
    
    // Create FTS5 virtual table for symbols
    pub fn init_schema(&self) -> Result<()>;
    
    // Insert chunks into FTS5 table
    pub fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()>;
    
    // Delete all chunks for a given file (for incremental updates)
    pub fn delete_chunks_for_file(&self, file_path: &Path) -> Result<()>;

    // Delete a file's files_metadata row (deletion reconciliation, §5.2). Added in M5.3:
    // symmetric with delete_chunks_for_file so the indexer can fully evict a file that has
    // disappeared from disk (chunks + metadata row). Deleting an unknown file is a no-op.
    pub fn delete_file_meta(&self, file_path: &Path) -> Result<()>;

    // Enumerate every path in files_metadata (deletion reconciliation, §5.2). Added in M5.3:
    // lets index_all compare the known/indexed set against the on-disk discovery set and evict
    // files that are gone. Also used to recompute DB-wide index_state totals after a delta.
    pub fn all_indexed_files(&self) -> Result<Vec<PathBuf>>;

    // Full-text search with BM25 ranking (built-in default per-column weights 10,1,1,5,2,2,2).
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;

    // Full-text search with caller-supplied per-column BM25 weights (R2.2a / Decision Log D24).
    // `weights` is one f64 per indexed FTS5 column, in `schema::CREATE_SYMBOLS` order (7 total);
    // `None` is exactly equivalent to `search` (default weights) — `search` delegates here with
    // `None`, so the default path is byte-identical. FTS5 `bm25()` weights are auxiliary-function
    // arguments that cannot be bound as `?` parameters, so the 7 f64 are formatted into the SQL
    // ranking expression; this is injection-safe ONLY because each is a validated `f64` (never raw
    // CLI text). Ordering invariant is unchanged (`ORDER BY bm25 ASC`, span tie-break downstream).
    pub fn search_with_weights(
        &self,
        query: &str,
        limit: usize,
        weights: Option<&[f64; 7]>,
    ) -> Result<Vec<SearchResult>>;

    // Path-scoped symbol skeleton for the `codecache_outline` tool (Decision Log D19, M8.3).
    // Reads the skeleton columns (symbol_name, symbol_type, parent_symbol, file_path, start_line,
    // end_line) straight off the contentful `symbols` FTS5 table for an exact file OR a directory
    // prefix (<dir>/%), ordered by (file_path, start_line, end_line). Zero source reads (D7/D13);
    // returns slim SymbolOutline rows, not full Chunks. Additive — no change to `search`/schema.
    pub fn symbols_for_path(&self, path: &Path) -> Result<Vec<SymbolOutline>>;
    
    // Metadata operations
    pub fn get_file_hash(&self, file_path: &Path) -> Result<Option<String>>;
    // Persists hash + the §4.1 metadata fields (file_size, chunk_count). The `FileMeta`
    // parameter (Decision Log D6) lets M5's incremental indexer record everything the
    // `files_metadata` row needs in one call.
    pub fn update_file_hash(&self, file_path: &Path, meta: &FileMeta) -> Result<()>;

    // Batched multi-file write under ONE outer transaction (Decision Log D20, v0.1.x perf
    // follow-up). M10.1 measured the 10K-LOC cold index at 6.04s vs the <5s budget — the only
    // budget miss — because each per-file write committed its own transaction (≈200+ fsyncs for a
    // 200-file index) on top of FTS5 write amplification. `write_in_transaction` runs `each` once
    // per item inside a SINGLE outer transaction, isolating each item in its own SAVEPOINT:
    //   each(writer, &items[i]) -> Ok(())  ⇒ RELEASE the savepoint (item's writes persist in the tx)
    //   each(writer, &items[i]) -> Err(e)  ⇒ ROLLBACK TO the savepoint (discard the item's partial
    //                                         writes), record Err(e), and CONTINUE the batch.
    // The outer tx commits once at the end. Returns one inner `Result` per item (same order/length),
    // so the indexer keeps D2 per-file isolation (a failing file is skipped, not aborted) while the
    // whole batch pays a single commit/fsync. The OUTER `Result` is `Err` only for a non-isolatable
    // failure (outer BEGIN/COMMIT, poisoned lock). `BatchWriter` lends the per-item write ops, each
    // executing against the CURRENT savepoint over the SAME connection (D8) — it must NOT re-lock the
    // `Arc<Mutex<Connection>>` inside the closure (re-entrant-lock deadlock). The non-batched
    // `insert_chunks`/`delete_chunks_for_file`/`update_file_hash` above stay (autocommit) for
    // single-shot callers such as `app::ingest_chunks` (§3.2.4). No reachable panic.
    pub fn write_in_transaction<T, F>(&self, items: &[T], each: F) -> Result<Vec<Result<()>>>
    where
        F: FnMut(&BatchWriter<'_>, &T) -> Result<()>;
}

// Decision Log D20: lends the per-item write ops inside a `write_in_transaction` savepoint. Borrows
// the open transaction (shares the single D8 connection); never re-locks `Storage`.
pub struct BatchWriter<'a> { /* borrows the open tx/savepoint */ }
impl BatchWriter<'_> {
    pub fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()>;
    pub fn delete_chunks_for_file(&self, file_path: &Path) -> Result<()>;
    pub fn update_file_hash(&self, file_path: &Path, meta: &FileMeta) -> Result<()>;
}

// Decision Log D6: the write-side metadata bundle for `files_metadata`.
pub struct FileMeta {
    pub content_hash: String,   // xxHash3-128 hex string
    pub mtime: u64,             // Unix epoch seconds
    pub file_size: u64,         // bytes
    pub language: Language,
    pub chunk_count: usize,     // symbols extracted from this file
}

pub struct SearchResult {
    pub chunk: Chunk,
    pub bm25_score: f64,
}

// Decision Log D19: the slim per-symbol projection backing `codecache_outline` (M8.3). Only the
// fields the skeleton needs — no chunk_text/imports — so the outline stays within the §11.2 budget.
pub struct SymbolOutline {
    pub symbol_name: String,
    pub symbol_type: SymbolType,
    pub parent_symbol: Option<String>,
    pub file_path: PathBuf,
    pub start_line: usize,   // 1-based inclusive (D7)
    pub end_line: usize,
}
```

**SQL Schema** (detailed in Section 4):

```sql
-- Main symbols table (FTS5 virtual table)
CREATE VIRTUAL TABLE symbols USING fts5(
    symbol_name,
    symbol_type,      -- 'function', 'class', 'method'
    chunk_text,       -- Full source code of symbol
    parent_symbol,    -- D3: enclosing class/fn (indexed for recall)
    imports,          -- D3: import statements (indexed)
    cross_references, -- D3: referenced symbol names (indexed)
    file_docstring,   -- D3: module/file-level docstring (indexed)
    file_path UNINDEXED,
    start_byte UNINDEXED,
    end_byte UNINDEXED,
    start_line UNINDEXED,  -- D7: 1-based line range for output
    end_line UNINDEXED,
    language UNINDEXED,
    tokenize='unicode61 remove_diacritics 2'
);

-- File metadata (for incremental updates)
CREATE TABLE files_metadata (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,   -- xxHash3-128 hex string
    mtime INTEGER NOT NULL,       -- Unix timestamp
    language TEXT NOT NULL,
    indexed_at INTEGER NOT NULL
);

-- Index state (global metadata)
CREATE TABLE index_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

#### 3.2.3 Retriever Module

```rust
// retriever/mod.rs
// Decision Log D8: `Storage` wraps a shared `Arc<Mutex<rusqlite::Connection>>` (Connection is
// not `Clone`). `Storage` is therefore cheaply `Clone` (clones the Arc), so the MCP server can
// hand the same underlying connection to both `Retriever` and `Indexer` without re-opening.
pub struct Retriever {
    storage: Storage,
}

impl Retriever {
    pub fn new(storage: Storage) -> Self;
    
    // Main query interface
    pub fn query(&self, user_query: &str, options: QueryOptions) -> Result<QueryResult>;
    
    // Preprocess query: tokenize, remove stopwords
    fn preprocess_query(&self, query: &str) -> Vec<String>;
    
    // Apply token budget: pack chunks greedily
    fn apply_token_budget(&self, results: Vec<SearchResult>, max_tokens: usize) 
        -> Vec<SearchResult>;
}

pub struct QueryOptions {
    pub max_tokens: usize,        // Default: 4000
    pub max_results: usize,       // Default: 20
    pub file_filter: Option<Vec<PathBuf>>,  // Optional: restrict to specific files
    // R2.2a / Decision Log D24: optional per-column BM25 weight override for the 7 indexed
    // FTS5 columns, in `schema::CREATE_SYMBOLS` order — symbol_name, symbol_type, chunk_text,
    // parent_symbol, imports, cross_references, file_docstring. `None` ⇒ the built-in default
    // weights (10,1,1,5,2,2,2), byte-identical to pre-R2.2a behavior. The fixed-size array makes
    // "exactly 7 weights" a compile-time invariant; the CLI `--bm25-weights` flag parses 7
    // comma-separated f64 into it (malformed/wrong-arity input → a typed error, never a panic).
    // Exists so the R2 research harness can sweep ranking weights per `codecache query` invocation
    // over the process boundary without recompiling.
    pub bm25_weights: Option<[f64; 7]>,
}

pub struct QueryResult {
    pub chunks: Vec<SearchResult>,
    pub total_tokens: usize,
    pub total_results_found: usize,  // Before token budget
}
```

#### 3.2.4 Indexer Module

```rust
// indexer/mod.rs
pub struct Indexer {
    parser: Parser,
    hasher: Hasher,
    storage: Storage,
    config: Config,
}

impl Indexer {
    // `root: PathBuf` is an explicit 3rd argument (extends the original `new(config, storage)`):
    // discovery walks `config.index_paths` resolved against `root`, defaulting to `root` itself
    // when `index_paths` is empty. The integration/e2e tests (M5.2+) must point the indexer at a
    // `TempDir`, and discovery's `discover_files(config, root)` already takes an explicit root, so
    // the root is passed in rather than derived from cwd. Returns `Result<Indexer, IndexError>`.
    pub fn new(config: Config, storage: Storage, root: PathBuf) -> Result<Self>;
    
    // Full re-index. `Result<IndexStats, IndexError>`.
    pub fn index_all(&mut self) -> Result<IndexStats>;
    
    // Incremental update for specific files (M5.3). `Result<IndexStats, IndexError>`.
    pub fn update_files(&mut self, files: &[PathBuf]) -> Result<IndexStats>;
    
    // Internal: discover all files in index_paths (free fn `discover_files(config, root)` in M5.1)
    fn discover_files(&self) -> Result<Vec<PathBuf>>;
    
    // Internal: detect changed files via hash comparison
    fn detect_changed_files(&self, files: &[PathBuf]) -> Result<Vec<PathBuf>>;
}

pub struct IndexStats {
    pub files_processed: usize,
    pub chunks_indexed: usize,
    pub duration_ms: u64,
}
```

##### Chunk ingestion seam (Decision Log **D25**, research track R2.3a)

A CLI-reachable path that inserts **caller-supplied, pre-chunked** records straight into storage,
bypassing discover→parse→chunk. It exists so the R2 research harness can ablate the chunker over the
**same** storage + FTS5-BM25 + retriever (the chunker is an index-time hardcoded free fn — `chunker::chunk`
— not a swappable trait; the harness is process-boundary-only). The seam lives on the `app` facade beside
`index`/`init`, with a **format-local input DTO** (serde stays off `types::Chunk` — D4/D5):

```rust
// app.rs (facade), driven by the hidden `codecache ingest <CHUNKS_JSON>` command (§7.2).
pub fn ingest_chunks(project_root: &Path, chunks_json: &Path) -> Result<IngestStats, AppError>;

pub struct IngestStats { pub files_ingested: usize, pub chunks_ingested: usize }
```

Behavior: open `Storage` at the resolved `db_path` → deserialize the JSON **array of chunk records** (a
format-local DTO → `types::Chunk`, enum strings via `from_str_lenient`, optional enrichment fields default
to `null`/`[]`/`false`) → `Storage::insert_chunks` **in JSON-array order** (so the `bm25 ASC, rowid ASC`
tie-break is deterministic) → write one `files_metadata` row per distinct `file_path` (so `status`/
`codecache_outline` work) → restamp `index_state` `total_files`/`total_chunks`. Malformed JSON / missing
required field / unknown enum / wrong type → typed [`AppError`] → nonzero exit (no panic). Re-ingest /
incremental is **out of scope** (the harness inits a fresh DB per arm). The full input schema is the
`ingest` command spec in §7.2. Reuses the existing `insert_chunks` / `update_file_hash` / `set_index_state`
storage APIs — **no new `Storage` method, no new dependency** (serde/serde_json already in the tree).

***

## 4. Data Models and Storage

### 4.1 SQLite Schema (Detailed)

```sql
-- ============================================================
-- FTS5 Virtual Table: symbols
-- ============================================================
-- This is the core search table. FTS5 automatically creates
-- an inverted index for full-text search.
CREATE VIRTUAL TABLE symbols USING fts5(
    symbol_name,           -- Indexed: function/class name (e.g., "authenticate_user")
    symbol_type,           -- Indexed: 'function', 'class', 'method', 'struct'
    chunk_text,            -- Indexed: Full source code of the symbol
    parent_symbol,         -- Indexed (D3): enclosing class/fn for methods/nested defs
    imports,               -- Indexed (D3): import statements visible in the file
    cross_references,      -- Indexed (D3): referenced symbol names within the chunk
    file_docstring,        -- Indexed (D3): module/file-level docstring (recall on file-intent queries)
    file_path UNINDEXED,   -- NOT indexed in FTS5 (retrieved after search)
    start_byte UNINDEXED,  -- Byte offset in file (for snippet extraction)
    end_byte UNINDEXED,
    start_line UNINDEXED,  -- D7: 1-based start line (output as file:start-end without re-reading source)
    end_line UNINDEXED,    -- D7: 1-based inclusive end line
    language UNINDEXED,    -- 'python', 'typescript', 'go'
    
    -- Tokenizer configuration
    tokenize='unicode61 remove_diacritics 2'

    -- NOTE (Decision Log D11): the original pseudo-DDL had `content='symbols'`, but in FTS5
    -- `content=` names a *separate* external-content table — aiming it at this table's own name
    -- is invalid. v0.1 uses a default (contentful) FTS5 table: FTS5 stores every column value and
    -- returns it on SELECT, so chunks round-trip with no companion table. The list columns
    -- `imports`/`cross_references` are stored as `\n`-joined text (FTS5 has no array type).
    -- BM25 ranking uses FTS5 defaults k1=1.2, b=0.75 with per-column weights at query time
    -- (symbol_name weighted highest). Revisit external-content at M10 only if the <100MB index
    -- budget is threatened (§4.2 estimates ~6MB at Django scale).
);

-- ============================================================
-- Files Metadata: track hashes for incremental updates
-- ============================================================
CREATE TABLE files_metadata (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,      -- xxHash3-128 (32 hex chars)
    mtime INTEGER NOT NULL,          -- File modification time (Unix epoch)
    file_size INTEGER NOT NULL,      -- Bytes
    language TEXT NOT NULL,          -- 'python', 'typescript', 'go'
    chunk_count INTEGER NOT NULL,    -- Number of symbols extracted
    indexed_at INTEGER NOT NULL      -- When this file was last indexed
);

CREATE INDEX idx_files_mtime ON files_metadata(mtime);
CREATE INDEX idx_files_language ON files_metadata(language);

-- ============================================================
-- Index State: global metadata
-- ============================================================
CREATE TABLE index_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Seed initial state
INSERT INTO index_state (key, value) VALUES
    ('version', '0.1.0'),
    ('created_at', strftime('%s', 'now')),
    ('last_full_index', '0'),
    ('total_files', '0'),
    ('total_chunks', '0');
```

### 4.2 Storage Backend Rationale

**Why SQLite + FTS5?**

1. **Zero setup**: Single-file database, no server process
2. **FTS5 performance**: Inverted index enables <50ms search on 100K chunks
3. **BM25 built-in**: `bm25(symbols)` function provides ranking out-of-the-box
4. **Atomic transactions**: ACID guarantees for incremental updates
5. **Portability**: Works on Linux/macOS/Windows, embeds in Rust via `rusqlite`

**Index size estimation:**

* **Django codebase**: 2,910 files, \~450K LOC → \~20K functions/classes
* **FTS5 overhead**: \~5× raw text size (inverted index + positions)
* **Expected index size**: 20K × 50 bytes (avg symbol size) × 5 = **\~5MB** (symbols) + \~1MB (metadata) = **\~6MB total**

This is well under the 100MB target.

### 4.3 Chunk Data Model

**Location (Decision Log D5):** `Chunk`, `Language`, `SymbolType`, and `FileMeta` (§3.2.2)
live in a dependency-free `crate::types` module — not inside `parser`. Both `storage` (M1) and
`parser`/`chunker` (M3/M4) depend on these types, so housing them in a leaf module keeps the
bottom-up build order (`ENGINEERING_PLAN.md` §2) acyclic: `storage` need not depend on `parser`.

```rust
// crate::types
pub struct Chunk {
    pub symbol_name: String,      // "authenticate_user"
    pub symbol_type: SymbolType,  // Function | Class | Method | Struct
    pub file_path: PathBuf,       // "src/auth/handlers.py"
    pub start_byte: usize,        // 1234 (byte offset in file)
    pub end_byte: usize,          // 1789
    pub start_line: usize,        // D7: 1-based start line
    pub end_line: usize,          // D7: 1-based inclusive end line
    pub chunk_text: String,       // "def authenticate_user(...):\n    ..."
    pub language: Language,       // Python

    // Metadata enrichment (Decision Log D3):
    pub parent_symbol: Option<String>,
    pub file_docstring: Option<String>,
    pub imports: Vec<String>,
    pub cross_references: Vec<String>,

    // Graceful degradation (Decision Log D2): true when the chunk came from the M4 line-heuristic
    // fallback (high parser ERROR rate) rather than the AST path. AST/storage chunks are `false`.
    pub is_heuristic: bool,
}

pub enum SymbolType {
    Function,
    Class,
    Method,   // Class method
    Struct,   // Go/Rust structs
}

pub enum Language {
    Python,
    TypeScript,
    Go,
}
```

### 4.4 File Hash Strategy

**Hash computation:**

```rust
use xxhash_rust::xxh3::Xxh3;

pub fn compute_file_hash(path: &Path) -> Result<String> {
    let content = std::fs::read(path)?;
    let metadata = std::fs::metadata(path)?;
    let mtime = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
    
    // Hash both content and mtime (mtime alone is insufficient)
    let mut hasher = Xxh3::new();
    hasher.update(&content);
    hasher.update(&mtime.to_le_bytes());
    
    Ok(format!("{:032x}", hasher.digest128()))
}
```

**Why xxHash over SHA-256?**

* **Speed**: xxHash3-128 is 10×+ faster than SHA-256
* **Collision resistance**: 128-bit output provides 2^64 collision resistance (sufficient for file hashing)
* **No cryptographic requirement**: We're detecting file changes, not validating integrity

***

## 5. Indexing Pipeline (Detailed)

### 5.1 Initial Indexing Algorithm

```rust
// Pseudocode for full index operation
fn index_all(config: &Config, storage: &mut Storage) -> Result<IndexStats> {
    let start = Instant::now();
    let mut stats = IndexStats::default();
    
    // 1. Discover all files
    let files = discover_files(&config.index_paths, &config.ignore_patterns)?;
    
    // 2. Group by language
    let mut by_language: HashMap<Language, Vec<PathBuf>> = HashMap::new();
    for file in files {
        if let Some(lang) = detect_language(&file) {
            by_language.entry(lang).or_default().push(file);
        }
    }
    
    // 3. Parse and index each language group
    for (lang, files) in by_language {
        let mut parser = Parser::new_for_language(lang)?;
        
        for file_path in files {
            // 3a. Compute hash
            let hash = compute_file_hash(&file_path)?;
            
            // 3b. Parse AST
            let content = std::fs::read_to_string(&file_path)?;
            let tree = parser.parse_file(&file_path, &content, lang)?;
            
            // 3c. Extract chunks
            let chunks = parser.extract_chunks(&tree, &content, lang)?;
            
            // 3d. Store chunks
            storage.insert_chunks(&chunks)?;
            
            // 3e. Update metadata
            let mtime = std::fs::metadata(&file_path)?.modified()?.as_secs();
            storage.update_file_hash(&file_path, &hash, mtime)?;
            
            stats.files_processed += 1;
            stats.chunks_indexed += chunks.len();
        }
    }
    
    stats.duration_ms = start.elapsed().as_millis() as u64;
    
    // 4. Update global stats
    storage.execute("UPDATE index_state SET value = ? WHERE key = 'total_files'", 
                    &[stats.files_processed.to_string()])?;
    storage.execute("UPDATE index_state SET value = ? WHERE key = 'total_chunks'", 
                    &[stats.chunks_indexed.to_string()])?;
    
    Ok(stats)
}
```

### 5.2 Incremental Update Algorithm

```rust
fn update_files(files: &[PathBuf], storage: &mut Storage) -> Result<IndexStats> {
    let mut stats = IndexStats::default();
    
    for file_path in files {
        // 1. Compute new hash
        let new_hash = compute_file_hash(file_path)?;
        
        // 2. Compare against stored hash
        let old_hash = storage.get_file_hash(file_path)?;
        
        if Some(&new_hash) == old_hash.as_ref() {
            continue;  // File unchanged, skip
        }
        
        // 3. Delete old chunks for this file
        storage.delete_chunks_for_file(file_path)?;
        
        // 4. Re-parse and re-index
        let lang = detect_language(file_path).ok_or("Unknown language")?;
        let mut parser = Parser::new_for_language(lang)?;
        let content = std::fs::read_to_string(file_path)?;
        let tree = parser.parse_file(file_path, &content, lang)?;
        let chunks = parser.extract_chunks(&tree, &content, lang)?;
        
        storage.insert_chunks(&chunks)?;
        
        // 5. Update metadata
        let mtime = std::fs::metadata(file_path)?.modified()?.as_secs();
        storage.update_file_hash(file_path, &new_hash, mtime)?;
        
        stats.files_processed += 1;
        stats.chunks_indexed += chunks.len();
    }
    
    Ok(stats)
}
```

### 5.3 Tree-sitter Query Language (Concrete Examples)

Tree-sitter uses S-expression queries to match AST nodes. Below are production-ready queries for the three target languages:

#### Python Extraction Queries

```scheme
; Extract all function definitions
(function_definition
  name: (identifier) @function.name
  parameters: (parameters) @function.params
  body: (block) @function.body) @function.definition

; Extract all class definitions
(class_definition
  name: (identifier) @class.name
  body: (block) @class.body) @class.definition

; Extract methods (functions inside classes)
(class_definition
  body: (block
    (function_definition
      name: (identifier) @method.name
      body: (block) @method.body) @method.definition))
```

#### TypeScript Extraction Queries

```scheme
; Function declarations
(function_declaration
  name: (identifier) @function.name
  parameters: (formal_parameters) @function.params
  body: (statement_block) @function.body) @function.definition

; Arrow functions (assigned to variables)
(variable_declarator
  name: (identifier) @function.name
  value: (arrow_function
    parameters: (_) @function.params
    body: (_) @function.body)) @function.definition

; Class declarations
(class_declaration
  name: (type_identifier) @class.name
  body: (class_body) @class.body) @class.definition

; Methods
(method_definition
  name: (property_identifier) @method.name
  parameters: (formal_parameters) @method.params
  body: (statement_block) @method.body) @method.definition
```

#### Go Extraction Queries

```scheme
; Function declarations
(function_declaration
  name: (identifier) @function.name
  parameters: (parameter_list) @function.params
  body: (block) @function.body) @function.definition

; Method declarations
(method_declaration
  receiver: (parameter_list) @method.receiver
  name: (field_identifier) @method.name
  parameters: (parameter_list) @method.params
  body: (block) @method.body) @method.definition

; Struct definitions
(type_declaration
  (type_spec
    name: (type_identifier) @struct.name
    type: (struct_type) @struct.body)) @struct.definition
```

### 5.4 Performance Targets (Indexing)

| **Operation**                 | **Target** | **Benchmark**                                 |
| ----------------------------- | ---------- | --------------------------------------------- |
| Cold index (10K LOC)          | <5s        | Parse 200 Python files, insert into SQLite    |
| Cold index (100K LOC)         | <30s       | Parse 2,000 Python files                      |
| Incremental update (10 files) | <2s        | Re-parse and re-index 10 modified files       |
| Hash computation (1K files)   | <500ms     | xxHash3-128 on 1,000 files (avg 500 LOC each) |

***

## 6. Retrieval Pipeline (Detailed)

### 6.1 Query Execution Flow

```rust
fn query(user_query: &str, options: QueryOptions, storage: &Storage) -> Result<QueryResult> {
    // 1. Preprocess query
    let tokens = preprocess_query(user_query);  // ["authenticate", "user"]
    let fts_query = tokens.join(" OR ");        // "authenticate OR user"
    
    // 2. Execute FTS5 search
    let sql = r#"
        SELECT 
            symbol_name,
            symbol_type,
            chunk_text,
            file_path,
            start_byte,
            end_byte,
            language,
            bm25(symbols) AS score
        FROM symbols
        WHERE symbols MATCH ?
        ORDER BY bm25(symbols)
        LIMIT ?
    "#;
    
    let raw_results = storage.query(sql, &[&fts_query, &options.max_results.to_string()])?;
    
    // 3. Apply token budget
    let packed_results = apply_token_budget(raw_results, options.max_tokens);
    
    // 4. Return results
    Ok(QueryResult {
        chunks: packed_results.clone(),
        total_tokens: packed_results.iter().map(|r| r.token_count).sum(),
        total_results_found: raw_results.len(),
    })
}
```

### 6.2 BM25 Ranking (SQLite FTS5)

SQLite FTS5 provides a built-in `bm25()` function that implements the BM25 ranking algorithm. **No custom implementation needed.**

**BM25 formula:**

```
score(D, Q) = Σ IDF(qi) × (f(qi, D) × (k1 + 1)) / (f(qi, D) + k1 × (1 - b + b × |D| / avgdl))
```

Where:

* `D` = document (code chunk)
* `Q` = query terms
* `f(qi, D)` = term frequency of `qi` in `D`
* `|D|` = document length
* `avgdl` = average document length
* `k1` = term saturation parameter (default: 1.2)
* `b` = length normalization (default: 0.75)

**FTS5 usage:**

```sql
SELECT *, bm25(symbols) AS score
FROM symbols
WHERE symbols MATCH 'authenticate OR user'
ORDER BY bm25(symbols)  -- Lower is better (negative log-likelihood)
LIMIT 20;
```

### 6.3 Token Budget Enforcement

```rust
fn apply_token_budget(results: Vec<SearchResult>, max_tokens: usize) -> Vec<SearchResult> {
    let mut packed = Vec::new();
    let mut total_tokens = 0;
    
    for result in results {
        let chunk_tokens = estimate_tokens(&result.chunk.chunk_text);
        
        if total_tokens + chunk_tokens > max_tokens {
            break;  // Budget exhausted
        }
        
        packed.push(result);
        total_tokens += chunk_tokens;
    }
    
    packed
}

// Fast token estimation (GPT-style: 1 token ≈ 4 chars)
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}
```

### 6.4 Output Formats

#### 6.4.1 TOON Format (Tree-sitter Outline Notation)

Used by Tree-sitter Analyzer MCP. Compact, LLM-friendly:

```
src/auth/handlers.py:45-67
src/auth/utils.py:12-34
src/db/session.py:89-120
```

**Format spec:**

* One file:line\_range per line
* Sorted by BM25 score (descending)
* Can be piped directly to `cat` or editor

#### 6.4.2 JSON Format

For programmatic use:

```json
{
  "query": "authenticate user",
  "total_results": 15,
  "total_tokens": 3842,
  "chunks": [
    {
      "symbol_name": "authenticate_user",
      "symbol_type": "function",
      "file_path": "src/auth/handlers.py",
      "start_byte": 1234,
      "end_byte": 1789,
      "language": "python",
      "bm25_score": -2.45,
      "chunk_text": "def authenticate_user(username, password):\n    ..."
    }
  ]
}
```

#### 6.4.3 Plain Text Format

Human-readable, for CLI display:

```
────────────────────────────────────────────────────────
🔍 Query: "authenticate user"
📊 Found 15 results (showing top 10, 3842 tokens)
────────────────────────────────────────────────────────

[1] src/auth/handlers.py:45-67 (score: -2.45)
def authenticate_user(username: str, password: str) -> Optional[User]:
    """Authenticate user with username and password."""
    user = get_user_by_username(username)
    if user and verify_password(password, user.password_hash):
        return user
    return None

[2] src/auth/utils.py:12-34 (score: -1.89)
def verify_password(plain: str, hashed: str) -> bool:
    """Verify plaintext password against hash."""
    return bcrypt.checkpw(plain.encode(), hashed.encode())

────────────────────────────────────────────────────────
```

***

## 7. CLI Design

### 7.1 Command Structure

```bash
codecache <COMMAND> [OPTIONS]

COMMANDS:
    init        Initialize a new CodeCache index in the current directory
    index       Build or rebuild the full index
    update      Incrementally update the index for specific files
    query       Search the codebase and retrieve relevant code snippets
    status      Show index statistics and health
    config      Manage configuration
    serve       Start an MCP server (for Claude Code integration)
    # (hidden) ingest  Insert pre-chunked records from JSON, bypassing parse/chunk — research-only (D25)

OPTIONS:
    -h, --help       Print help
    -V, --version    Print version
    -v, --verbose    Enable verbose logging
```

### 7.2 Detailed Command Specifications

#### `codecache init`

Initialize a new index database.

```bash
codecache init [OPTIONS]

OPTIONS:
    --db-path <PATH>         Database location [default: .codecache/index.db]
    --index-path <PATH>      Paths to index (can specify multiple) [default: .]
    --ignore <PATTERN>       Additional ignore patterns beyond .gitignore
    --languages <LANG,...>   Languages to index [default: python,typescript,go]

EXAMPLES:
    # Initialize in current directory
    codecache init
    
    # Initialize with custom paths
    codecache init --index-path src --index-path lib
    
    # Initialize, ignoring tests
    codecache init --ignore "**/*_test.go" --ignore "test_*.py"
```

**Generated files:**

* `.codecache/index.db` (SQLite database)
* `.codecache/config.toml` (configuration file)

#### `codecache index`

Build or rebuild the full index.

```bash
codecache index [OPTIONS]

OPTIONS:
    --full               Force full re-index (ignore existing hashes)
    --db-path <PATH>     Database location [default: .codecache/index.db]
    --progress           Show progress bar

EXAMPLES:
    # Incremental index (only changed files)
    codecache index
    
    # Full re-index
    codecache index --full
    
    # With progress
    codecache index --progress
```

**Output:**

```
🔍 Discovering files...
Found 2,458 files (Python: 1,234 | TypeScript: 987 | Go: 237)

🔄 Detecting changes...
Changed: 45 files | New: 12 files | Deleted: 3 files

⚙️  Indexing...
[████████████████████████████████████████] 100% (60/60 files)

✅ Indexing complete
   Files processed: 60
   Chunks indexed: 1,234
   Duration: 4.2s
   Index size: 8.3 MB
```

#### `codecache update`

Update specific files (useful for editor integrations or git hooks).

```bash
codecache update <FILE>... [OPTIONS]

ARGUMENTS:
    <FILE>...    Files to update (can use glob patterns)

OPTIONS:
    --db-path <PATH>    Database location [default: .codecache/index.db]

EXAMPLES:
    # Update a single file
    codecache update src/auth/handlers.py
    
    # Update multiple files
    codecache update src/auth/*.py
    
    # Update from git (modified files)
    git diff --name-only | xargs codecache update
```

#### `codecache ingest` (hidden — research-only, Decision Log D25)

Insert **caller-supplied, pre-chunked** records straight into the index from a JSON file, **bypassing**
file discovery, parsing, and chunking. This is a research seam (clap `hide = true`; not shown in `--help`)
for the R2 chunker ablation: it lets an external chunker's output flow through CodeCache's same storage +
FTS5-BM25 + retriever so the chunker is the only variable. Not part of the normal user workflow — use
`init`/`index` for real indexing.

```bash
codecache ingest <CHUNKS_JSON> [OPTIONS]

ARGUMENTS:
    <CHUNKS_JSON>    Path to a JSON file: an array of chunk records (schema below)

OPTIONS:
    --db-path <PATH>    Database location [default: .codecache/index.db]
```

**Input schema** — top-level is a JSON **array**; array order is insertion (rowid) order, so a fixed input
yields a deterministic `bm25 ASC, rowid ASC` ranking. Each record (a *fuller* shape than the lossy
query-output JSON of §6.4.2 — it carries every field the harness controls, enrichment included, so R2.3b
can hold enrichment constant):

```jsonc
[
  {
    // required
    "symbol_name": "authenticate_user",
    "symbol_type": "function",        // function | class | method | struct
    "file_path":   "src/auth/handlers.py",
    "start_byte":  1234,
    "end_byte":    1789,
    "start_line":  45,                // 1-based inclusive (D7)
    "end_line":    67,
    "chunk_text":  "def authenticate_user(): ...",
    "language":    "python",          // python | typescript | go
    // optional (defaults shown)
    "parent_symbol":    null,         // string | null
    "file_docstring":   null,         // string | null
    "imports":          [],           // string[]
    "cross_references": [],           // string[]
    "is_heuristic":     false         // bool (passed in-memory; storage has no column yet — dropped)
  }
]
```

Required fields missing, an unknown `symbol_type`/`language` string, a wrong JSON type, or malformed JSON
all surface a typed error → **nonzero exit**, never a panic. Empty input (`[]`) is a valid no-op (0 files,
0 chunks, exit 0). After insertion, one `files_metadata` row is written per distinct `file_path` (so
`status` and `codecache_outline` see the data) and `index_state` `total_files`/`total_chunks` are
restamped. **Re-ingest / incremental update is out of scope** — the harness `init`s a fresh DB per arm.
Implemented by `app::ingest_chunks` (§3.2.4) over the existing `Storage::insert_chunks`; serde/serde_json
are already in the tree, so this adds **no dependency**.

#### `codecache query`

Search and retrieve code snippets.

```bash
codecache query <QUERY> [OPTIONS]

ARGUMENTS:
    <QUERY>    Search query (free-form text)

OPTIONS:
    --max-tokens <N>        Maximum tokens in output [default: 4000]
    --max-results <N>       Maximum number of results [default: 20]
    --format <FORMAT>       Output format: toon|json|text [default: text]
    --file-filter <GLOB>    Restrict search to files matching glob
    --bm25-weights <W>      7 comma-separated f64 per-column BM25 weights, in indexed-column
                            order: symbol_name,symbol_type,chunk_text,parent_symbol,imports,
                            cross_references,file_docstring. Omitted ⇒ the built-in defaults
                            10,1,1,5,2,2,2. (R2.2a / D24 — for the R2 weight-sweep harness;
                            malformed or non-7-value input is a clean error, never a panic.)
    --db-path <PATH>        Database location [default: .codecache/index.db]

EXAMPLES:
    # Basic query
    codecache query "authenticate user"
    
    # Limit tokens (for LLM context budget)
    codecache query "error handling" --max-tokens 2000
    
    # JSON output (for programmatic use)
    codecache query "database connection" --format json
    
    # TOON format (for editor integration)
    codecache query "parse config" --format toon
    
    # Filter to specific directory
    codecache query "auth" --file-filter "src/auth/**"

    # Override BM25 per-column weights (R2 ranking sweep; default is 10,1,1,5,2,2,2)
    codecache query "authenticate user" --bm25-weights "5,1,3,2,1,1,1"
```

#### `codecache status`

Show index health and statistics.

```bash
codecache status [OPTIONS]

OPTIONS:
    --db-path <PATH>    Database location [default: .codecache/index.db]

EXAMPLES:
    codecache status
```

**Output:**

```
📊 CodeCache Index Status
────────────────────────────────────────
Version:          0.1.0
Created:          2026-06-01 14:23:11 UTC
Last full index:  2026-06-09 09:15:42 UTC
Last update:      2026-06-09 10:03:29 UTC

📁 Files
  Total:          2,458
  Python:         1,234 (50.2%)
  TypeScript:       987 (40.1%)
  Go:              237 (9.6%)

📦 Chunks
  Total:         18,945
  Functions:     15,234 (80.4%)
  Classes:        2,456 (13.0%)
  Methods:        1,255 (6.6%)

💾 Storage
  Database size:   8.3 MB
  Index size:      7.8 MB
  Metadata size:   0.5 MB
────────────────────────────────────────
```

#### `codecache config`

Read or write configuration values in `.codecache/config.toml` (Decision Log D18).

```bash
codecache config [KEY] [VALUE] [OPTIONS]

ARGUMENTS:
    [KEY]      Config key to read or set (omit to print the whole resolved config)
    [VALUE]    New value to set for KEY (omit to read KEY)

OPTIONS:
    --db-path <PATH>    Database location [default: .codecache/index.db]

EXAMPLES:
    # Print the current resolved configuration
    codecache config

    # Set a value and persist it back to .codecache/config.toml
    codecache config storage.max_db_size_mb 1000
```

Backed by `Config::load` (read) + the additive `Config::save` (write, D18). Writes never clobber
unrelated keys (the full `Config` is round-tripped through TOML). Unknown keys / malformed values
exit nonzero with a message; no panic.

> **Empty-result text output (M7.3):** when `query` finds no matches AND `--format text` (the
> default), the CLI prints `No results found.` rather than the formatter's empty-text header (which
> echoes the query). This is a CLI presentation choice; the pure `formatter` empty-text shape (§6.4.3)
> is unchanged, and `--format json`/`toon` always render through the formatter.

#### `codecache serve`

Start MCP server for agent integration.

```bash
codecache serve [OPTIONS]

OPTIONS:
    --transport <TYPE>    Transport type: stdio|sse [default: stdio]
    --port <PORT>         Port for SSE transport [default: 3000]
    --db-path <PATH>      Database location [default: .codecache/index.db]

EXAMPLES:
    # Start stdio server (for Claude Code)
    codecache serve --transport stdio
    
    # Start SSE server (for web clients)
    codecache serve --transport sse --port 3000
```

### 7.3 Configuration File (`.codecache/config.toml`)

```toml
# CodeCache configuration
version = "0.1.0"

# Paths to index
index_paths = ["src", "lib"]

# Ignore patterns (in addition to .gitignore)
ignore_patterns = [
    "**/*.test.py",
    "**/test_*.py",
    "**/*.spec.ts",
    "**/*_test.go",
]

# Languages to index
languages = ["python", "typescript", "go"]

# Storage settings
[storage]
db_path = ".codecache/index.db"
max_db_size_mb = 500

# Retrieval settings
[retrieval]
default_max_tokens = 4000
default_max_results = 20
bm25_k1 = 1.2  # Term saturation
bm25_b = 0.75  # Length normalization

# MCP server settings
[mcp]
transport = "stdio"
sse_port = 3000
```

***

## 8. Claude Code / Agent Integration

### 8.1 Integration Methods

<!-- Copilot-Researcher-Visualization -->



### 8.2 MCP Server Implementation

The Model Context Protocol (MCP) is Anthropic's standard for connecting AI tools. CodeCache will expose three MCP tools.

**Agent-first output ordering (D13).** Tool output is ordered for the *agent's* next action:
symbol name, qualified parent, `file:start-end`, and a one-line signature first; full bodies
only within the remaining token budget. The **text** formatter (§6.4.3) realizes this ordering —
each result emits its locating header + one-line signature before its body. The **TOON** format
(§6.4.1) stays the compact `file:start-end`-per-line list it is defined as (it carries no bodies,
so there is nothing to order after the locator — it is already "locator-only", the strongest
agent-first form); the **JSON** format (§6.4.2) is field-keyed, so order is not semantic. The M8
`codecache_outline` skeleton output reuses the text formatter's signature-before-body line shape.

**Self-healing search (D14).** Before answering, `codecache_search` hash-checks the files
implicated by the top results (cheap — hashes are stored, §4.4) and transparently re-indexes
any that changed, so results are correct-by-construction and never stale. A result file deleted
from disk is **evicted** (its stale chunks + `files_metadata` row dropped) and removed from the
answer — never a panic. The heal cost is bounded by the result count (only surfaced files are
checked). The window is keyed off the **stored §4.4 hash**: a result whose file has no
`files_metadata` row (e.g. content inserted into the index without an on-disk source) has no
staleness window and is left untouched. Implemented at **M8.4**; the per-search staleness metric
(`files_checked` / `files_reindexed` / `files_dropped`) is exposed via
`mcp_server::CodeCacheServer::staleness_handle()` (overview §5.2 Layer 3).

#### Tool 1: `codecache_search`

```json
{
  "name": "codecache_search",
  "description": "Search the codebase for relevant functions, classes, or code snippets using semantic queries. Returns concentrated code context optimized for token budgets.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Free-form search query (e.g., 'authentication logic', 'error handling')"
      },
      "max_tokens": {
        "type": "integer",
        "description": "Maximum tokens to return (for context budget)",
        "default": 4000
      },
      "file_filter": {
        "type": "string",
        "description": "Optional glob pattern to filter files (e.g., 'src/auth/**')",
        "default": null
      }
    },
    "required": ["query"]
  }
}
```

**Example invocation:**

```json
{
  "tool": "codecache_search",
  "arguments": {
    "query": "authenticate user with password",
    "max_tokens": 3000
  }
}
```

**Example response:**

````json
{
  "content": [
    {
      "type": "text",
      "text": "# CodeCache Search Results\n\nQuery: \"authenticate user with password\"\nFound 8 results (2,847 tokens)\n\n## 1. src/auth/handlers.py:45-67\n```python\ndef authenticate_user(username: str, password: str) -> Optional[User]:\n    \"\"\"Authenticate user with username and password.\"\"\"\n    user = get_user_by_username(username)\n    if user and verify_password(password, user.password_hash):\n        return user\n    return None\n```\n\n## 2. src/auth/utils.py:12-34\n```python\ndef verify_password(plain: str, hashed: str) -> bool:\n    \"\"\"Verify plaintext password against hash.\"\"\"\n    return bcrypt.checkpw(plain.encode(), hashed.encode())\n```\n\n..."
    }
  ]
}
````

#### Tool 2: `codecache_update`

```json
{
  "name": "codecache_update",
  "description": "Incrementally update the CodeCache index for specific files. Call this after modifying code to ensure fresh search results.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "files": {
        "type": "array",
        "items": {"type": "string"},
        "description": "List of file paths to re-index"
      }
    },
    "required": ["files"]
  }
}
```

#### Tool 3: `codecache_outline` (D13)

```json
{
  "name": "codecache_outline",
  "description": "Return the symbol skeleton (functions, classes, methods with signatures and line ranges) of a file or directory. The cheapest way to orient in unfamiliar code before reading bodies.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": {
        "type": "string",
        "description": "File or directory path to outline (relative to the indexed root)"
      },
      "max_tokens": {
        "type": "integer",
        "description": "Maximum tokens to return",
        "default": 2000
      }
    },
    "required": ["path"]
  }
}
```

Outline rows come straight from the index (`symbols`: symbol name, type, `parent_symbol`,
`start_line`/`end_line` — D7), so no source files are read at query time. This is the
repo-map primitive aider validated; for CodeCache it is one indexed lookup —
`Storage::symbols_for_path` (Decision Log **D19**), a path-scoped column `SELECT` over the
contentful `symbols` table for an exact file OR a directory prefix (`<dir>/%`), ordered by
`(file_path, start_line, end_line)`. The handler formats the returned `SymbolOutline` rows via the
§6.4.3 text skeleton-line shape (signature/locator before bodies — D13).

### 8.3 MCP Server Pseudocode

```rust
// mcp_server/mod.rs
use mcp_sdk::{Server, Tool, ToolCall, ToolResult, Transport};

pub struct CodeCacheMCPServer {
    storage: Storage,
    retriever: Retriever,
    indexer: Indexer,
}

impl CodeCacheMCPServer {
    pub fn new(db_path: &Path) -> Result<Self> {
        // D8: Storage holds an Arc<Mutex<Connection>>; `.clone()` shares the same connection
        // (cheap Arc clone) rather than re-opening the DB or cloning a raw Connection.
        let storage = Storage::new(db_path)?;
        let retriever = Retriever::new(storage.clone());
        let indexer = Indexer::new(Config::load()?, storage.clone())?;
        
        Ok(Self { storage, retriever, indexer })
    }
    
    pub fn run(&mut self, transport: Transport) -> Result<()> {
        let mut server = Server::new(transport);
        
        // Register tools
        server.register_tool(Tool {
            name: "codecache_search".to_string(),
            description: "Search codebase for relevant code snippets".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "max_tokens": {"type": "integer", "default": 4000},
                    "file_filter": {"type": "string", "default": null}
                },
                "required": ["query"]
            }),
            handler: Box::new(|call: ToolCall| self.handle_search(call)),
        });
        
        server.register_tool(Tool {
            name: "codecache_update".to_string(),
            description: "Update index for specific files".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "files": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["files"]
            }),
            handler: Box::new(|call: ToolCall| self.handle_update(call)),
        });
        
        // Start server
        server.serve()?;
        Ok(())
    }
    
    fn handle_search(&self, call: ToolCall) -> Result<ToolResult> {
        let query: String = call.arguments.get("query")?.as_str()?;
        let max_tokens: usize = call.arguments.get("max_tokens")?.as_u64()? as usize;
        
        let results = self.retriever.query(&query, QueryOptions {
            max_tokens,
            ..Default::default()
        })?;
        
        let formatted = format_results_for_llm(&results);
        
        Ok(ToolResult {
            content: vec![Content::Text(formatted)],
        })
    }
    
    fn handle_update(&mut self, call: ToolCall) -> Result<ToolResult> {
        let files: Vec<PathBuf> = call.arguments.get("files")?.as_array()?
            .iter()
            .map(|v| PathBuf::from(v.as_str()?))
            .collect();
        
        let stats = self.indexer.update_files(&files)?;
        
        Ok(ToolResult {
            content: vec![Content::Text(format!(
                "Updated {} files, indexed {} chunks in {}ms",
                stats.files_processed,
                stats.chunks_indexed,
                stats.duration_ms
            ))],
        })
    }
}
```

### 8.4 Claude Code Configuration

Add to `~/.config/claude-code/mcp.json`:

```json
{
  "mcpServers": {
    "codecache": {
      "command": "codecache",
      "args": ["serve", "--transport", "stdio"],
      "cwd": "/path/to/your/project"
    }
  }
}
```

### 8.5 End-to-End Workflow Example

**Scenario:** Developer asks Claude Code to add authentication to an API endpoint.

```
User: Add JWT authentication to the /api/users endpoint

Claude Code (internal):
  1. Calls `codecache_search("authentication JWT")`
  2. Receives:
     - src/auth/jwt.py:create_access_token()
     - src/auth/middleware.py:verify_jwt()
     - src/api/routes.py:protected_route_example()
  3. Incorporates snippets into context
  4. Generates code modification
  5. Applies changes to src/api/users.py
  6. Calls `codecache_update(["src/api/users.py"])` to re-index

Claude Code (to user):
  "I've added JWT authentication to /api/users using the verify_jwt 
   middleware pattern from your existing auth module. The endpoint now 
   requires a valid JWT token in the Authorization header."
```

**Token savings:**

* Without CodeCache: 15K tokens (full context dump of 5 auth files)
* With CodeCache: 3K tokens (3 targeted snippets)
* **Savings: 80%**

***

## 9. First-Release Scope (v0.1)

### 9.1 Included Features

<!-- Copilot-Researcher-Visualization -->



**Concrete deliverables:**

1. **Rust CLI binary** (`codecache`) with all commands
2. **SQLite database schema** with FTS5 virtual table
3. **MCP server implementation** with stdio transport
4. **Documentation**:
   * README.md with quickstart
   * ARCHITECTURE.md (this document)
   * CLAUDE\_CODE\_SETUP.md (integration guide)
5. **Benchmarks**: Token reduction validation on 5 real-world tasks

### 9.2 Deferred Features (v0.2+)

| **Feature**                                | **Rationale for Deferral**                                                         | **Estimated Effort** |
| ------------------------------------------ | ---------------------------------------------------------------------------------- | -------------------- |
| **Embeddings-based retrieval**             | AST+BM25 proves 80% of value; embeddings add complexity (model hosting, vector DB) | 2-3 weeks            |
| **Call graph analysis**                    | Requires type inference and cross-file analysis; different problem domain          | 3-4 weeks            |
| **Additional languages** (Rust, Java, C++) | Pareto principle: Python/TS/Go cover 90% of users                                  | 1 week per language  |
| **Real-time file watching**                | Battery drain on laptops; explicit `update` is sufficient                          | 1 week               |
| **Web UI**                                 | CLI-first design; web UI is nice-to-have                                           | 2-3 weeks            |
| **Multi-repo support**                     | Single-repo is 95% use case; multi-repo adds complexity                            | 1-2 weeks            |

### 9.3 Success Metrics (v0.1)

| **Metric**      | **Measurement Method**                         | **Pass Criteria**           |
| --------------- | ---------------------------------------------- | --------------------------- |
| Token/turn economy (D16) | ContextBench-Lite gold-context scoring (Layer 1) + same-agent tool-swap study (Layer 2; research track R1–R3, `project_overview.md` §5) | Dominance over grep-only at matched retrieval recall, with bootstrap CIs |
| Query latency   | Measure p50, p95, p99 on 100K LOC repo         | p95 <500ms                  |
| Index size      | Measure SQLite DB size for Django codebase     | <100MB                      |
| User adoption   | GitHub stars + npm/crates downloads (3 months) | >500 stars OR >1K downloads |

***

## 10. Implementation Plan

### 10.1 Phased Roadmap

<!-- Copilot-Researcher-Visualization -->



### 10.2 Technology Stack

| **Component** | **Technology**              | **Rationale**                                                                           |
| ------------- | --------------------------- | --------------------------------------------------------------------------------------- |
| **Language**  | Rust                        | Performance (crucial for parsing), memory safety, excellent CLI ecosystem (clap, tokio) |
| **Parser**    | Tree-sitter (Rust bindings) | Industry-standard AST parser, incremental parsing, 40+ languages                        |
| **Storage**   | SQLite + FTS5               | Zero-setup, FTS5 for BM25 ranking, portable                                             |
| **Hashing**   | xxHash3-128 (`xxhash-rust`) | 10× faster than SHA-256, sufficient collision resistance                                |
| **CLI**       | `clap` v4                   | Derive-based API, auto-generated help, robust arg parsing                               |
| **JSON**      | `serde_json`                | De-facto Rust JSON library                                                              |
| **MCP**       | Hand-rolled JSON-RPC 2.0 over stdio (`serde`/`serde_json` only) — **ratified, D15 (2026-06-12)** | v0.1 hand-rolls the stdio MCP adapter: zero new runtime deps, no tokio/async over the synchronous SQLite core (D8), and no MSRV conflict with the deliberate 1.85.0 pin (D10). The official `rmcp` SDK was evaluated and deferred to v0.2 (SSE/HTTP transports, D4) — see ROADMAP D15. The `mcp_server` module stays behind the D4 transport-agnostic seam so `rmcp` can be swapped in later as an adapter change. |

### 10.3 Key Dependencies (Rust `Cargo.toml`)

```toml
[package]
# Package/crate name is `codecache-rs` to clear the crates.io name conflict (ROADMAP D30); the
# produced *binary* stays `codecache` (`[[bin]] name` below) — the ripgrep crate≠binary model.
name = "codecache-rs"
version = "0.1.0"
edition = "2021"
# Publish allowlist (ROADMAP D31): ships ONLY product code to crates.io — 52 files, no
# research/.claude/docs/CLAUDE.md. Root-level patterns are leading-`/` anchored (cargo's
# gitignore-style globs match a bare filename at ANY depth, else nested READMEs leak).
include = [
    "src/**/*.rs", "src/**/*.scm",
    "benches/**/*.rs", "examples/**/*.rs",
    "/README.md", "/LICENSE-MIT", "/LICENSE-APACHE",
    "/CHANGELOG.md", "/CONTRIBUTING.md", "/rust-toolchain.toml",
]

[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }
anyhow = "1.0"  # Error handling

# Parsing
tree-sitter = "0.24"
tree-sitter-python = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-go = "0.23"

# Storage. FTS5 is compiled into the `bundled` SQLite amalgamation by default; rusqlite 0.32 has
# no separate `fts5` feature. See ROADMAP Decision Log D9.
rusqlite = { version = "0.32", features = ["bundled"] }

# Hashing
xxhash-rust = { version = "0.8", features = ["xxh3"] }

# Serialization. `serde` + `serde_json` also back the M8 hand-rolled MCP JSON-RPC adapter
# (D15, 2026-06-12) — no new runtime dep is needed for the stdio MCP server.
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# File system
ignore = "0.4"  # Respect .gitignore
walkdir = "2.5"

# Utilities
once_cell = "1.19"  # Lazy statics
regex = "1.10"

[dev-dependencies]
criterion = "0.5"  # Benchmarking
tempfile = "3.10"  # Testing
```

### 10.4 Contributor Workflow

**For new contributors:**

1. **Clone repo**: `git clone https://github.com/AdvancedUno/codecache.git`
2. **Install Rust**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
3. **Build**: `cargo build --release`
4. **Run tests**: `cargo test`
5. **Run benchmarks**: `cargo bench`
6. **Check formatting**: `cargo fmt --check`
7. **Check lints**: `cargo clippy -- -D warnings`

**Project structure:**

```
codecache/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── cli/                 # Command implementations
│   │   ├── mod.rs
│   │   ├── init.rs
│   │   ├── index.rs
│   │   ├── query.rs
│   │   └── serve.rs
│   ├── indexer/             # Indexing pipeline
│   │   ├── mod.rs
│   │   ├── discovery.rs     # File discovery
│   │   └── pipeline.rs      # Orchestration
│   ├── parser/              # Tree-sitter integration
│   │   ├── mod.rs
│   │   ├── python.rs
│   │   ├── typescript.rs
│   │   └── go.rs
│   ├── storage/             # SQLite interface
│   │   ├── mod.rs
│   │   ├── schema.rs
│   │   └── queries.rs
│   ├── retriever/           # Query execution
│   │   ├── mod.rs
│   │   └── ranking.rs
│   ├── hasher/              # File hashing
│   │   └── mod.rs
│   ├── formatter/           # Output formatters
│   │   ├── mod.rs
│   │   ├── toon.rs
│   │   ├── json.rs
│   │   └── text.rs
│   ├── mcp_server/          # MCP integration
│   │   ├── mod.rs
│   │   └── tools.rs
│   └── config/              # Configuration
│       └── mod.rs
├── tests/                   # Integration tests
│   ├── indexing_tests.rs
│   ├── retrieval_tests.rs
│   └── e2e_tests.rs
├── benches/                 # Benchmarks
│   ├── indexing_bench.rs
│   └── query_bench.rs
├── docs/
│   ├── ARCHITECTURE.md      # This document
│   ├── CLAUDE_CODE_SETUP.md # Integration guide
│   └── CONTRIBUTING.md      # Contributor guide
├── examples/
│   └── django_benchmark/    # Sample codebase for testing
├── Cargo.toml
├── README.md
└── LICENSE
```

***

## 11. Performance & Scalability Considerations

### 11.1 Monorepo Scalability

<!-- Copilot-Researcher-Visualization -->



**Scaling strategies (for future):**

1. **Parallel parsing**: Parse files concurrently (Rayon threadpool)
2. **Batch inserts**: Insert chunks in batches of 1000 (SQLite transaction overhead)
3. **Streaming results**: Return top-K results before full ranking completes
4. **Index partitioning**: Shard by directory (for >1M LOC repos)

### 11.2 Query Latency Breakdown

Target: <500ms p95 on 100K LOC repo

| **Operation**         | **Expected Latency** | **Optimization**                                 |
| --------------------- | -------------------- | ------------------------------------------------ |
| FTS5 search (top-100) | <50ms                | SQLite FTS5 uses inverted index; inherently fast |
| BM25 scoring          | <10ms                | Built-in FTS5 function                           |
| Snippet extraction    | <20ms                | Byte-range seeks in SQLite BLOB storage          |
| Token counting        | <10ms                | Simple char-count heuristic (1 token ≈ 4 chars)  |
| Formatting            | <10ms                | String concatenation                             |
| **Total**             | **<100ms**           | **Well under 500ms target**                      |

**Worst-case scenario (cold cache):**

* SQLite page cache miss → disk I/O
* Expected: +100-200ms on HDD, +10-20ms on SSD
* Mitigation: Recommend SSD for production use

### 11.3 Memory Footprint

| **Component**      | **Memory Usage**    | **Notes**                            |
| ------------------ | ------------------- | ------------------------------------ |
| Tree-sitter parser | \~50MB per language | Grammars are loaded on-demand        |
| SQLite cache       | \~64MB (default)    | Configurable via `PRAGMA cache_size` |
| In-flight chunks   | \~10MB              | Bounded by `max_results` limit       |
| **Total**          | **\~150MB**         | Acceptable for CLI tool              |

### 11.4 Disk I/O Optimization

**Incremental updates:**

```rust
// Efficient file change detection
fn detect_changes(storage: &Storage, files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    
    for file in files {
        let new_hash = compute_file_hash(file)?;
        let old_hash = storage.get_file_hash(file)?;
        
        if Some(&new_hash) != old_hash.as_ref() {
            changed.push(file.clone());
        }
    }
    
    Ok(changed)
}
```

**Cost analysis:**

* 10,000 files × 500 bytes/file × 10GB/s (xxHash3 throughput) = **\~500ms** for full hash scan
* Only changed files are re-parsed (typical: <1% per commit)

***

## 12. Risks & Trade-offs

### 12.1 Critical Risks

<!-- Copilot-Researcher-Visualization -->



### 12.2 Technical Trade-offs

| **Decision**                 | **Pro**                     | **Con**                             | **Mitigation**                                   |
| ---------------------------- | --------------------------- | ----------------------------------- | ------------------------------------------------ |
| **AST-only (no embeddings)** | Simple, fast, deterministic | Semantic gap on 30-40% of queries   | Add embeddings in v0.2 with explicit toggle      |
| **SQLite (not Postgres)**    | Zero setup, portable        | Limited concurrency (single-writer) | Acceptable for local CLI use                     |
| **Rust (not Python)**        | Performance, memory safety  | Steeper learning curve              | Provide Docker image for non-Rust users          |
| **xxHash (not SHA-256)**     | 10× faster                  | Not cryptographically secure        | Collision resistance sufficient for file hashing |
| **3 languages only (v0.1)**  | Faster time-to-market       | Excludes Java/C++ users             | Pareto principle: 90% coverage with 3 languages  |

### 12.3 Open-Source Sustainability

**Maintenance risks:**

* Tree-sitter grammar updates (quarterly)
* SQLite FTS5 API changes (rare, but possible)
* MCP protocol evolution (Anthropic controls spec)

**Mitigation strategies:**

1. **Pin dependencies**: Lock Tree-sitter grammar versions in Cargo.toml
2. **Automated testing**: CI/CD on every commit (GitHub Actions)
3. **Contributor guidelines**: Clear CONTRIBUTING.md with code style, testing requirements
4. **Versioned releases**: Semantic versioning (v0.1.0, v0.2.0, ...)
5. **Community engagement**: Discord/Slack for user support, GitHub Discussions for feature requests

***

## 13. Future Extensions (Post-v1)

### 13.1 Hybrid Retrieval (AST + Embeddings)

**Problem:** AST-only retrieval fails on semantic queries ("find all error handling").

**Solution:** Hypothetical Prompt Embeddings (HyPE)

```rust
// Future: embeddings module
pub struct HybridRetriever {
    ast_retriever: Retriever,
    embedding_model: CodeBERTModel,  // Local model via onnxruntime
    vector_index: FAISSIndex,         // FAISS for vector search
}

impl HybridRetriever {
    pub fn query_hybrid(&self, query: &str, options: QueryOptions) -> Result<QueryResult> {
        // 1. AST retrieval (top-100 via BM25)
        let ast_results = self.ast_retriever.query(query, options)?;
        
        // 2. Embed query
        let query_embedding = self.embedding_model.encode(query)?;
        
        // 3. Embed top-100 chunks
        let chunk_embeddings: Vec<_> = ast_results.chunks
            .iter()
            .map(|c| self.embedding_model.encode(&c.chunk.chunk_text))
            .collect();
        
        // 4. Re-rank by cosine similarity
        let reranked = rerank_by_similarity(&ast_results, query_embedding, chunk_embeddings);
        
        Ok(reranked)
    }
}
```

**Estimated effort:** 2-3 weeks (model integration, vector DB, API)

### 13.2 Call Graph Analysis

**Problem:** Users ask "what calls this function?" → requires cross-file analysis.

**Solution:** Build a call graph during indexing.

```sql
CREATE TABLE call_graph (
    caller_symbol TEXT NOT NULL,
    caller_file TEXT NOT NULL,
    callee_symbol TEXT NOT NULL,
    callee_file TEXT NOT NULL,
    line_number INTEGER NOT NULL,
    FOREIGN KEY (caller_file) REFERENCES files_metadata(file_path),
    FOREIGN KEY (callee_file) REFERENCES files_metadata(file_path)
);

CREATE INDEX idx_callee ON call_graph(callee_symbol, callee_file);
```

**Challenge:** Requires type inference (e.g., resolving `user.save()` → `User.save()` method).

**Estimated effort:** 3-4 weeks (type analysis, graph construction)

### 13.3 Additional Languages

**Next batch:** Rust, Java, C++

**Effort per language:**

* Tree-sitter grammar integration: 2 days
* AST query patterns: 2 days
* Testing: 1 day
* **Total:** \~1 week per language

***

## 14. Conclusion

**CodeCache is a technically feasible, economically motivated project** that addresses a real pain point in AI-driven coding workflows: **inefficient context management**.

**Key strengths:**

* ✅ **Proven architecture** (AST chunking + BM25 ranking used by Cursor, Tree-sitter Analyzer)
* ✅ **Strong economics** (50-70% token reduction = $1K–$2K/year savings per agent)
* ✅ **Local-first** (zero latency, no API keys, no cloud dependencies)
* ✅ **Concrete scope** (Python/TS/Go for v0.1; AST-only retrieval)

**Key weaknesses:**

* ⚠️ **Semantic gap** (30-40% of queries need embeddings, deferred to v0.2)
* ⚠️ **Maintenance burden** (Tree-sitter grammar fragility, multi-language support)
* ⚠️ **Competitive/crowding risk** — agentic (grep-in-a-loop) search is the industry default,
  and the adjacent niches are taken (claude-context: cloud/embedding hybrid; Serena: LSP-based).
  CodeCache's wedge is **zero-dependency determinism + self-healing freshness** — neither
  competitor can structurally follow. Landscape analysis: `../project_overview.md` §2/§7.

**Recommendation for v0.1:**

1. **Ship AST-only version** in 10 weeks
2. **Validate token savings** with 5 real-world benchmarks
3. **Integrate with Claude Code** via MCP protocol
4. **Gather user feedback** before adding embeddings (v0.2)

**Success hinges on execution speed.** The technical primitives are well-understood; the differentiation is **usability + community trust**. Ship fast, iterate based on real usage data, and maintain a tight feedback loop with early adopters.

***

**This specification is comprehensive enough for multiple contributors to begin implementation immediately.** All core data structures, APIs, CLI commands, and integration methods are defined with concrete pseudocode and examples. The phased roadmap provides clear milestones, and the risk analysis prepares the team for likely challenges.

**Next steps:**

1. Set up Rust project scaffolding
2. Implement storage module (SQLite + FTS5 schema)
3. Integrate Tree-sitter for Python (first language)
4. Build end-to-end indexing + query pipeline
5. Benchmark against Claude Code on real tasks

**Let's build this.**

***

> **Note on scope & open decisions.** This document is the product + architecture spec (the
> *what/why*). For engineering execution — module ownership, the TDD workflow, milestones,
> and the test matrix — see [`ENGINEERING_PLAN.md`](ENGINEERING_PLAN.md),
> [`ROADMAP.md`](ROADMAP.md), and [`TEST_STRATEGY.md`](TEST_STRATEGY.md). Design critiques
> raised during review (D1 hybrid AST+embeddings retrieval, D2 graceful Tree-sitter
> degradation, D3 chunk-metadata enrichment, D4 MCP decoupling via HTTP/LSP) and the
> clarifications surfaced during phase planning (D5 `crate::types` location, D6 `FileMeta`
> write signature, D7 stored line numbers, D8 `Arc<Mutex<Connection>>` ownership) are tracked
> in the **Decision Log** in [`ROADMAP.md`](ROADMAP.md#decision-log). D5–D7 are reflected in
> §3.2 / §4.1 / §4.3 above; D8 in §3.2.3 / §8.3.
