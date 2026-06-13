//! Integration tests for the `parser` module (M3 — Python first).
//!
//! TDD RED: written before `src/parser/mod.rs` is implemented. Scenarios from
//! `docs/plans/M3-parser-python.md` (slices M3.1–M3.3) + `docs/TEST_STRATEGY.md#parser`.
//!
//! The public API under test (`project_plan.md` §3.2.1, plus the M3-introduced ERROR-rate API):
//! ```ignore
//! pub struct Parser { /* ts_parser, language_configs */ }
//! impl Parser {
//!     pub fn new() -> Result<Self>;
//!     pub fn parse_file(&mut self, path: &Path, content: &str, lang: Language)
//!         -> Result<tree_sitter::Tree>;
//!     pub fn extract_chunks(&self, tree: &tree_sitter::Tree, source: &str, lang: Language)
//!         -> Result<Vec<Chunk>>;
//! }
//! pub fn error_rate(tree: &tree_sitter::Tree) -> f32;        // fraction of ERROR/MISSING nodes
//! pub fn should_fall_back(rate: f32) -> bool;                // rate >= HEURISTIC_FALLBACK_THRESHOLD
//! pub const HEURISTIC_FALLBACK_THRESHOLD: f32;               // ~0.20 (D2)
//! ```
//!
//! Span assertions deliberately compare `&source[start_byte..end_byte]` against the expected
//! symbol text (the strongest off-by-one / byte-vs-char guard). Fixtures live under
//! `tests/fixtures/python/` and are committed. Tests are deterministic and parallel-safe.

use std::path::{Path, PathBuf};

use codecache::parser::{error_rate, should_fall_back, Parser, HEURISTIC_FALLBACK_THRESHOLD};
use codecache::types::{Chunk, Language, SymbolType};

// ───────────────────────────── fixture helpers ─────────────────────────────

/// Absolute path to a committed Python fixture.
fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("python")
        .join(name)
}

/// Load a committed Python fixture's bytes as a UTF-8 string, preserving its exact newlines.
fn load_fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

/// Parse a fixture to a tree (M3.1 seam) and hand back the (source, tree) pair.
fn parse_fixture(parser: &mut Parser, name: &str) -> (String, tree_sitter::Tree) {
    let source = load_fixture(name);
    let path = fixture_path(name);
    let tree = parser
        .parse_file(&path, &source, Language::Python)
        .unwrap_or_else(|e| panic!("parse {name}: {e}"));
    (source, tree)
}

/// Parse + extract chunks from a fixture in one step.
fn chunks_of(name: &str) -> (String, Vec<Chunk>) {
    let mut parser = Parser::new().expect("Parser::new");
    let (source, tree) = parse_fixture(&mut parser, name);
    let chunks = parser
        .extract_chunks(&tree, &source, Language::Python)
        .unwrap_or_else(|e| panic!("extract_chunks {name}: {e}"));
    (source, chunks)
}

/// Find exactly one chunk with the given symbol name; panic with context otherwise.
fn one_named<'a>(chunks: &'a [Chunk], name: &str) -> &'a Chunk {
    let matches: Vec<&Chunk> = chunks.iter().filter(|c| c.symbol_name == name).collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one chunk named {name:?}, found {}: {:?}",
        matches.len(),
        chunks.iter().map(|c| &c.symbol_name).collect::<Vec<_>>()
    );
    matches[0]
}

/// The bedrock off-by-one guard: the chunk's byte span must slice the source to exactly its text.
fn assert_span_slices_to_text(source: &str, chunk: &Chunk) {
    assert!(
        chunk.start_byte <= chunk.end_byte && chunk.end_byte <= source.len(),
        "span [{}, {}) out of bounds for source of {} bytes ({:?})",
        chunk.start_byte,
        chunk.end_byte,
        source.len(),
        chunk.symbol_name
    );
    assert_eq!(
        &source[chunk.start_byte..chunk.end_byte],
        chunk.chunk_text,
        "chunk_text must equal source[start_byte..end_byte] for {:?}",
        chunk.symbol_name
    );
}

