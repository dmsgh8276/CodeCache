//! Storage: SQLite interface — schema (FTS5 `symbols`, `files_metadata`, `index_state`),
//! insert/query/delete, BM25 search.
//!
//! API anchor: `project_plan.md` §3.2.2 / §4.1. `Storage` wraps `Arc<Mutex<Connection>>`
//! (Decision Log **D8**) so it is cheaply `Clone` and the MCP server (M8) can lend one
//! connection to both `Retriever` and `Indexer`. Owner: `principal-engineering-lead` +
//! `rust-treesitter-specialist` (FTS5). Scenarios: `docs/TEST_STRATEGY.md#storage-sqlite--fts5`.
//!
//! No reachable `unwrap()/expect()/panic!`: every fallible step returns [`StorageError`] via `?`,
//! including the `Mutex` lock (a poisoned lock is mapped to a typed error rather than panicking).

mod queries;
mod schema;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{Chunk, FileMeta, Language, SymbolOutline, SymbolType};

/// A typed storage error. Wraps the underlying SQLite error plus the cases this layer adds
/// (poisoned lock, unparseable stored enum). Never panics.
#[derive(Debug)]
pub enum StorageError {
    /// An underlying `rusqlite`/SQLite error (open, prepare, execute, corrupt db, …).
    Sqlite(rusqlite::Error),
    /// The shared connection `Mutex` was poisoned by a panic in another holder.
    LockPoisoned,
    /// A stored row held a value this layer could not interpret (e.g. an unknown `language` or
    /// `symbol_type` string), indicating a corrupt or forward-version row.
    CorruptRow(String),
    /// A caller-supplied BM25 column weight was not finite (`NaN`/±`inf`). Such a value cannot be
    /// rendered as a SQL numeric literal, so it is rejected here rather than emitted into the query
    /// (R2.2a / D24). The CLI validates finiteness first, so this is a defensive storage-level guard.
    NonFiniteWeight(f64),
    /// A single [`write_in_transaction`](Storage::write_in_transaction) item's closure failed and its
    /// SAVEPOINT was rolled back, isolating the failure from its committed siblings (Decision Log
    /// **D20**). Carries the underlying per-item error's description. This is the storage-layer signal
    /// a caller (the indexer) maps a non-storage per-item failure into so the item rolls back and is
    /// counted-as-skipped; it never aborts the outer transaction.
    BatchItem(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Sqlite(e) => write!(f, "sqlite error: {e}"),
            StorageError::LockPoisoned => write!(f, "storage connection lock was poisoned"),
            StorageError::CorruptRow(what) => write!(f, "corrupt stored row: {what}"),
            StorageError::NonFiniteWeight(w) => {
                write!(f, "BM25 column weight must be finite, got {w}")
            }
            StorageError::BatchItem(what) => {
                write!(f, "batch item failed and was rolled back: {what}")
            }
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        StorageError::Sqlite(e)
    }
}

/// Convenience alias for storage results.
pub type Result<T> = std::result::Result<T, StorageError>;

/// One search hit: the reconstructed [`Chunk`] plus its BM25 score (lower is more relevant).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// The matched chunk, with all columns reconstructed.
    pub chunk: Chunk,
    /// FTS5 `bm25()` score; more negative = better match.
    pub bm25_score: f64,
}

