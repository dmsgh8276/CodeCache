//! Chunker: turn parser output into enriched [`Chunk`]s.
//!
//! API anchor: `project_plan.md` §3.2.1 / §4.3. Owner: `principal-engineering-lead` +
//! `rust-treesitter-specialist`. Scenarios: `docs/TEST_STRATEGY.md#chunker`.
//!
//! ## Two paths (Decision Log D2)
//! [`chunk`] inspects the parser's [`error_rate`](crate::parser::error_rate):
//! - **AST path** (`error_rate < HEURISTIC_FALLBACK_THRESHOLD`): reuse the M3 cursor walk to map
//!   each `function`/`class`/`method` definition to a [`Chunk`] (spans byte-exact, parent/child
//!   nesting per plan policy (a)), then enrich each chunk in a single pass over the tree with the
//!   D3 fields below. Every chunk has `is_heuristic = false`.
//! - **Heuristic path** (`error_rate >= HEURISTIC_FALLBACK_THRESHOLD`): the tree is too broken to
//!   trust, so fall back to a line heuristic (`def `/`class ` at column 0 for Python). Chunks are
//!   flat (pairwise disjoint) and flagged `is_heuristic = true`; enrichment is left empty.
//!
//! ## Enrichment (Decision Log D3, AST path)
//! - `parent_symbol`: the enclosing class/function (computed by the M3 walk; carried through).
//! - `file_docstring`: the module-level docstring — the first statement of the `module` node when
//!   it is an `expression_statement` wrapping a `string`. Same value on every chunk of the file.
//! - `imports`: every `import_statement` / `import_from_statement` node text, in file order.
//! - `cross_references`: identifier names called inside each chunk's byte span (best-effort,
//!   deduplicated, stable order) — i.e. the function name of each `call` expression.
//!
//! Never panics on malformed input: a malformed tree yields `Ok` (possibly empty); surviving
//! chunks always satisfy `start_byte < end_byte <= source.len()` and slice back to `chunk_text`.

use tree_sitter::{Node, Tree};

use crate::parser::{self, Parser, ParserError, HEURISTIC_FALLBACK_THRESHOLD};
use crate::types::{Chunk, Language, SymbolType};

/// Errors the chunker can surface. Implements [`std::error::Error`] with a real
/// [`source`](std::error::Error::source) so callers can introspect the underlying parser failure
/// without any reachable `unwrap`/`expect`/`panic!` on a library path.
#[derive(Debug)]
pub enum ChunkerError {
    /// The underlying parser could not interpret the tree for the requested language (e.g. an
    /// unsupported language). Wraps the [`ParserError`] so its `source` chain is preserved.
    Parser(ParserError),
}

impl std::fmt::Display for ChunkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkerError::Parser(_) => write!(f, "chunker could not extract chunks from the tree"),
        }
    }
}

impl std::error::Error for ChunkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ChunkerError::Parser(e) => Some(e),
        }
    }
}

impl From<ParserError> for ChunkerError {
    fn from(e: ParserError) -> Self {
        ChunkerError::Parser(e)
    }
}

/// Crate-local result alias for chunker operations.
type Result<T> = std::result::Result<T, ChunkerError>;

/// Turn parser output into enriched [`Chunk`]s for `source` in `lang`.
///
/// Routes to the AST path or the heuristic fallback based on the tree's
/// [`error_rate`](crate::parser::error_rate). Returns `Ok(vec![])` for an empty file and never
/// panics on malformed input (Decision Log D2). Chunks are deterministically ordered by
/// `start_byte`.
pub fn chunk(tree: &Tree, source: &str, lang: Language) -> Result<Vec<Chunk>> {
    if parser::error_rate(tree) >= HEURISTIC_FALLBACK_THRESHOLD {
        return Ok(heuristic_chunks(source, lang));
    }

    // AST path: reuse the M3 cursor walk for byte-exact spans + parent_symbol classification.
    let parser = Parser::new()?;
    let mut chunks = parser.extract_chunks(tree, source, lang)?;

    // Single pass over the tree for the file-wide enrichment, then attach per-chunk references.
    let root = tree.root_node();
    let file_docstring = module_docstring(root, source);
    let imports = collect_imports(root, source);

    // One walk collects every bare-identifier `call` (its span + callee name) in DFS order; each
    // chunk then takes the calls contained in its span. This is O(nodes + chunks·calls) instead of
    // the previous O(chunks × tree_nodes) per-chunk re-walk, while preserving the observable output
    // (callees deduped in first-seen DFS order within each chunk span).
    let calls = collect_calls(root, source);

    for c in &mut chunks {
        c.file_docstring = file_docstring.clone();
        c.imports = imports.clone();
        c.cross_references = call_names_in_span(&calls, c.start_byte, c.end_byte);
    }

    chunks.sort_by_key(|c| c.start_byte);
    Ok(chunks)
}

