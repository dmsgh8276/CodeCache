//! Parser: Tree-sitter integration — load grammars, run `.scm` queries, extract AST nodes with
//! exact byte spans; ERROR-node detection (graceful degradation, Decision Log D2).
//!
//! API anchor: `project_plan.md` §3.2.1 / §5.3. Owner: `principal-engineering-lead` +
//! `rust-treesitter-specialist`. Scenarios: `docs/TEST_STRATEGY.md#parser-python--ts--go`.
//! M3 ships Python; TS/Go land at M9.
//!
//! ## Extraction model (M3)
//! [`Parser::parse_file`] returns a raw `tree_sitter::Tree`; [`Parser::extract_chunks`] walks it
//! with a `TreeCursor` and emits one [`Chunk`] per function/class/method with a **byte-exact**
//! span (`&source[start_byte..end_byte] == chunk_text`, UTF-8-boundary correct, CRLF preserved).
//!
//! Two pinned specialist decisions (see `BRIEF-M3-parser-python.md`):
//! - **Decorator inclusion:** when a `function_definition`/`class_definition` is wrapped in a
//!   `decorated_definition`, the chunk span is taken from the *wrapper* so the `@decorator` lines
//!   are inside the span.
//! - **Method vs function:** a `function_definition` whose nearest *definition* ancestor is a
//!   `class_definition` is a [`SymbolType::Method`] with `parent_symbol = <class name>`; a
//!   function nested in another function stays a [`SymbolType::Function`] with
//!   `parent_symbol = <enclosing fn name>`.
//!
//! ## Degradation seam (D2)
//! [`error_rate`] reports the `(ERROR + MISSING) / named-node` fraction and [`should_fall_back`]
//! compares it to [`HEURISTIC_FALLBACK_THRESHOLD`]. M3 only *reports*; the actual heuristic/regex
//! chunker fallback (and the `heuristic` chunk flag) is owned by the M4 chunker. `parse_file` and
//! `extract_chunks` never panic on malformed input — they return `Ok` (possibly empty).

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Query, Tree};

use crate::types::{Chunk, Language, SymbolType};

mod go;
mod python;
mod typescript;

/// [`error_rate`] value at or above which a file should be routed to the M4 heuristic chunker
/// instead of trusting the AST (Decision Log D2). ~20% of named nodes broken.
pub const HEURISTIC_FALLBACK_THRESHOLD: f32 = 0.20;

/// Per-language Tree-sitter configuration: the compiled grammar plus the `.scm` extraction
/// queries (`project_plan.md` §3.2.1 / §5.3).
pub struct LanguageConfig {
    /// The Tree-sitter grammar for this language.
    grammar: tree_sitter::Language,
    /// The `.scm` extraction queries (function/class/method), validated in [`Parser::new`].
    queries: &'static str,
}

/// Errors the parser can surface. Implements [`std::error::Error`] with a real [`Error::source`]
/// so callers can introspect the underlying Tree-sitter failure without us reaching for
/// `unwrap`/`expect`/`panic!` on any library path.
#[derive(Debug)]
pub enum ParserError {
    /// The requested [`Language`] has no wired grammar yet (e.g. Go/TS at M3).
    UnsupportedLanguage(Language),
    /// Applying a grammar to the Tree-sitter parser failed.
    Language(tree_sitter::LanguageError),
    /// One of the embedded `.scm` queries failed to compile against the grammar.
    Query(tree_sitter::QueryError),
    /// Tree-sitter returned no tree for the given input (it never does for in-memory UTF-8, but
    /// we model it rather than `unwrap` the `Option`).
    ParseFailed {
        /// The file whose parse produced no tree.
        path: PathBuf,
    },
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserError::UnsupportedLanguage(lang) => {
                write!(f, "unsupported language for parsing: {}", lang.as_str())
            }
            ParserError::Language(_) => write!(f, "failed to set Tree-sitter language"),
            ParserError::Query(_) => write!(f, "failed to compile Tree-sitter query"),
            ParserError::ParseFailed { path } => {
                write!(f, "Tree-sitter produced no tree for {}", path.display())
            }
        }
    }
}