/// SQLite-backed storage. Cheap to [`Clone`] — clones share one underlying connection (D8).
#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    /// Open (or create) the database at `db_path`. Does not create the schema — call
    /// [`Storage::init_schema`]. A non-SQLite/corrupt file surfaces as [`StorageError::Sqlite`].
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Storage {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Lock the shared connection, mapping a poisoned lock to a typed error (no panic).
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| StorageError::LockPoisoned)
    }

    /// Create the schema if absent and migrate an older database forward. Idempotent:
    /// re-running on a current database is a no-op. Uses `CREATE ... IF NOT EXISTS` and
    /// `INSERT OR IGNORE`, so it never clobbers live data.
    pub fn init_schema(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute_batch(&format!(
            "{}\n{}\n{}\n{}\n{}",
            schema::CREATE_SYMBOLS,
            schema::CREATE_FILES_METADATA,
            schema::CREATE_FILES_INDEXES,
            schema::CREATE_INDEX_STATE,
            schema::SEED_INDEX_STATE,
        ))?;
        drop(conn);
        self.migrate()?;
        Ok(())
    }

    /// Bring an older database up to [`schema::CURRENT_VERSION`]. For v0.1 there is a single
    /// version, so migration is "stamp current version forward"; future versions add ordered
    /// steps here keyed on the stored `index_state.version`.
    fn migrate(&self) -> Result<()> {
        let current = self.get_index_state("version")?;
        if current.as_deref() != Some(schema::CURRENT_VERSION) {
            self.set_index_state("version", schema::CURRENT_VERSION)?;
        }
        Ok(())
    }

    /// Insert chunks into the `symbols` table inside a single transaction (batch — §11.1), so a
    /// failure rolls the whole batch back and bulk inserts pay one commit.
    pub fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        insert_chunks_on(&tx, chunks)?;
        tx.commit()?;
        Ok(())
    }

    /// Delete all `symbols` rows for `file_path` (incremental update support).
    pub fn delete_chunks_for_file(&self, file_path: &Path) -> Result<()> {
        let conn = self.lock()?;
        delete_chunks_for_file_on(&conn, file_path)?;
        Ok(())
    }

    /// Delete a file's `files_metadata` row (deletion reconciliation, §5.2). Symmetric with
    /// [`Storage::delete_chunks_for_file`]; deleting an unknown file is a no-op (0 rows affected).
    pub fn delete_file_meta(&self, file_path: &Path) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(queries::DELETE_FILE_META, params![path_to_str(file_path)])?;
        Ok(())
    }

    /// Enumerate every file path currently recorded in `files_metadata`. Drives deletion
    /// reconciliation: paths returned here but no longer on disk are stale and must be cleaned up.
    pub fn all_indexed_files(&self) -> Result<Vec<std::path::PathBuf>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare_cached(queries::ALL_INDEXED_FILES)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(std::path::PathBuf::from(row?));
        }
        Ok(out)
    }

    /// Full-text BM25 search with the built-in default per-column weights (10,1,1,5,2,2,2). Returns
    /// at most `limit` hits, best-first. An empty database (or no match) yields an empty `Vec`, not
    /// an error. Delegates to [`Storage::search_with_weights`] with `None`, so this default path is
    /// byte-identical to a caller passing `None` or `Some(&[10.,1.,1.,5.,2.,2.,2.])`.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search_with_weights(query, limit, None)
    }

    /// Full-text BM25 search with caller-supplied per-column weights (R2.2a / D24). `weights` is one
    /// `f64` per indexed FTS5 column, in `schema::CREATE_SYMBOLS` order (`symbol_name`, `symbol_type`,
    /// `chunk_text`, `parent_symbol`, `imports`, `cross_references`, `file_docstring` — 7 total).
    ///
    /// `None` reuses the cached default [`queries::SEARCH`] verbatim, so the default path stays
    /// byte-identical and warm in the statement cache. `Some(w)` builds the same statement with the
    /// weights formatted into the `bm25(symbols, …)` ranking expression — FTS5's `bm25()` weights
    /// are auxiliary-function arguments that cannot be bound as `?` parameters, so they are rendered
    /// into the SQL text (injection-safe: each is a finite `f64`, never raw user input). The
    /// `MATCH ?1` / `LIMIT ?2` bindings stay parameterized exactly as the default. The weighted SQL
    /// is dynamic per weight vector, so it uses `prepare` (not the constant-keyed `prepare_cached`).
    ///
    /// FTS5 accepts zero and negative weights, so those pass through and rank normally. A non-finite
    /// weight (`NaN`/±`inf`) cannot be a SQL numeric literal and is rejected as
    /// [`StorageError::NonFiniteWeight`] rather than emitted into the query (the CLI validates this
    /// first; this is a defensive guard). Ordering invariant is unchanged: `bm25 ASC, rowid ASC`.
    pub fn search_with_weights(
        &self,
        query: &str,
        limit: usize,
        weights: Option<&[f64; 7]>,
    ) -> Result<Vec<SearchResult>> {
        let conn = self.lock()?;
        let mut out = Vec::new();
        match weights {
            None => {
                let mut stmt = conn.prepare_cached(queries::SEARCH)?;
                let rows = stmt.query_map(params![query, limit as i64], map_search_row)?;
                for row in rows {
                    out.push(row??);
                }
            }
            Some(w) => {
                for &weight in w {
                    if !weight.is_finite() {
                        return Err(StorageError::NonFiniteWeight(weight));
                    }
                }
                let sql = queries::search_with_weights_sql(w);
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![query, limit as i64], map_search_row)?;
                for row in rows {
                    out.push(row??);
                }
            }
        }
        Ok(out)
    }

    /// Path-scoped symbol skeleton for the `codecache_outline` tool (Decision Log **D19**).
    ///
    /// Returns every [`SymbolOutline`] whose `file_path` is EXACTLY `path` or lives under it as a
    /// directory prefix (`<path>/%`), ordered deterministically by `(file_path, start_line,
    /// end_line)`. A plain column `SELECT` over the contentful `symbols` table reads the stored
    /// UNINDEXED line columns with no re-parse and no file I/O (D7). An unknown path (matching no
    /// stored file, exact or prefix) yields an empty `Vec`, never an error. SQL `LIKE` wildcards
    /// (`%`/`_`) in the path are escaped so a sibling sharing the prefix string never over-matches.
    pub fn symbols_for_path(&self, path: &Path) -> Result<Vec<SymbolOutline>> {
        let exact = path_to_str(path);
        let prefix = format!("{}/%", escape_like(&exact));

        let conn = self.lock()?;
        let mut stmt = conn.prepare_cached(queries::SYMBOLS_FOR_PATH)?;
        let rows = stmt.query_map(params![exact, prefix], map_outline_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    /// Read a file's stored content hash, or `None` if the file is unknown.
    pub fn get_file_hash(&self, file_path: &Path) -> Result<Option<String>> {
        let conn = self.lock()?;
        let hash = conn
            .query_row(
                queries::GET_FILE_HASH,
                params![path_to_str(file_path)],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(hash)
    }

    /// Read a file's full [`FileMeta`], or `None` if the file is unknown.
    pub fn get_file_meta(&self, file_path: &Path) -> Result<Option<FileMeta>> {
        let conn = self.lock()?;
        let meta = conn
            .query_row(
                queries::GET_FILE_META,
                params![path_to_str(file_path)],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        match meta {
            None => Ok(None),
            Some((content_hash, mtime, file_size, lang_str, chunk_count)) => {
                let language = Language::from_str_lenient(&lang_str).ok_or_else(|| {
                    StorageError::CorruptRow(format!("unknown language '{lang_str}'"))
                })?;
                Ok(Some(FileMeta {
                    content_hash,
                    mtime: mtime as u64,
                    file_size: file_size as u64,
                    language,
                    chunk_count: chunk_count as usize,
                }))
            }
        }
    }

    /// Upsert a file's metadata row (D6).
    pub fn update_file_hash(&self, file_path: &Path, meta: &FileMeta) -> Result<()> {
        let conn = self.lock()?;
        update_file_hash_on(&conn, file_path, meta)?;
        Ok(())
    }

    /// Run `each` over `items` inside ONE outer transaction, isolating every item in its own
    /// SAVEPOINT so a single item's error rolls back only that item — never the survivors already
    /// written in the same outer transaction (Decision Log **D20**, plan §3.2.2). This amortizes the
    /// per-item commit/fsync that the autocommit [`Storage::insert_chunks`]/
    /// [`Storage::update_file_hash`] pay (the indexer batches a whole run's per-file writes here),
    /// while preserving D2 per-file error isolation.
    ///
    /// For each `item`, in order: open a savepoint, run `each(&writer, item)` — whose writes through
    /// `writer` participate in that savepoint —
    /// * `Ok(())` ⇒ RELEASE the savepoint (the item's writes persist within the outer tx);
    /// * `Err(e)` ⇒ ROLLBACK TO the savepoint (discard the item's partial writes), record `Err(e)`,
    ///   and CONTINUE to the next item.
    ///
    /// After all items, COMMIT the outer transaction once — committing every survivor atomically.
    /// Returns `Ok(per_item)` with one inner [`Result`] per input item (same order, same length).
    /// The OUTER `Result` is `Err` only for a non-isolatable failure: the outer begin/commit, a
    /// savepoint begin/release/rollback, or a poisoned lock ([`StorageError::LockPoisoned`]).
    ///
    /// `BatchWriter` borrows the open transaction over the SAME shared connection (**D8**); `each`
    /// must NOT re-lock `Storage` (that would deadlock the single `Mutex<Connection>`).
    pub fn write_in_transaction<T, F>(&self, items: &[T], mut each: F) -> Result<Vec<Result<()>>>
    where
        F: FnMut(&BatchWriter<'_>, &T) -> Result<()>,
    {
        let mut conn = self.lock()?;
        let mut tx = conn.transaction()?;
        let mut per_item = Vec::with_capacity(items.len());
        for item in items {
            let mut sp = tx.savepoint()?;
            let outcome = {
                let writer = BatchWriter { conn: &sp };
                each(&writer, item)
            };
            match outcome {
                Ok(()) => {
                    // RELEASE: fold the item's writes into the outer transaction.
                    sp.commit()?;
                    per_item.push(Ok(()));
                }
                Err(e) => {
                    // ROLLBACK TO: discard only this item's partial writes, then RELEASE the now-empty
                    // savepoint so the marker does not linger. A failure here is non-isolatable.
                    sp.rollback()?;
                    sp.commit()?;
                    per_item.push(Err(e));
                }
            }
        }
        tx.commit()?;
        Ok(per_item)
    }

    /// Read one `index_state` value by key.
    pub fn get_index_state(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock()?;
        let value = conn
            .query_row(queries::GET_INDEX_STATE, params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(value)
    }

    /// Upsert one `index_state` key/value pair.
    pub fn set_index_state(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(queries::SET_INDEX_STATE, params![key, value])?;
        Ok(())
    }
}

/// The per-item write surface lent by [`Storage::write_in_transaction`] (Decision Log **D20**).
///
/// Each method runs against the CURRENT savepoint by borrowing the open transaction's connection
/// (a [`Savepoint`](rusqlite::Savepoint) derefs to [`Connection`]). It therefore shares the single
/// `Arc<Mutex<Connection>>` (**D8**) without re-locking it — calling these inside the
/// `write_in_transaction` closure does not deadlock. The methods mirror the autocommit
/// [`Storage`] methods of the same name, but their writes participate in the active savepoint
/// rather than committing on their own.
pub struct BatchWriter<'a> {
    conn: &'a Connection,
}

impl BatchWriter<'_> {
    /// Insert `chunks` into `symbols`, scoped to the current savepoint (transactional sibling of
    /// [`Storage::insert_chunks`]).
    pub fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()> {
        insert_chunks_on(self.conn, chunks)
    }

    /// Delete every `symbols` row for `file_path`, scoped to the current savepoint (transactional
    /// sibling of [`Storage::delete_chunks_for_file`]).
    pub fn delete_chunks_for_file(&self, file_path: &Path) -> Result<()> {
        delete_chunks_for_file_on(self.conn, file_path)
    }

    /// Upsert `file_path`'s `files_metadata` row, scoped to the current savepoint (transactional
    /// sibling of [`Storage::update_file_hash`]).
    pub fn update_file_hash(&self, file_path: &Path, meta: &FileMeta) -> Result<()> {
        update_file_hash_on(self.conn, file_path, meta)
    }
}

/// Insert `chunks` into the `symbols` table over `conn` (no transaction control of its own — the
/// caller owns the transaction/savepoint). Shared by the autocommit [`Storage::insert_chunks`] and
/// the transactional [`BatchWriter::insert_chunks`] so both write the exact same rows.
fn insert_chunks_on(conn: &Connection, chunks: &[Chunk]) -> Result<()> {
    let mut stmt = conn.prepare_cached(queries::INSERT_CHUNK)?;
    for c in chunks {
        stmt.execute(params![
            c.symbol_name,
            c.symbol_type.as_str(),
            c.chunk_text,
            c.parent_symbol,
            c.imports.join("\n"),
            c.cross_references.join("\n"),
            c.file_docstring,
            path_to_str(&c.file_path),
            c.start_byte as i64,
            c.end_byte as i64,
            c.start_line as i64,
            c.end_line as i64,
            c.language.as_str(),
        ])?;
    }
    Ok(())
}

/// Delete every `symbols` row for `file_path` over `conn`. Shared by the autocommit and
/// transactional `delete_chunks_for_file` paths.
fn delete_chunks_for_file_on(conn: &Connection, file_path: &Path) -> Result<()> {
    conn.execute(
        queries::DELETE_CHUNKS_FOR_FILE,
        params![path_to_str(file_path)],
    )?;
    Ok(())
}

/// Upsert `file_path`'s `files_metadata` row over `conn` (D6). Shared by the autocommit and
/// transactional `update_file_hash` paths.
fn update_file_hash_on(conn: &Connection, file_path: &Path, meta: &FileMeta) -> Result<()> {
    conn.execute(
        queries::UPSERT_FILE_META,
        params![
            path_to_str(file_path),
            meta.content_hash,
            meta.mtime as i64,
            meta.file_size as i64,
            meta.language.as_str(),
            meta.chunk_count as i64,
        ],
    )?;
    Ok(())
}

/// Lossy-free conversion of a path to the text we store. Uses `to_string_lossy`; on the target
/// platforms file paths originate from UTF-8 sources, and lossy conversion avoids a panic on the
/// rare non-UTF-8 path while keeping the round-trip stable for the UTF-8 paths we test.
fn path_to_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Escape the SQL `LIKE` metacharacters in a literal path so it can be embedded in a
/// `<path>/%` prefix pattern without over-matching. With `ESCAPE '\'`, the escape char `\` and
/// the wildcards `%` (any run) and `_` (any single char) each become a literal by prefixing `\`.
/// The trailing `/%` the caller appends stays an unescaped wildcard — only the path portion is
/// escaped (Decision Log D19). Order matters: escape `\` first so we don't double-escape the
/// backslashes we introduce for `%`/`_`.
fn escape_like(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Raw, untyped columns of a `SEARCH` result row, in `queries::SEARCH` order.
struct RawSearchRow {
    symbol_name: String,
    symbol_type_str: String,
    chunk_text: String,
    parent_symbol: Option<String>,
    imports_joined: String,
    cross_joined: String,
    file_docstring: Option<String>,
    file_path: String,
    start_byte: i64,
    end_byte: i64,
    start_line: i64,
    end_line: i64,
    language_str: String,
    bm25_score: f64,
}

/// Map a `SEARCH` result row back into a [`SearchResult`]. Column order matches `queries::SEARCH`.
/// The outer `rusqlite::Result` is the raw-column read; the inner [`Result`] defers typed-enum
/// validation so a corrupt stored value becomes a [`StorageError::CorruptRow`], not a panic.
fn map_search_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<SearchResult>> {
    let raw = RawSearchRow {
        symbol_name: row.get(0)?,
        symbol_type_str: row.get(1)?,
        chunk_text: row.get(2)?,
        parent_symbol: row.get(3)?,
        imports_joined: row.get(4)?,
        cross_joined: row.get(5)?,
        file_docstring: row.get(6)?,
        file_path: row.get(7)?,
        start_byte: row.get(8)?,
        end_byte: row.get(9)?,
        start_line: row.get(10)?,
        end_line: row.get(11)?,
        language_str: row.get(12)?,
        bm25_score: row.get(13)?,
    };
    Ok(build_search_result(raw))
}

/// Validate the typed enums and assemble a [`SearchResult`] from raw columns.
fn build_search_result(raw: RawSearchRow) -> Result<SearchResult> {
    let symbol_type = SymbolType::from_str_lenient(&raw.symbol_type_str).ok_or_else(|| {
        StorageError::CorruptRow(format!("unknown symbol_type '{}'", raw.symbol_type_str))
    })?;
    let language = Language::from_str_lenient(&raw.language_str).ok_or_else(|| {
        StorageError::CorruptRow(format!("unknown language '{}'", raw.language_str))
    })?;
    Ok(SearchResult {
        chunk: Chunk {
            symbol_name: raw.symbol_name,
            symbol_type,
            file_path: std::path::PathBuf::from(raw.file_path),
            start_byte: raw.start_byte as usize,
            end_byte: raw.end_byte as usize,
            start_line: raw.start_line as usize,
            end_line: raw.end_line as usize,
            chunk_text: raw.chunk_text,
            language,
            parent_symbol: raw.parent_symbol,
            file_docstring: raw.file_docstring,
            imports: split_joined(&raw.imports_joined),
            cross_references: split_joined(&raw.cross_joined),
            // M1 schema has no is_heuristic column; storage round-trips only AST chunks, so the
            // flag is reconstructed as false. Persisting it is a known M5/M7 follow-up (see
            // src/chunker/CLAUDE.md "Storage-persistence seam").
            is_heuristic: false,
        },
        bm25_score: raw.bm25_score,
    })
}

/// Map a `SYMBOLS_FOR_PATH` row into a [`SymbolOutline`] (D19). Column order matches
/// `queries::SYMBOLS_FOR_PATH`: `symbol_name, symbol_type, parent_symbol, file_path, start_line,
/// end_line`. As with `map_search_row`, the inner [`Result`] defers typed-enum validation so a
/// corrupt stored `symbol_type` becomes a [`StorageError::CorruptRow`], never a panic.
fn map_outline_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<SymbolOutline>> {
    let symbol_name: String = row.get(0)?;
    let symbol_type_str: String = row.get(1)?;
    let parent_symbol: Option<String> = row.get(2)?;
    let file_path: String = row.get(3)?;
    let start_line: i64 = row.get(4)?;
    let end_line: i64 = row.get(5)?;
    Ok(SymbolType::from_str_lenient(&symbol_type_str)
        .ok_or_else(|| StorageError::CorruptRow(format!("unknown symbol_type '{symbol_type_str}'")))
        .map(|symbol_type| SymbolOutline {
            symbol_name,
            symbol_type,
            parent_symbol,
            file_path: std::path::PathBuf::from(file_path),
            start_line: start_line as usize,
            end_line: end_line as usize,
        }))
}

/// Inverse of `Vec::join("\n")` used when storing list columns. An empty stored string yields an
/// empty vec (not a one-element vec containing "").
fn split_joined(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split('\n').map(str::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_like_escapes_wildcards_and_backslash() {
        // A plain path is unchanged.
        assert_eq!(escape_like("src/auth"), "src/auth");
        // `%` and `_` become literals (prefixed with the `\` escape char).
        assert_eq!(escape_like("a%b"), "a\\%b");
        assert_eq!(escape_like("a_b"), "a\\_b");
        // The escape char itself is escaped first, so it does not consume a following metachar.
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("a\\%b"), "a\\\\\\%b");
    }

    #[test]
    fn split_joined_handles_empty_and_multi() {
        assert_eq!(split_joined(""), Vec::<String>::new());
        assert_eq!(split_joined("a"), vec!["a".to_string()]);
        assert_eq!(
            split_joined("a\nb\nc"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }
}