// ═══════════════════════ Slice M3.1 — parse to a tree ═══════════════════════

#[test]
fn parse_valid_python_expects_tree_without_errors() {
    let mut parser = Parser::new().expect("Parser::new");
    let (_source, tree) = parse_fixture(&mut parser, "valid_module.py");
    // A well-formed module parses to a tree whose root reports no syntax errors and an
    // (effectively) zero ERROR-node rate.
    assert!(
        !tree.root_node().has_error(),
        "valid Python must parse without ERROR nodes"
    );
    assert_eq!(
        error_rate(&tree),
        0.0,
        "valid Python must have a 0.0 ERROR-node rate"
    );
}

#[test]
fn parse_empty_file_expects_empty_tree_no_panic() {
    let mut parser = Parser::new().expect("Parser::new");
    // An empty file must parse without panicking and yield no extractable symbols.
    let tree = parser
        .parse_file(Path::new("empty.py"), "", Language::Python)
        .expect("empty file parses to a tree");
    assert!(
        !tree.root_node().has_error(),
        "empty file is not a syntax error"
    );
    let chunks = parser
        .extract_chunks(&tree, "", Language::Python)
        .expect("extract over empty tree");
    assert!(
        chunks.is_empty(),
        "an empty file yields no chunks, got {chunks:?}"
    );
}

#[test]
fn all_v01_languages_parse_supported() {
    // M9 EXIT GUARANTEE: v0.1 language coverage is Python / TypeScript / Go. M3 wired only Python
    // (and this test used to assert "Go is unsupported"); M9.1 wired TypeScript and M9.2 wired Go,
    // so all three `Language` variants now parse to a tree. This replaces the now-false
    // "Go unsupported" claim with the real coverage guarantee — every supported v0.1 language must
    // `parse_file` to `Ok`, not panic and not silently mis-parse. (The typed `UnsupportedLanguage`
    // error path is still covered as a focused unit test in `src/parser/mod.rs`.)
    let mut parser = Parser::new().expect("Parser::new");
    let cases = [
        (Path::new("m.py"), "def f():\n    pass\n", Language::Python),
        (Path::new("m.ts"), "function f() {}\n", Language::TypeScript),
        (
            Path::new("m.go"),
            "package main\nfunc F() {}\n",
            Language::Go,
        ),
    ];
    for (path, content, lang) in cases {
        let tree = parser
            .parse_file(path, content, lang)
            .unwrap_or_else(|e| panic!("supported language {lang:?} must parse to Ok, got {e}"));
        assert!(
            !tree.root_node().has_error(),
            "valid {lang:?} snippet must parse without ERROR nodes"
        );
    }
}

// ═════════════════ Slice M3.2 — exact byte spans + symbol typing ════════════

#[test]
fn extracts_top_level_function_with_exact_span() {
    let (source, chunks) = chunks_of("top_level_function.py");
    let f = one_named(&chunks, "greet");
    assert_eq!(f.symbol_type, SymbolType::Function);
    assert_eq!(f.language, Language::Python);
    // Whole `def greet(...): ...` body, exact bytes.
    assert_span_slices_to_text(&source, f);
    assert_eq!(
        f.chunk_text, "def greet(name):\n    return \"hi \" + name\n",
        "function span must cover the full definition"
    );
    // 1-based inclusive line range (D7).
    assert_eq!(f.start_line, 1, "function starts on line 1");
    assert_eq!(f.end_line, 2, "function ends on line 2");
}