impl std::error::Error for ParserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParserError::Language(e) => Some(e),
            ParserError::Query(e) => Some(e),
            ParserError::UnsupportedLanguage(_) | ParserError::ParseFailed { .. } => None,
        }
    }
}

impl From<tree_sitter::LanguageError> for ParserError {
    fn from(e: tree_sitter::LanguageError) -> Self {
        ParserError::Language(e)
    }
}

impl From<tree_sitter::QueryError> for ParserError {
    fn from(e: tree_sitter::QueryError) -> Self {
        ParserError::Query(e)
    }
}

/// Crate-local result alias for parser operations.
type Result<T> = std::result::Result<T, ParserError>;

/// Tree-sitter front-end: holds one reusable `tree_sitter::Parser` and the per-language configs.
///
/// `parse_file` swaps the active grammar; `extract_chunks` is read-only over a produced tree, so
/// the borrow checker lets a caller hold a `&Tree` while extracting. See `project_plan.md` §3.2.1.
pub struct Parser {
    ts_parser: tree_sitter::Parser,
    language_configs: HashMap<Language, LanguageConfig>,
}

impl Parser {
    /// Build a parser with the Python grammar wired (M3). TS/Go are added at M9.
    ///
    /// The embedded `.scm` queries are compiled here so a malformed query is a construction-time
    /// error rather than a per-file surprise.
    pub fn new() -> Result<Self> {
        let mut language_configs = HashMap::new();

        let py = python::config();
        // Validate the queries against the grammar up front (proves capture/node names match).
        Query::new(&py.grammar, py.queries)?;
        language_configs.insert(Language::Python, py);

        let ts = typescript::config();
        Query::new(&ts.grammar, ts.queries)?;
        language_configs.insert(Language::TypeScript, ts);

        let go = go::config();
        Query::new(&go.grammar, go.queries)?;
        language_configs.insert(Language::Go, go);

        Ok(Self {
            ts_parser: tree_sitter::Parser::new(),
            language_configs,
        })
    }

    /// Parse `content` for `lang` into a Tree-sitter tree.
    ///
    /// Returns [`ParserError::UnsupportedLanguage`] for a language without a wired grammar.
    /// Malformed input still parses (to an error-laden tree) and never panics.
    pub fn parse_file(&mut self, path: &Path, content: &str, lang: Language) -> Result<Tree> {
        let config = self
            .language_configs
            .get(&lang)
            .ok_or(ParserError::UnsupportedLanguage(lang))?;
        self.ts_parser.set_language(&config.grammar)?;
        self.ts_parser
            .parse(content, None)
            .ok_or_else(|| ParserError::ParseFailed {
                path: path.to_path_buf(),
            })
    }

    /// Extract function/class/method chunks from `tree` over its `source`, ordered by
    /// `start_byte` (deterministic). Returns `Ok(vec)` even for empty or malformed trees.
    pub fn extract_chunks(&self, tree: &Tree, source: &str, lang: Language) -> Result<Vec<Chunk>> {
        // We only know how to interpret a tree for a language we can parse.
        if !self.language_configs.contains_key(&lang) {
            return Err(ParserError::UnsupportedLanguage(lang));
        }

        let file_path = PathBuf::new();
        let mut chunks = Vec::new();
        let root = tree.root_node();
        // Recurse from the root; `parent_symbol` is threaded down the definition stack.
        collect_chunks(root, source, lang, &file_path, None, &mut chunks);

        chunks.sort_by_key(|c| c.start_byte);
        Ok(chunks)
    }
}

