# src/parser/ — CLAUDE.md

**Module:** `parser` · **Owner:** `principal-engineering-lead` + `rust-treesitter-specialist`
· **Milestone:** M3 (Python), M9 (TypeScript + Go) · stub at M0.

## Purpose
Tree-sitter integration: load grammars, run `.scm` queries to extract function/class/method
nodes with **exact** byte spans; detect ERROR-node rate and route high-error files to heuristic
fallback (**Decision Log D2** — indexing never hard-fails on malformed input).

## API anchor
`docs/project_plan.md` §3.2.1 (`Parser`, `LanguageConfig`) + §5.3 (per-language queries).

## Tests / scenarios
`docs/TEST_STRATEGY.md#parser-python--ts--go` — exact spans on nested/async/decorated symbols;
ERROR-node rate computed; heuristic fallback exercised; unsupported language → error.

## Shipped API (M3 — Python)
```rust
pub struct Parser { /* ts_parser, language_configs */ }
impl Parser {
    pub fn new() -> Result<Self>;                  // wires Python; validates `.scm` vs grammar
    pub fn parse_file(&mut self, path: &Path, content: &str, lang: Language)
        -> Result<tree_sitter::Tree>;              // unsupported lang ⇒ ParserError::UnsupportedLanguage
    pub fn extract_chunks(&self, tree: &tree_sitter::Tree, source: &str, lang: Language)
        -> Result<Vec<Chunk>>;                      // deterministic, sorted by start_byte
}
pub fn error_rate(tree: &tree_sitter::Tree) -> f32;   // (ERROR+MISSING)/named-nodes, in [0,1]
pub fn should_fall_back(rate: f32) -> bool;           // rate >= HEURISTIC_FALLBACK_THRESHOLD
pub const HEURISTIC_FALLBACK_THRESHOLD: f32;          // 0.20 (D2)
pub enum ParserError { UnsupportedLanguage(Language), Language(..), Query(..), ParseFailed{path} }
// ParserError: std::error::Error with source() chaining the underlying tree-sitter error.
```
Files: `mod.rs` (the above), `python.rs` / `typescript.rs` / `go.rs` (each a `LanguageConfig` =
grammar + queries), `queries/python.scm` + `queries/typescript.scm` + `queries/go.scm` (§5.3
S-expression queries). All three v0.1 languages wired.

## Design notes
- **Extraction = `TreeCursor` walk, not `QueryCursor`.** The `.scm` queries are compiled and
  validated in `new` (a bad query is a construction error), but extraction walks the tree
  directly. This (a) gives ancestor access for the two pinned decisions and (b) avoids the
  external `streaming-iterator` crate that tree-sitter 0.24's `QueryCursor::matches` requires —
  keeping `Cargo.toml` lean. M4 can drive the queries for D3 enrichment.
- **Decorator inclusion:** spans come from the `decorated_definition` wrapper when present, so
  `@decorator` lines are inside the span and `start_line` is the first decorator line.
- **Method vs function:** nearest *definition* ancestor decides — `class_definition` ⇒ `Method`
  (parent = class); `function_definition` ⇒ nested `Function` (parent = enclosing fn).
- **Per-language extraction dispatch (M9):** the `TreeCursor` walk calls `recognize_definition`,
  which `match`-dispatches on `Language` to `recognize_python` / `recognize_typescript` /
  `recognize_go` and returns a `Definition { span_node, name, symbol_type, parent_override }` fed to
  the shared `build_chunk` / `extend_to_line_end` / `field_text` / `node_text` helpers. Adding a
  language = one recognizer + one config + one `.scm`; the span/line/D2 machinery is
  language-agnostic and reused unchanged.