#[test]
fn extracts_class_with_exact_span() {
    let (source, chunks) = chunks_of("simple_class.py");
    let c = one_named(&chunks, "Greeter");
    assert_eq!(c.symbol_type, SymbolType::Class);
    assert_span_slices_to_text(&source, c);
    // The class span covers the whole class block, from `class` through the last method line.
    assert!(
        c.chunk_text.starts_with("class Greeter:"),
        "class span must start at the `class` keyword, got {:?}",
        c.chunk_text
    );
    assert!(
        c.chunk_text.contains("def greet(self):"),
        "class span must include its method bodies, got {:?}",
        c.chunk_text
    );
    assert_eq!(c.start_line, 1, "class starts on line 1");
}

#[test]
fn extracts_method_inside_class_as_method_type() {
    let (source, chunks) = chunks_of("simple_class.py");
    // `greet` is defined inside `class Greeter` ⇒ Method, not Function (specialist decision).
    let m = one_named(&chunks, "greet");
    assert_eq!(
        m.symbol_type,
        SymbolType::Method,
        "a function defined in a class body must be typed as Method"
    );
    assert_eq!(
        m.parent_symbol.as_deref(),
        Some("Greeter"),
        "method's parent_symbol must be its enclosing class (D3)"
    );
    assert_span_slices_to_text(&source, m);
    // The `__init__` method is also a Method, distinct from any free function.
    let init = one_named(&chunks, "__init__");
    assert_eq!(init.symbol_type, SymbolType::Method);
    assert_eq!(init.parent_symbol.as_deref(), Some("Greeter"));
}

#[test]
fn nested_function_extracted_with_correct_parent_context() {
    let (source, chunks) = chunks_of("nested_function.py");
    // `outer` is a free function; `inner` is nested within it.
    let outer = one_named(&chunks, "outer");
    assert_eq!(outer.symbol_type, SymbolType::Function);
    assert_eq!(
        outer.parent_symbol, None,
        "a top-level function has no parent"
    );
    assert_span_slices_to_text(&source, outer);

    let inner = one_named(&chunks, "inner");
    assert_eq!(
        inner.symbol_type,
        SymbolType::Function,
        "a function nested in a function (not a class) stays a Function"
    );
    assert_eq!(
        inner.parent_symbol.as_deref(),
        Some("outer"),
        "nested function's parent_symbol must be its enclosing function (D3)"
    );
    assert_span_slices_to_text(&source, inner);
    assert_eq!(
        inner.chunk_text, "def inner(y):\n        return y + 1\n",
        "nested function span must cover exactly the inner def"
    );
}

#[test]
fn async_def_extracted() {
    let (source, chunks) = chunks_of("async_def.py");
    let f = one_named(&chunks, "fetch");
    assert_eq!(f.symbol_type, SymbolType::Function);
    assert_span_slices_to_text(&source, f);
    assert!(
        f.chunk_text.starts_with("async def fetch("),
        "async function span must include the `async` keyword, got {:?}",
        f.chunk_text
    );
}

#[test]
fn decorated_function_span_includes_decorator() {
    // Specialist decision (per plan): a decorated def's span INCLUDES the `@decorator` lines.
    let (source, chunks) = chunks_of("decorated_function.py");
    let f = one_named(&chunks, "compute");
    assert_eq!(f.symbol_type, SymbolType::Function);
    assert_span_slices_to_text(&source, f);
    assert!(
        f.chunk_text.starts_with("@cache\n@retry(3)\ndef compute("),
        "decorated function span must start at the first decorator line, got {:?}",
        f.chunk_text
    );
    assert_eq!(
        f.start_line, 1,
        "span starts at the first decorator (line 1), not the `def` line"
    );
}

#[test]
fn multibyte_identifier_span_is_byte_correct() {
    // The function name uses multibyte UTF-8 (Greek αβγ / τ). Byte offsets must land on UTF-8
    // boundaries and the slice must reproduce the exact text — the core byte-vs-char guard.
    let (source, chunks) = chunks_of("multibyte_identifier.py");
    let f = one_named(&chunks, "αβγ");
    assert_eq!(f.symbol_type, SymbolType::Function);
    // If start/end were char-counted rather than byte-counted, this slice would panic or mismatch.
    assert_span_slices_to_text(&source, f);
    assert_eq!(
        f.chunk_text, "def αβγ(τ):\n    return τ\n",
        "multibyte function span must reproduce the exact UTF-8 text"
    );
}