/// Recursively walk `node`, emitting a [`Chunk`] for each definition the active `lang` recognizes,
/// dispatching per language to the right node kinds and span/parent rules.
///
/// `parent` is the name of the nearest enclosing *definition* (class/struct or function), used both
/// for `parent_symbol` and to decide Method vs Function. The shared helpers (`build_chunk`,
/// `extend_to_line_end`, `field_text`, `node_text`) are reused across languages; only the node-kind
/// recognition and the span/parent decisions vary, so each language gets a small `recognize` step.
fn collect_chunks(
    node: Node,
    source: &str,
    lang: Language,
    file_path: &Path,
    parent: Option<&str>,
    out: &mut Vec<Chunk>,
) {
    let mut walk = node.walk();
    let children: Vec<Node> = node.children(&mut walk).collect();
    for child in children {
        // A language-specific recognizer decides whether this child is a definition we emit, and
        // if so, which node to span, what symbol type it is, and the name to carry to its children.
        match recognize_definition(child, lang, source) {
            Some(def) => {
                // A recognizer may override the parent symbol (Go methods → receiver type name);
                // otherwise the chunk's parent is the nearest enclosing definition (threaded down).
                let chunk_parent = def.parent_override.or(parent);
                if let Some(chunk) = build_chunk(
                    def.span_node,
                    def.name,
                    def.symbol_type,
                    lang,
                    file_path,
                    chunk_parent,
                    source,
                ) {
                    out.push(chunk);
                }
                // Recurse into the def; its name becomes the children's enclosing parent.
                collect_chunks(child, source, lang, file_path, Some(def.name), out);
            }
            // Not a definition (module, wrapper, block, statement, ERROR …): recurse unchanged so
            // nested defs are still found.
            None => collect_chunks(child, source, lang, file_path, parent, out),
        }
    }
}

/// A recognized definition: the node whose byte span the chunk uses, the symbol name, and its type.
///
/// `parent_override` lets a recognizer set the chunk's `parent_symbol` directly rather than using
/// the nearest enclosing definition threaded by the walk — needed for Go methods, whose parent is
/// the receiver TYPE name (`func (s *Server) Handle()` → `Server`), not a lexical ancestor.
struct Definition<'a> {
    span_node: Node<'a>,
    name: &'a str,
    symbol_type: SymbolType,
    parent_override: Option<&'a str>,
}

/// Decide whether `node` is a definition the given `lang` emits as a [`Chunk`]. Returns the span
/// node, name, and symbol type, or `None` for any other node kind.
fn recognize_definition<'a>(
    node: Node<'a>,
    lang: Language,
    source: &'a str,
) -> Option<Definition<'a>> {
    match lang {
        Language::Python => recognize_python(node, source),
        Language::TypeScript => recognize_typescript(node, source),
        Language::Go => recognize_go(node, source),
    }
}

/// Python: `function_definition` (Method when nearest def ancestor is a class) and
/// `class_definition`. Decorated defs are spanned from their `decorated_definition` wrapper so the
/// `@decorator` lines are inside the span.
fn recognize_python<'a>(node: Node<'a>, source: &'a str) -> Option<Definition<'a>> {
    match node.kind() {
        "function_definition" => {
            let name = field_text(node, "name", source)?;
            let symbol_type = if python_parent_is_class(node) {
                SymbolType::Method
            } else {
                SymbolType::Function
            };
            Some(Definition {
                span_node: python_span_node_for(node),
                name,
                symbol_type,
                parent_override: None,
            })
        }
        "class_definition" => Some(Definition {
            span_node: python_span_node_for(node),
            name: field_text(node, "name", source)?,
            symbol_type: SymbolType::Class,
            parent_override: None,
        }),
        _ => None,
    }
}