// ───────────────────────────── enrichment (D3, AST path) ─────────────────────────────

/// The module-level docstring: the text of the first statement of the `module` root when it is an
/// `expression_statement` wrapping a `string`, with the string quotes stripped. `None` otherwise.
fn module_docstring(root: Node, source: &str) -> Option<String> {
    let mut walk = root.walk();
    let first = root
        .named_children(&mut walk)
        .find(|n| n.kind() != "comment")?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let mut inner_walk = first.walk();
    let string_node = first
        .named_children(&mut inner_walk)
        .find(|n| n.kind() == "string")?;
    let raw = node_text(string_node, source)?;
    Some(strip_string_literal(raw))
}

/// Strip Python string-literal quotes (triple or single, with an optional `r`/`b`/`f` prefix) and
/// return the inner text. Best-effort: if nothing recognizable wraps it, the input is returned.
fn strip_string_literal(raw: &str) -> String {
    // Drop a leading string prefix (e.g. r, b, f, rb) only when ASCII letters are immediately
    // followed by a quote; otherwise leave the input untouched (best-effort, never over-strips).
    let prefix_len = raw
        .find(['"', '\''])
        .filter(|&i| raw[..i].chars().all(|c| c.is_ascii_alphabetic()) && i <= 2)
        .unwrap_or(0);
    let after_prefix = &raw[prefix_len..];
    let quote = if after_prefix.starts_with("\"\"\"") || after_prefix.starts_with("'''") {
        3
    } else if after_prefix.starts_with('"') || after_prefix.starts_with('\'') {
        1
    } else {
        return after_prefix.to_string();
    };
    let bytes = after_prefix.as_bytes();
    if bytes.len() >= 2 * quote {
        after_prefix[quote..after_prefix.len() - quote].to_string()
    } else {
        after_prefix.to_string()
    }
}

/// Collect every top-level `import_statement` / `import_from_statement` node text, in file order.
fn collect_imports(root: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut walk = root.walk();
    for child in root.named_children(&mut walk) {
        if matches!(child.kind(), "import_statement" | "import_from_statement") {
            if let Some(text) = node_text(child, source) {
                imports.push(text.trim_end().to_string());
            }
        }
    }
    imports
}

/// A bare-identifier `call` site: the byte span of the `call` node plus its callee name. Collected
/// once per tree by [`collect_calls`] (DFS order) so cross-reference enrichment is a single walk.
struct CallSite {
    start: usize,
    end: usize,
    name: String,
}

/// Walk the tree **once** via a [`tree_sitter::TreeCursor`], collecting every `call` expression
/// with a bare `identifier` callee as a [`CallSite`] in DFS (document) order. Attribute calls like
/// `os.urandom(...)` are skipped (their callee is not a plain `identifier`).
fn collect_calls(root: Node, source: &str) -> Vec<CallSite> {
    let mut calls: Vec<CallSite> = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "call" {
            if let Some(name) = call_function_name(node, source) {
                calls.push(CallSite {
                    start: node.start_byte(),
                    end: node.end_byte(),
                    name: name.to_string(),
                });
            }
        }

        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return calls;
            }
        }
    }
}

/// Best-effort cross-references for the chunk span `[start, end)`: the callee names of the
/// pre-collected [`CallSite`]s contained in the span, deduplicated in first-seen (DFS) order. The
/// `calls` slice is already in DFS order, so this preserves the original observable ordering.
fn call_names_in_span(calls: &[CallSite], start: usize, end: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for call in calls {
        if call.start >= start && call.end <= end && !names.iter().any(|n| n == &call.name) {
            names.push(call.name.clone());
        }
    }
    names
}

/// The simple identifier a `call` invokes, when the callee is a plain `identifier` (e.g.
/// `hash_password(...)`). Attribute calls like `os.urandom(...)` return `None` (we keep references
/// to bare names simple and deterministic).
fn call_function_name<'a>(call: Node, source: &'a str) -> Option<&'a str> {
    let func = call.child_by_field_name("function")?;
    if func.kind() == "identifier" {
        node_text(func, source)
    } else {
        None
    }
}

