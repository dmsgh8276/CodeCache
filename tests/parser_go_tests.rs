//! Integration tests for the `parser` module — M9.2 (Go).
//!
//! TDD RED: written before `src/parser/go.rs` exists and before `Parser::new` wires the Go
//! grammar. Scenarios from `BRIEF-M9-typescript-go.md` (slice M9.2) +
//! `docs/TEST_STRATEGY.md#parser-python--ts--go` + `docs/project_plan.md` §5.3.
//!
//! These mirror `parser_tests.rs` / `parser_ts_tests.rs` exactly (same helpers, same
//! span-exactness discipline) but drive `Language::Go`. Until Go is wired they fail at
//! `parse_file(.., Go)` returning `ParserError::UnsupportedLanguage(Go)` — the correct RED state.
//!
//! The Python contract Go MUST match (non-negotiable, from M3):
//!  - byte-exact spans: `&source[start_byte..end_byte] == chunk_text`;
//!  - D7 1-based inclusive line numbers; the trailing terminator does not advance `end_line`;
//!  - a `method_declaration` (one with a receiver) ⇒ `SymbolType::Method` whose `parent_symbol`
//!    is the RECEIVER TYPE name — stripped of the pointer `*` and the receiver variable, e.g.
//!    `func (s *Server) Handle(...)` ⇒ parent `Server`;
//!  - a top-level `function_declaration` ⇒ `SymbolType::Function`, `parent_symbol = None`;
//!  - a `type_declaration`/`type_spec` whose `type` is a `struct_type` ⇒ `SymbolType::Struct`;
//!  - D2 graceful degradation: `error_rate` + `should_fall_back` apply unchanged, never panic;
//!  - deterministic order (chunks sorted by `start_byte`).
//!
//! §5.3 lists no query for Go interfaces, so interfaces are NOT emitted as chunks in v0.1.
//!
//! Span assertions compare `&source[start_byte..end_byte]` against the expected text (the
//! strongest off-by-one / byte-vs-char guard). Fixtures live under `tests/fixtures/go/` and are
//! committed (LF newlines). Tests are deterministic and parallel-safe.

use std::path::{Path, PathBuf};

use codecache::parser::{error_rate, should_fall_back, Parser, HEURISTIC_FALLBACK_THRESHOLD};
use codecache::types::{Chunk, Language, SymbolType};

// ───────────────────────────── fixture helpers ─────────────────────────────

/// Absolute path to a committed Go fixture.
fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("go")
        .join(name)
}

/// Load a committed Go fixture's bytes as a UTF-8 string, preserving its exact newlines.
fn load_fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

/// Parse a fixture to a tree (the M3.1 seam) and hand back the (source, tree) pair.
fn parse_fixture(parser: &mut Parser, name: &str) -> (String, tree_sitter::Tree) {
    let source = load_fixture(name);
    let path = fixture_path(name);
    let tree = parser
        .parse_file(&path, &source, Language::Go)
        .unwrap_or_else(|e| panic!("parse {name}: {e}"));
    (source, tree)
}