/// TypeScript (§5.3): `function_declaration` → Function; an arrow fn assigned to a
/// `variable_declarator` → Function named by the declarator identifier, spanned by the declarator
/// (so it excludes the `const ` keyword and the trailing `;`); `class_declaration` → Class;
/// `method_definition` inside a `class_declaration` → Method. Interfaces/type-aliases are not
/// emitted in v0.1. The span node is the node itself in all cases (TS has no decorator wrapper here).
fn recognize_typescript<'a>(node: Node<'a>, source: &'a str) -> Option<Definition<'a>> {
    match node.kind() {
        "function_declaration" => Some(Definition {
            span_node: node,
            name: field_text(node, "name", source)?,
            symbol_type: SymbolType::Function,
            parent_override: None,
        }),
        // Only declarators whose value is an arrow function are emitted (named by the identifier).
        "variable_declarator"
            if node
                .child_by_field_name("value")
                .is_some_and(|v| v.kind() == "arrow_function") =>
        {
            Some(Definition {
                span_node: node,
                name: field_text(node, "name", source)?,
                symbol_type: SymbolType::Function,
                parent_override: None,
            })
        }
        "class_declaration" => Some(Definition {
            span_node: node,
            name: field_text(node, "name", source)?,
            symbol_type: SymbolType::Class,
            parent_override: None,
        }),
        "method_definition" => Some(Definition {
            span_node: node,
            name: field_text(node, "name", source)?,
            symbol_type: if ts_parent_is_class(node) {
                SymbolType::Method
            } else {
                SymbolType::Function
            },
            parent_override: None,
        }),
        _ => None,
    }
}

/// Go (§5.3): `function_declaration` → Function (top-level, `parent_symbol = None`);
/// `method_declaration` (the `func` form WITH a `receiver:`) → Method whose `parent_symbol` is the
/// receiver TYPE name (pointer `*` and receiver variable stripped, e.g. `(s *Server)` → `Server`);
/// `type_declaration` wrapping a `type_spec` whose `type` is a `struct_type` → Struct, spanned from
/// the `type_declaration` so the span starts at the `type` keyword. The package clause and import
/// declarations have no arm, so they are never emitted. Interfaces are out of scope in v0.1.
fn recognize_go<'a>(node: Node<'a>, source: &'a str) -> Option<Definition<'a>> {
    match node.kind() {
        "function_declaration" => Some(Definition {
            span_node: node,
            name: field_text(node, "name", source)?,
            symbol_type: SymbolType::Function,
            parent_override: None,
        }),
        "method_declaration" => Some(Definition {
            span_node: node,
            name: field_text(node, "name", source)?,
            symbol_type: SymbolType::Method,
            // The method's parent is the receiver TYPE name (`(s *Server)` → `Server`), not a
            // lexical ancestor. If the drill fails on a malformed tree, fall back to no parent.
            parent_override: go_receiver_type_name(node, source),
        }),
        // A `type X struct {...}` declaration: only emit when the `type_spec`'s `type` field is a
        // `struct_type`. The span node is the `type_declaration` (so it starts at the `type`
        // keyword), per §5.3 / the RED test's expected bytes.
        "type_declaration" => {
            let mut walk = node.walk();
            let type_spec = node.children(&mut walk).find(|c| c.kind() == "type_spec")?;
            let type_node = type_spec.child_by_field_name("type")?;
            if type_node.kind() != "struct_type" {
                return None;
            }
            Some(Definition {
                span_node: node,
                name: field_text(type_spec, "name", source)?,
                symbol_type: SymbolType::Struct,
                parent_override: None,
            })
        }
        _ => None,
    }
}

/// The receiver TYPE name of a Go `method_declaration`, used as the method's `parent_symbol`.
/// Drills `method_declaration` → `receiver:` parameter_list → first `parameter_declaration` →
/// `type:`; if that type is a `pointer_type`, descends to its inner `type_identifier`. Returns
/// `None` (no panic) if any step is missing, so the method still emits with `parent_symbol = None`.
fn go_receiver_type_name<'a>(method: Node, source: &'a str) -> Option<&'a str> {
    let receiver = method.child_by_field_name("receiver")?;
    let mut walk = receiver.walk();
    let decl = receiver
        .children(&mut walk)
        .find(|c| c.kind() == "parameter_declaration")?;
    let type_node = decl.child_by_field_name("type")?;
    let ident = if type_node.kind() == "pointer_type" {
        // `*Server`: descend to the inner `type_identifier`.
        let mut inner_walk = type_node.walk();
        let found = type_node
            .children(&mut inner_walk)
            .find(|c| c.kind() == "type_identifier");
        found?
    } else {
        type_node
    };
    node_text(ident, source)
}