// ───────────────────────────── heuristic fallback (D2) ─────────────────────────────

/// Line heuristic: split `source` into flat, non-overlapping chunks at each line that begins with
/// `def ` or `class ` at column 0 (Python). Each chunk runs from its header line up to (but not
/// including) the next header line — so siblings are pairwise disjoint and span the whole file.
/// Every emitted chunk is flagged `is_heuristic = true` with empty enrichment.
fn heuristic_chunks(source: &str, lang: Language) -> Vec<Chunk> {
    // Byte offsets of header lines, plus a terminal sentinel at the file end.
    let mut headers: Vec<(usize, &str, SymbolType)> = Vec::new();
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        if let Some((name, kind)) = header_symbol(line) {
            headers.push((offset, name, kind));
        }
        offset += line.len();
    }
    let file_len = source.len();

    let mut chunks = Vec::with_capacity(headers.len());
    for i in 0..headers.len() {
        let (start, name, symbol_type) = headers[i];
        let end = headers.get(i + 1).map(|h| h.0).unwrap_or(file_len);
        // Guard the invariant defensively; a real header always yields start < end <= file_len.
        let Some(text) = source.get(start..end) else {
            continue;
        };
        if start >= end {
            continue;
        }
        let (start_line, end_line) = line_range(source, start, end);
        chunks.push(Chunk {
            symbol_name: name.to_string(),
            symbol_type,
            file_path: std::path::PathBuf::new(),
            start_byte: start,
            end_byte: end,
            start_line,
            end_line,
            chunk_text: text.to_string(),
            language: lang,
            parent_symbol: None,
            file_docstring: None,
            imports: Vec::new(),
            cross_references: Vec::new(),
            is_heuristic: true,
        });
    }
    chunks
}

/// If `line` starts a Python `def `/`class ` definition at column 0, return its name and kind.
/// The name is the identifier between the keyword and the first `(` or `:` (whitespace-trimmed).
fn header_symbol(line: &str) -> Option<(&str, SymbolType)> {
    let (rest, symbol_type) = if let Some(r) = line.strip_prefix("def ") {
        (r, SymbolType::Function)
    } else if let Some(r) = line.strip_prefix("async def ") {
        (r, SymbolType::Function)
    } else if let Some(r) = line.strip_prefix("class ") {
        (r, SymbolType::Class)
    } else {
        return None;
    };
    let name = rest
        .trim_start()
        .split(|c: char| c == '(' || c == ':' || c.is_whitespace())
        .next()
        .unwrap_or("");
    if name.is_empty() {
        return None;
    }
    Some((name, symbol_type))
}

/// 1-based inclusive `(start_line, end_line)` for the byte span `[start, end)` in `source`.
fn line_range(source: &str, start: usize, end: usize) -> (usize, usize) {
    let start_line = 1 + source[..start].bytes().filter(|&b| b == b'\n').count();
    // The chunk's last content line: count newlines strictly before the final byte of the span.
    let last_content = end.saturating_sub(1).max(start);
    let end_line = 1 + source[..last_content]
        .bytes()
        .filter(|&b| b == b'\n')
        .count();
    (start_line, end_line)
}

// ───────────────────────────── shared helpers ─────────────────────────────

/// Slice `node`'s exact bytes out of `source` (byte-exact, UTF-8-boundary safe).
fn node_text<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    source.get(node.start_byte()..node.end_byte())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_string_literal_handles_triple_and_single_and_prefix() {
        assert_eq!(strip_string_literal("\"\"\"hi\"\"\""), "hi");
        assert_eq!(strip_string_literal("'one'"), "one");
        assert_eq!(strip_string_literal("r\"raw\""), "raw");
        // Nothing recognizable: returned unchanged (best-effort).
        assert_eq!(strip_string_literal("bare"), "bare");
    }

    #[test]
    fn header_symbol_detects_def_class_async_and_extracts_name() {
        assert_eq!(
            header_symbol("def alpha():\n"),
            Some(("alpha", SymbolType::Function))
        );
        assert_eq!(
            header_symbol("class Foo:\n"),
            Some(("Foo", SymbolType::Class))
        );
        assert_eq!(
            header_symbol("async def beta(x):\n"),
            Some(("beta", SymbolType::Function))
        );
        assert_eq!(header_symbol("    def indented():\n"), None);
        assert_eq!(header_symbol("x = 1\n"), None);
    }
}
