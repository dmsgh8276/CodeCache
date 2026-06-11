# CodeCache — Test Strategy

The scenario matrix the `principal-test-engineering-lead` works from. Tests come **first**;
this document is the source for "what scenarios must a slice cover" referenced by task briefs.

## Test levels
- **Unit** — in-module `#[cfg(test)] mod tests`: pure logic, fast, no I/O.
- **Integration** — `tests/<module>_tests.rs`: module seams against real SQLite/fixtures.
- **E2E** — `tests/e2e_*.rs`: full `init → index → query → update` via the public surface/binary.
- **Property** — `proptest`: invariants over generated inputs.
- **Bench** — `benches/` (criterion), owned by the performance engineer; budgets in `ROADMAP.md` M10.

## Conventions
- Isolate all filesystem/DB state with `tempfile`; never touch the real working tree.
- Fixtures live in `tests/fixtures/`, small and committed; documented in `tests/CLAUDE.md`.
- Name tests `behavior_under_condition_expects_result`. Deterministic & parallel-safe.
- Assert real values, not just `is_ok()`. Coverage target: ≥85% lines on core modules
  (`parser`, `chunker`, `storage`, `retriever`, `indexer`).

---

## Cross-cutting scenarios (apply to every slice that touches them)
- **Encoding/format**: UTF-8 incl. multibyte identifiers; CRLF vs LF; trailing newline / none.
- **Sizes**: empty file; single-symbol file; very large file; deeply nested symbols.
- **Malformed input**: files with `ERROR` nodes → graceful degradation (Decision Log #2), never panic.
- **Determinism**: same input ⇒ identical output and ordering (stable tie-breaks).
- **Idempotency**: repeating an operation on unchanged input is a no-op.
- **Errors surfaced**: missing/unreadable path, corrupt DB, unsupported language → typed errors, no panic.

---

## Per-module matrix

### config
- Valid TOML loads; defaults applied when fields omitted; unknown keys handled per policy.
- Invalid TOML / missing file → clear error. Ignore-pattern parsing correct.

### storage (SQLite + FTS5)
- Schema creation idempotent; migration on version bump.
- Insert/query/delete round-trip; bulk insert; delete-by-file.
- FTS5 `MATCH` returns expected rows; `bm25()` orders by relevance; column weighting respected.
- Corrupt/locked DB → error, not panic. Empty-DB query → empty result.

### hasher
- Deterministic xxHash3-128 for identical content; differs on 1-byte change.
- Change detection: unchanged file ⇒ "same"; modified ⇒ "changed". Binary & large files.

### parser (Python → TS → Go)
- Extracts functions/classes/methods with **exact** `start_byte`/`end_byte` (off-by-one guards).
- Nested functions, decorators, async, generics (TS), methods vs free functions, comments/docstrings.
- ERROR-node rate computed; high-error file routes to heuristic fallback.
- Per-language fixtures; unsupported language → error.

### chunker
- Property: chunks never overlap and always lie within `[0, file_len)`.
- Metadata enrichment populated: `parent_symbol`, `file_docstring`, `imports`, `cross_references`.
- Heuristic chunks flagged in metadata when degradation triggered.

### indexer
- Discovery honors `.gitignore` + extra ignore patterns; respects configured languages.
- Full index of a fixture repo populates storage correctly: chunks searchable, per-file
  `files_metadata` written (content_hash, file_size, language, chunk_count), and `index_state`
  totals (`total_files`/`total_chunks`) updated (§5.1 step 4); `IndexStats` counts + `duration_ms`.
- Malformed file in a full index does not abort the batch (**D2**): `index_all` returns `Ok`, the
  bad file is skipped/heuristically chunked, and sibling valid files are still indexed.
- Incremental: re-index unchanged ⇒ no writes (idempotent); modify N files ⇒ exactly those re-indexed.
- `update_files(&[..])` re-indexes exactly the changed files in the list (hash-filtered); a modified
  file's new symbol becomes searchable while untouched files keep their hash/chunks.
- Re-index (reconcile mode) discovers a newly-added file: its symbol is searchable + `files_metadata`
  row written, without dropping pre-existing files.
- Deleted file ⇒ its chunks removed AND its `files_metadata` row cleared; `index_state` totals decrease.
- **E2E (`tests/e2e_index.rs`, M5.4)** — public library surface (`codecache::{init, index}`), no CLI:
  `init(root)` creates `.codecache/` + `config.toml` + the schema-initialized DB at the resolved
  `db_path`; `index(root)` populates a queryable DB with correct `IndexStats`; re-`init` is
  idempotent/non-clobbering; re-`index` after a file edit reflects the change (incremental reconcile).

### retriever
- BM25 ranking deterministic; relevant chunk ranks above irrelevant.
- `--max-tokens` budget never exceeded; greedy packing stops at budget; token count accurate.
- Empty query / no matches ⇒ empty, well-formed result. Dedup of overlapping snippets.

### formatter
- Golden outputs for TOON, JSON, plaintext; JSON is valid and round-trips; file:line pairs correct.

### cli
- Each command parses expected args/flags; `--help`/`--version`; bad args ⇒ helpful error + nonzero exit.
- E2E: `init → index → query` through the built binary on a fixture repo.

### mcp_server
- JSON-RPC handshake; tool registration list; `query` tool round-trip vs mock client; malformed
  request ⇒ proper JSON-RPC error.

---

## Definition of "good test coverage" for a slice
All cross-cutting scenarios that apply + the module-specific rows above, at the appropriate
level, all initially RED, with meaningful assertions. The manager checks this against the task
brief before GREEN begins.