/// The Python span node: the enclosing `decorated_definition` (so `@decorator` lines are included)
/// when present, otherwise the definition node itself.
fn python_span_node_for(def: Node) -> Node {
    match def.parent() {
        Some(p) if p.kind() == "decorated_definition" => p,
        _ => def,
    }
}

/// True if `def`'s nearest enclosing *definition* is a `class_definition` (⇒ Method), as opposed
/// to a `function_definition` (⇒ nested Function) or module level. Climbs past structural nodes
/// (`block`, `decorated_definition`, ERROR, …) and stops at the first definition ancestor.
fn python_parent_is_class(def: Node) -> bool {
    let mut cur = def.parent();
    while let Some(node) = cur {
        match node.kind() {
            "class_definition" => return true,
            "function_definition" => return false,
            _ => cur = node.parent(),
        }
    }
    false
}

/// True if `method`'s nearest enclosing *definition* is a `class_declaration` (⇒ Method). Climbs
/// past structural nodes (`class_body`, statements, ERROR, …) and stops at the first definition.
fn ts_parent_is_class(method: Node) -> bool {
    let mut cur = method.parent();
    while let Some(node) = cur {
        match node.kind() {
            "class_declaration" => return true,
            "function_declaration" | "arrow_function" | "method_definition" => return false,
            _ => cur = node.parent(),
        }
    }
    false
}

/// Read the UTF-8 text of `node`'s `field` child, if present and valid UTF-8.
fn field_text<'a>(node: Node, field: &str, source: &'a str) -> Option<&'a str> {
    let child = node.child_by_field_name(field)?;
    node_text(child, source)
}

/// Slice `node`'s exact bytes out of `source` (byte-exact, UTF-8-boundary safe).
fn node_text<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    source.get(node.start_byte()..node.end_byte())
}

/// Build a [`Chunk`] from a span node (already chosen to include decorators where relevant).
///
/// Returns `None` only if the span isn't a valid UTF-8 slice of `source` (e.g. truncated/broken
/// tree) — i.e. we drop a degenerate chunk rather than emit one that violates the span invariant.
fn build_chunk(
    span_node: Node,
    name: &str,
    symbol_type: SymbolType,
    lang: Language,
    file_path: &Path,
    parent: Option<&str>,
    source: &str,
) -> Option<Chunk> {
    let start_byte = span_node.start_byte();
    // Tree-sitter ends a definition at the last *content* byte, before the trailing newline. We
    // extend the span to include the single line terminator that closes the def's last line so a
    // chunk reads as a whole, self-contained source block (and CRLF `\r\n` is preserved verbatim).
    let end_byte = extend_to_line_end(source, span_node.end_byte());
    let text = source.get(start_byte..end_byte)?;
    Some(Chunk {
        symbol_name: name.to_string(),
        symbol_type,
        file_path: file_path.to_path_buf(),
        start_byte,
        end_byte,
        // Tree-sitter rows are 0-based; D7 line numbers are 1-based inclusive. The trailing
        // newline we appended belongs to the def's last content line, so the line range is
        // unchanged by the byte extension.
        start_line: span_node.start_position().row + 1,
        end_line: span_node.end_position().row + 1,
        chunk_text: text.to_string(),
        language: lang,
        parent_symbol: parent.map(str::to_string),
        // Enrichment (D3) beyond parent_symbol is filled by the M4 chunker.
        file_docstring: None,
        imports: Vec::new(),
        cross_references: Vec::new(),
        // AST path: a well-formed extraction is never heuristic (M4 sets this true on fallback).
        is_heuristic: false,
    })
}