#[test]
fn crlf_file_spans_correct() {
    // The fixture uses CRLF line endings. Byte spans must account for the `\r\n` (2 bytes) so the
    // sliced text still equals chunk_text exactly.
    let (source, chunks) = chunks_of("crlf_function.py");
    assert!(
        source.contains("\r\n"),
        "crlf fixture must retain CRLF line endings on read"
    );
    let f = one_named(&chunks, "crlf_fn");
    assert_eq!(f.symbol_type, SymbolType::Function);
    assert_span_slices_to_text(&source, f);
    assert!(
        f.chunk_text.contains("\r\n"),
        "CRLF endings must be preserved within the chunk span, got {:?}",
        f.chunk_text
    );
}

// ═══════════ Slice M3.3 — ERROR-node detection + degradation (D2) ═══════════

#[test]
fn error_node_rate_computed_for_malformed_file() {
    // A file with a clearly broken construct must report a positive ERROR-node rate in [0, 1].
    let mut parser = Parser::new().expect("Parser::new");
    let (_source, tree) = parse_fixture(&mut parser, "malformed.py");
    let rate = error_rate(&tree);
    assert!(
        (0.0..=1.0).contains(&rate),
        "error_rate must be a fraction in [0, 1], got {rate}"
    );
    assert!(
        rate > 0.0,
        "a malformed file must report a positive ERROR-node rate, got {rate}"
    );
    // Sanity: the threshold constant is itself a sensible fraction (~20% per D2).
    assert!(
        (0.0..1.0).contains(&HEURISTIC_FALLBACK_THRESHOLD),
        "HEURISTIC_FALLBACK_THRESHOLD must be a fraction in [0, 1)"
    );
}

#[test]
fn high_error_file_above_threshold_flags_for_heuristic_fallback() {
    // A mostly-garbage file must exceed the threshold and flag for heuristic fallback (D2). The
    // parser only *reports* the flag here; M4's chunker owns the actual heuristic output.
    let mut parser = Parser::new().expect("Parser::new");
    let (_source, tree) = parse_fixture(&mut parser, "high_error.py");
    let rate = error_rate(&tree);
    assert!(
        rate >= HEURISTIC_FALLBACK_THRESHOLD,
        "high-error file rate {rate} must meet/exceed threshold {HEURISTIC_FALLBACK_THRESHOLD}"
    );
    assert!(
        should_fall_back(rate),
        "rate {rate} at/above threshold must request heuristic fallback"
    );
    // And the inverse holds at the bottom of the range: a clean file does not fall back.
    assert!(
        !should_fall_back(0.0),
        "a zero ERROR rate must never request fallback"
    );
}

#[test]
fn malformed_file_never_panics_returns_result() {
    // D2: malformed input must degrade gracefully — parse + extract return a Result and never
    // panic, even when the tree is riddled with ERROR/MISSING nodes.
    let mut parser = Parser::new().expect("Parser::new");
    let source = load_fixture("high_error.py");
    let path = fixture_path("high_error.py");

    let tree = parser
        .parse_file(&path, &source, Language::Python)
        .expect("malformed input must still parse to a (possibly error-laden) tree, not panic");

    // Extraction over a broken tree must also return Ok (possibly empty), never panic.
    let result = parser.extract_chunks(&tree, &source, Language::Python);
    assert!(
        result.is_ok(),
        "extract_chunks over a malformed tree must return Ok, got {result:?}"
    );
    // Whatever chunks survive must still satisfy the span invariant.
    for chunk in result.expect("ok") {
        assert_span_slices_to_text(&source, &chunk);
    }
}