/// Parse + extract chunks from a fixture in one step.
fn chunks_of(name: &str) -> (String, Vec<Chunk>) {
    let mut parser = Parser::new().expect("Parser::new");
    let (source, tree) = parse_fixture(&mut parser, name);
    let chunks = parser
        .extract_chunks(&tree, &source, Language::Go)
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

// ════════════════ M9.2 — exact byte spans + symbol typing (Go) ═══════════════

#[test]
fn extracts_function_declaration_with_exact_span() {
    let (source, chunks) = chunks_of("top_level_function.go");
    let f = one_named(&chunks, "Foo");
    assert_eq!(f.symbol_type, SymbolType::Function);
    assert_eq!(f.language, Language::Go);
    // Whole `func Foo(...) string { ... }` body, exact bytes (incl. the trailing line terminator).
    assert_span_slices_to_text(&source, f);
    assert_eq!(
        f.chunk_text, "func Foo(name string) string {\n\treturn \"hi \" + name\n}\n",
        "function span must cover the full declaration"
    );
    // 1-based inclusive line range (D7); the appended `\n` does not advance `end_line`.
    assert_eq!(f.start_line, 3, "function `Foo` starts on line 3");
    assert_eq!(
        f.end_line, 5,
        "function `Foo` ends on line 5 (closing brace)"
    );
    assert_eq!(
        f.parent_symbol, None,
        "a top-level function has no parent symbol"
    );
}

#[test]
fn extracts_method_declaration_with_receiver() {
    // §5.3: a `method_declaration` (i.e. a `func` with a receiver `(s *Server)`) ⇒
    // `SymbolType::Method`. Its `parent_symbol` is the RECEIVER TYPE name — `Server` — with the
    // pointer `*` and the receiver variable `s` stripped off. The span is the whole method
    // declaration (the `func` keyword through the closing brace + its trailing terminator).
    let (source, chunks) = chunks_of("method_with_receiver.go");
    let m = one_named(&chunks, "Handle");
    assert_eq!(
        m.symbol_type,
        SymbolType::Method,
        "a method_declaration (one with a receiver) must be typed as Method"
    );
    assert_eq!(m.language, Language::Go);
    assert_eq!(
        m.parent_symbol.as_deref(),
        Some("Server"),
        "method's parent_symbol must be the receiver TYPE name `Server`, \
         stripped of the pointer `*` and the receiver variable `s` (D3)"
    );
    assert_span_slices_to_text(&source, m);
    assert_eq!(
        m.chunk_text, "func (s *Server) Handle(path string) string {\n\treturn s.addr + path\n}\n",
        "method span must cover the full declaration including the receiver"
    );
    // The method declaration starts on line 7 and its body closes on line 9 (D7).
    assert_eq!(m.start_line, 7, "method `Handle` starts on line 7");
    assert_eq!(m.end_line, 9, "method `Handle` ends on line 9");
}

#[test]
fn extracts_struct_type_as_struct_symbol() {
    // §5.3: `type_declaration` → `type_spec` (name: type_identifier, type: struct_type) ⇒
    // `SymbolType::Struct`. The span node is the `type_declaration`, so it starts at the `type`
    // keyword and ends at the struct's closing brace + its trailing terminator.
    let (source, chunks) = chunks_of("struct_type.go");
    let s = one_named(&chunks, "Point");
    assert_eq!(
        s.symbol_type,
        SymbolType::Struct,
        "a `type X struct {{...}}` must be typed as Struct"
    );
    assert_eq!(s.language, Language::Go);
    assert_span_slices_to_text(&source, s);
    assert_eq!(
        s.chunk_text, "type Point struct {\n\tX int\n\tY int\n}\n",
        "struct span must cover the whole type declaration"
    );
    assert_eq!(s.start_line, 3, "struct `Point` starts on line 3");
    assert_eq!(
        s.end_line, 6,
        "struct `Point` ends on line 6 (closing brace)"
    );
    assert_eq!(
        s.parent_symbol, None,
        "a top-level struct has no parent symbol"
    );
}

#[test]
fn package_and_imports_handled() {
    // A file with a `package main` clause and an `import (...)` block plus one real function must
    // extract ONLY the function — the package clause and the import declarations must not produce
    // spurious chunks. Parse + extract must return Ok and never panic.
    let (source, chunks) = chunks_of("package_and_imports.go");

    // The real function IS extracted...
    let f = one_named(&chunks, "Run");
    assert_eq!(f.symbol_type, SymbolType::Function);
    assert_eq!(f.language, Language::Go);
    assert_span_slices_to_text(&source, f);
    assert_eq!(
        f.chunk_text,
        "func Run(name string) string {\n\treturn fmt.Sprint(strings.ToUpper(name))\n}\n",
        "function span must cover the full declaration"
    );
    assert_eq!(f.start_line, 8, "function `Run` starts on line 8");
    assert_eq!(f.end_line, 10, "function `Run` ends on line 10");

    // ...and it is the ONLY chunk: no chunk for the package clause or the imports.
    assert_eq!(
        chunks.len(),
        1,
        "only the function should be extracted; package/import decls must not produce chunks, got {:?}",
        chunks.iter().map(|c| &c.symbol_name).collect::<Vec<_>>()
    );
    assert!(
        !chunks.iter().any(|c| c.symbol_name == "main"),
        "the `package main` clause must not be emitted as a chunk, got {:?}",
        chunks.iter().map(|c| &c.symbol_name).collect::<Vec<_>>()
    );
    for chunk in &chunks {
        assert_span_slices_to_text(&source, chunk);
    }
}

#[test]
fn high_error_rate_go_file_flags_heuristic() {
    // D2 parity: a mostly-garbage Go file must exceed the threshold and flag for heuristic
    // fallback, and parse/extract must still return Ok without panic.
    let mut parser = Parser::new().expect("Parser::new");
    let source = load_fixture("high_error.go");
    let path = fixture_path("high_error.go");

    let tree = parser
        .parse_file(&path, &source, Language::Go)
        .expect("malformed Go must still parse to a (possibly error-laden) tree, not panic");

    let rate = error_rate(&tree);
    assert!(
        (0.0..=1.0).contains(&rate),
        "error_rate must be a fraction in [0, 1], got {rate}"
    );
    assert!(
        rate >= HEURISTIC_FALLBACK_THRESHOLD,
        "high-error Go rate {rate} must meet/exceed threshold {HEURISTIC_FALLBACK_THRESHOLD}"
    );
    assert!(
        should_fall_back(rate),
        "rate {rate} at/above threshold must request heuristic fallback (D2 parity)"
    );

    // Extraction over the broken tree must also return Ok and never panic; survivors keep spans.
    let result = parser.extract_chunks(&tree, &source, Language::Go);
    assert!(
        result.is_ok(),
        "extract_chunks over a malformed Go tree must return Ok, got {result:?}"
    );
    for chunk in result.expect("ok") {
        assert_span_slices_to_text(&source, &chunk);
    }
}