/// Extend `end` (a byte offset on a UTF-8 / char boundary) to include the line terminator that
/// immediately follows it: a lone `\n`, or a `\r\n` pair (CRLF preserved). If `end` is not at a
/// line break (e.g. EOF without a trailing newline), it is returned unchanged. Operating on raw
/// bytes here is safe because `\r` (0x0D) and `\n` (0x0A) are single-byte ASCII and never appear
/// inside a multibyte UTF-8 sequence.
fn extend_to_line_end(source: &str, end: usize) -> usize {
    let bytes = source.as_bytes();
    match bytes.get(end) {
        Some(b'\n') => end + 1,
        Some(b'\r') if bytes.get(end + 1) == Some(&b'\n') => end + 2,
        _ => end,
    }
}

/// Syntactic error density of `tree` in `[0, 1]` (Decision Log D2): the count of `ERROR` +
/// `MISSING` nodes over the count of **named** nodes.
///
/// Rationale for the *named-node* denominator: tree-sitter materializes every literal token
/// (`(`, `)`, `:`, `+`, `def`, …) as an anonymous node. Those carry no independent syntactic
/// meaning, and including them in the denominator dilutes the error signal so heavily that even a
/// file that is almost entirely garbage scores well under any sane fallback threshold. Named nodes
/// are the meaningful syntactic units, so the ratio "broken units / meaningful units" is the
/// honest measure of how much of the file tree-sitter could not understand. The numerator counts
/// every `ERROR`/`MISSING` node (named or not, so a single `MISSING` anonymous token — e.g. an
/// unclosed paren — still yields a positive rate). The result is clamped into `[0, 1]`.
///
/// `error_rate(valid) == 0.0`; a malformed file reports `> 0.0`. Walks every node once via a
/// `TreeCursor` (no recursion-depth limit, no per-node allocation, never panics).
pub fn error_rate(tree: &Tree) -> f32 {
    let mut named: u64 = 0;
    let mut bad: u64 = 0;

    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        if node.is_named() {
            named += 1;
        }
        if node.is_error() || node.is_missing() {
            bad += 1;
        }

        // Depth-first traversal without recursion.
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                // Back at the root with no more siblings: done.
                return ratio(bad, named);
            }
        }
    }
}

/// `bad / named` clamped into `[0, 1]`; `0.0` when there are no named nodes (defensive — a real
/// tree always has at least the named `module`/`source_file` root).
fn ratio(bad: u64, named: u64) -> f32 {
    if named == 0 {
        return 0.0;
    }
    (bad as f32 / named as f32).clamp(0.0, 1.0)
}

/// Whether a file with the given ERROR `rate` should be routed to the M4 heuristic chunker (D2).
/// `should_fall_back(0.0) == false`; true at or above [`HEURISTIC_FALLBACK_THRESHOLD`].
pub fn should_fall_back(rate: f32) -> bool {
    rate >= HEURISTIC_FALLBACK_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_a_sane_fraction() {
        assert!((0.0..1.0).contains(&HEURISTIC_FALLBACK_THRESHOLD));
    }

    #[test]
    fn should_fall_back_respects_threshold() {
        assert!(!should_fall_back(0.0));
        assert!(should_fall_back(HEURISTIC_FALLBACK_THRESHOLD));
        assert!(should_fall_back(1.0));
    }

    #[test]
    fn queries_compile_against_grammar() {
        // `Parser::new` validates the embedded `.scm`; surfacing it as a test documents the seam.
        assert!(Parser::new().is_ok());
    }

    #[test]
    fn unsupported_language_error_displays_typed_message() {
        // All three v0.1 `Language` variants are now wired (M9), so `parse_file` no longer rejects
        // any of them — but `ParserError::UnsupportedLanguage` is still live public API: it is the
        // typed error a future, not-yet-wired language would hit before its grammar lands. Keep the
        // variant and its Display under test as the intentional forward-compat contract.
        let e = ParserError::UnsupportedLanguage(crate::types::Language::Go);
        assert!(matches!(e, ParserError::UnsupportedLanguage(_)));
        assert_eq!(e.to_string(), "unsupported language for parsing: go");
    }
}