- **`parent_override` (M9.2):** `parent_symbol` is normally the nearest *lexical* definition
  ancestor (threaded down the walk). Go method receivers are an exception — the parent type is not a
  lexical ancestor — so `Definition.parent_override` lets a recognizer set the parent explicitly;
  `collect_chunks` uses `def.parent_override.or(parent)`. Python/TS recognizers always set it `None`,
  so their behavior is byte-for-byte unchanged.
- **TypeScript (M9.1):** `function_declaration` (incl. `async`) ⇒ `Function`; a `variable_declarator`
  whose `value` is an `arrow_function` ⇒ `Function` named by the declarator identifier, **span = the
  declarator** (excludes the `const`/`let` keyword); `class_declaration` ⇒ `Class`;
  `method_definition` inside a `class_declaration` ⇒ `Method` (parent = class, via
  `ts_parent_is_class`). Grammar = `tree_sitter_typescript::LANGUAGE_TYPESCRIPT`. **`.ts` → the
  TypeScript grammar; `.tsx`/JSX is deferred** (not in `detect_language` today). Generics parse
  inside the declaration node so spans are unaffected. **Interfaces / type aliases are NOT emitted
  as chunks** in v0.1 (§5.3 lists no query for them) — they only must not panic. No `SymbolType`
  variant was added.
- **Go (M9.2):** `function_declaration` ⇒ `Function`; `method_declaration` ⇒ `Method` with
  `parent_symbol` = the **receiver type name** (`go_receiver_type_name` drills receiver →
  `parameter_declaration` → `type:`, descends through a `pointer_type` so `*Server` and `Server`
  both yield `Server`, ignoring the receiver var — set via `parent_override`); a `type_declaration`
  whose `type_spec`'s `type:` is a `struct_type` ⇒ `SymbolType::Struct` (span node = the
  `type_declaration`, so the span starts at the `type` keyword). Grammar = `tree_sitter_go::LANGUAGE`.
  Package clauses, import declarations, and **interfaces are NOT emitted** as chunks (§5.3 lists no
  Go interface query). No `SymbolType` variant added. Go closures are `func_literal` (no recognizer
  arm) so they never double-emit.
- **Span exactness:** byte spans satisfy `&source[start..end] == chunk_text`; the span is
  extended to include the single trailing line terminator (`\n` / `\r\n`, CRLF preserved) that
  closes the def's last line; multibyte identifiers stay on UTF-8 boundaries. `start_line`/
  `end_line` are 1-based inclusive (D7); the appended terminator does not advance `end_line`.
- **error_rate denominator** is *named* nodes (anonymous literal tokens dilute the signal); the
  numerator counts every ERROR/MISSING node. `valid == 0.0`, malformed `> 0.0`, clamped to [0,1].

## Degradation seam (D2)
The parser only **reports** `error_rate` + `should_fall_back`; it never panics on malformed input
(`parse_file`/`extract_chunks` return `Ok`, possibly empty). The actual heuristic/regex chunker
fallback and the `heuristic` chunk flag are **owned by M4** (chunker), enforced again at M5.

## Known follow-ups
- **TS destructuring declarator** (`const {a} = () => {}`): the arrow-fn recognizer names the chunk
  from the declarator's `name` field even when it is a destructuring *pattern* rather than an
  `identifier`. No fixture hits this and the span invariant still holds; an optional guard
  (`name.kind() == "identifier"`) would skip it. Tracked for a future hardening pass (non-blocking,
  out of M9.1 fixture scope; reviewer minor).

## Status
**M3: GREEN (2026-06-10).** Python. **M9.1: GREEN (2026-06-12).** TypeScript
(`function_declaration`/arrow/`class_declaration`/`method_definition`). **M9.2: GREEN (2026-06-12).**
Go (`function_declaration`/`method_declaration`+receiver/struct → `Struct`). All three v0.1
languages wired (§5.3). 14 Python + 7 TS + 5 Go integration tests + unit tests pass; **full suite
179 green**; all four gates clean on Rust 1.85.0. M9.3 validates the mixed-language pipeline through
the indexer.
