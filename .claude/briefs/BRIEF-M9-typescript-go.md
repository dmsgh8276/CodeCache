# BRIEF — M9 / TypeScript + Go parsers

- **Milestone:** M9 — TypeScript + Go parsers  ·  **Module(s):** `parser` (+ `indexer` validation in M9.3)
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-12
- **Status (M9.1):** RED ✅  GREEN ✅  REVIEW ✅ (APPROVE)  DONE ✅
- **Status (M9.2):** RED ✅  GREEN ✅  REVIEW ✅ (APPROVE)  DONE ✅
- **Status (M9.3):** RED ▢  GREEN ▢  REVIEW ▢  DONE ▢
- **Links:** docs/ROADMAP.md#m9 · docs/plans/M9-typescript-go.md · docs/TEST_STRATEGY.md#parser-python--ts--go · docs/project_plan.md §5.3

## Goal
Add `tree-sitter-typescript` and `tree-sitter-go` `LanguageConfig`s + `.scm` extraction queries so
language coverage = Python / TypeScript / Go. TS and Go must produce the **exact same `Chunk` shape**
and honor the **same contracts** as the M3 Python parser, so chunker/indexer/retriever/formatter/MCP
all work unchanged. Validation milestone — no public API changes, no enum changes.

## The Python contract TS/Go MUST match (from M3 — non-negotiable)
1. **Byte-exact spans:** `&source[start_byte..end_byte] == chunk_text`, UTF-8-boundary correct,
   CRLF preserved. Span extended to include the single trailing line terminator (`\n`/`\r\n`) that
   closes the def's last line (`extend_to_line_end`).
2. **D7 line numbers:** `start_line`/`end_line` 1-based inclusive; the appended terminator does not
   advance `end_line`.
3. **Method vs function:** a function whose nearest *definition* ancestor is a class/struct-receiver
   context is `SymbolType::Method` with `parent_symbol = <enclosing>`; otherwise `Function` with
   `parent_symbol = <enclosing fn>` (nested) or `None` (top-level).
4. **D2 graceful degradation:** `error_rate` (shared, language-agnostic — already walks any tree)
   and `should_fall_back` apply unchanged; `parse_file`/`extract_chunks` never panic on malformed
   input, return `Ok` (possibly empty). Per-language parity tests required.
5. **Deterministic:** chunks sorted by `start_byte`.
6. **No reachable `unwrap()/expect()/panic!`** on any library path; drop a degenerate chunk
   (non-UTF-8 slice) rather than emit one violating the span invariant.

## Architecture note for the implementer (collect_chunks is Python-specific)
`src/parser/mod.rs::collect_chunks` currently hard-codes Python node kinds (`function_definition`,
`class_definition`, `decorated_definition`) and `parent_is_class`. TS and Go use different node
kinds (`function_declaration`, `class_declaration`, `method_definition`, `arrow_function`,
`variable_declarator`, `method_declaration` with `receiver`, `type_declaration`/`struct_type`).
**The walk must branch per `Language`** (or dispatch to a per-language node-kind table) while
reusing the shared helpers (`build_chunk`, `extend_to_line_end`, `field_text`, `node_text`,
`span_node_for`). Keep the dispatch minimal and idiomatic — engineering-lead + specialist decide
the cleanest shape (match on `lang` inside `collect_chunks`, or a small per-language strategy). Do
NOT regress Python behavior — all 14 existing `parser_tests.rs` cases must stay green.

## §5.3 documented queries (the capture-name contract; validated in `Parser::new`)
**TypeScript:** `function_declaration` (name: identifier, params: formal_parameters, body:
statement_block); arrow fns via `variable_declarator` (name: identifier, value: arrow_function);
`class_declaration` (name: type_identifier, body: class_body); `method_definition` (name:
property_identifier, params: formal_parameters, body: statement_block).
**Go:** `function_declaration` (name: identifier, params: parameter_list, body: block);
`method_declaration` (receiver: parameter_list, name: field_identifier, …) → Method;
`type_declaration`/`type_spec` (name: type_identifier, type: struct_type) → `SymbolType::Struct`.

## Manager decisions (signed off — record in OUTCOME / parser CLAUDE.md)
- **Deps:** `tree-sitter-typescript = "0.23"` and `tree-sitter-go = "0.23"` are ALREADY in
  `Cargo.toml` (declared at M0, §10.3). No add/pin needed. **MSRV stays 1.85.** No deviation.
- **No enum changes:** `Language::{TypeScript, Go}` and `SymbolType::{Function, Class, Method,
  Struct}` already exist. If the implementer believes a new `SymbolType` variant is needed (e.g.
  for TS `interface`/`type` alias), STOP and escalate to manager — that is a cross-module change
  requiring `project_plan.md` §4.3 update first.
- **`.ts` vs `.tsx` grammar selection:** the `tree-sitter-typescript` crate exposes two languages,
  `LANGUAGE_TYPESCRIPT` and `LANGUAGE_TSX`. **Decision:** v0.1 loads the **TypeScript** grammar for
  `Language::TypeScript`. `detect_language` currently maps only `.ts` (not `.tsx`) → TypeScript, so
  no `.tsx` files reach the parser today; wiring `.tsx` discovery is **deferred** (out of M9 scope)
  unless an M9.1 fixture forces it. The `tsx_or_type_only_constructs_no_panic` test targets TS
  **type-only constructs** (interfaces, type aliases, generics) parsing without panic under the
  TypeScript grammar — NOT JSX. Document this in `src/parser/CLAUDE.md`.
- **Interfaces / type aliases:** §5.3 does not list extraction queries for TS `interface`/`type`.
  v0.1 extracts functions/arrow-fns/classes/methods only; interfaces/type-aliases are NOT emitted
  as chunks (they just must not panic). If recall data later justifies them, that is a post-v0.1
  plan change. Keep scope to the §5.3 contract.

## Scope (in / out)
- **In:** TS + Go `LanguageConfig` + `.scm` + per-language fixtures + extraction wired through the
  shared `Parser`; D2 parity; D7 lines; mixed-repo indexer validation (M9.3).
- **Out:** `.tsx`/JSX discovery routing; TS interface/type-alias chunks; D3 enrichment beyond
  `parent_symbol` (M4 chunker owns imports/cross_refs — confirm it is language-agnostic, file a
  follow-up if TS/Go enrichment needs work); mixed-language aggregate benches (M10).

## Ordered slices

### M9.1 — TypeScript config + extraction (task #10)
- **RED (test-lead):** `tests/parser_ts_tests.rs` + `tests/fixtures/typescript/**`. Mirror the
  `parser_tests.rs` helpers (`assert_span_slices_to_text`, `one_named`). Scenarios:
  `extracts_function_declaration_with_exact_span`, `extracts_arrow_function_assigned_to_variable`,
  `extracts_class_declaration_and_method_definition`, `generics_handled`,
  `tsx_or_type_only_constructs_no_panic` (interfaces/type-aliases/generics, no panic, valid spans),
  `high_error_rate_ts_file_flags_heuristic` (D2 parity). Fixtures small, deterministic; record
  newline style per file in `tests/CLAUDE.md`.
- **GREEN (specialist + eng-lead):** `src/parser/typescript.rs` (config: `LANGUAGE_TYPESCRIPT` +
  `queries/typescript.scm`), wire into `Parser::new` (validate `.scm` vs grammar), extend
  `collect_chunks` dispatch. Method = `method_definition` inside `class_declaration` (parent =
  class). Arrow fn assigned to `variable_declarator` → Function named by the declarator identifier.

### M9.2 — Go config + extraction (task #11, blocked by M9.1)
- **RED:** `tests/parser_go_tests.rs` + `tests/fixtures/go/**`:
  `extracts_function_declaration_with_exact_span`, `extracts_method_declaration_with_receiver`
  (Method, parent = receiver type name), `extracts_struct_type_as_struct_symbol`
  (`SymbolType::Struct`), `package_and_imports_handled` (no spurious chunks for package/import
  decls), `high_error_rate_go_file_flags_heuristic` (D2 parity).
- **GREEN:** `src/parser/go.rs` + `queries/go.scm`; wire into `Parser::new`; extend dispatch. Go
  method receiver type → `parent_symbol` (e.g. `func (s *Server) Handle()` → parent `Server`).

### M9.3 — cross-language integration through indexer (task #12, blocked by M9.2)
- **RED:** `tests/e2e_multilang.rs` (or extend `tests/indexer_tests.rs`):
  `index_mixed_repo_indexes_python_ts_and_go_files`,
  `language_filter_in_config_restricts_indexed_languages`.
- **GREEN:** validation — `detect_language` already routes `.py/.ts/.go`. Confirm the full
  pipeline (discovery → parse → chunk → store) works for all three. Fix detection only if a gap
  surfaces. `pipeline.rs::detect_language(path).unwrap_or(Language::Python)` fallback is benign
  here (discovery already filtered to configured languages) but verify TS/Go files get the right
  language stamped.

## Definition of Done (per slice + phase)
- [ ] Tests written first (RED), now green · `cargo clippy --all-targets -- -D warnings` clean · `cargo fmt` clean
- [ ] Byte-exact spans (Python contract) · D7 lines · D2 parity · deterministic order · no reachable unwrap/expect/panic
- [ ] API matches project_plan §5.3 capture names · no enum/public-API change (escalate if needed)
- [ ] All 14 existing Python `parser_tests.rs` cases still green (no regression)
- [ ] reviewer APPROVED
- [ ] `docs/TODO.md` Phase 9 + `src/parser/CLAUDE.md` updated in the SAME change · `tests/CLAUDE.md` fixture rows added
- [ ] one commit per slice (message style matching M6/M7/M8); all four gates green on Rust 1.85

---
## RED — test lead

### M9.1 — TypeScript (2026-06-12) — RED ✅
**Tests added** — `tests/parser_ts_tests.rs` (7 tests; mirrors `parser_tests.rs` helpers
`fixture_path`/`load_fixture`/`chunks_of`/`one_named`/`assert_span_slices_to_text`, all driving
`Language::TypeScript`):
- `extracts_function_declaration_with_exact_span` — `Function`, `TypeScript`, exact span, D7 lines, `parent_symbol = None`.
- `extracts_arrow_function_assigned_to_variable` — arrow fn → `Function` named `bar`, exact span (see naming rule below).
- `extracts_class_declaration_and_method_definition` — `Foo` ⇒ `Class`, `greet` ⇒ `Method` with `parent_symbol = Some("Foo")`; both exact-span.
- `generics_handled` — `identity<T>` extracts with exact span (type params must not break it).
- `tsx_or_type_only_constructs_no_panic` — interfaces/type-aliases/generics parse + extract `Ok`, no panic; `makePair`/`Circle` ARE found; `Shape`/`Pair` are NOT emitted; all survivors satisfy span invariant.
- `high_error_rate_ts_file_flags_heuristic` — `error_rate >= HEURISTIC_FALLBACK_THRESHOLD`, `should_fall_back(rate)` true; parse/extract `Ok`, no panic (D2 parity).
- `async_function_extracted` — `async function fetchData(...)` ⇒ `Function`, span includes `async`.

**Fixtures added** — `tests/fixtures/typescript/` (all **LF**, exact bytes — do NOT reformat):
`top_level_function.ts`, `arrow_function.ts`, `class_with_method.ts`, `generics.ts`,
`type_only.ts`, `high_error.ts`, `async_function.ts`. (`tests/CLAUDE.md` updated with rows.)

**RED output** — `cargo test --test parser_ts_tests`: compiles clean; **0 passed / 7 failed**.
All fail for the right reason — `Parser::parse_file(.., Language::TypeScript)` returns
`ParserError::UnsupportedLanguage(TypeScript)` ("unsupported language for parsing: typescript"),
i.e. TS is not yet wired in `Parser::new`. No compile errors; no other test files touched.

**What the implementer (M9.1 GREEN) must satisfy — exact expectations:**
1. **Wire TS in `Parser::new`** (`LANGUAGE_TYPESCRIPT`, not TSX) + `queries/typescript.scm`
   validated against the grammar; branch `collect_chunks` per `Language` (it is currently
   hard-coded to Python node kinds).
2. **Hard-coded `chunk_text` (strongest off-by-one guards) — must match byte-for-byte:**
   - `top_level_function.ts` / `foo`:
     `"function foo(name: string): string {\n  return \"hi \" + name;\n}\n"` (start_line 1, end_line 3).
   - `arrow_function.ts` / `bar`:
     `"bar = (x: number) => {\n  return x + 1;\n}"` (start_line 1, end_line 3).
   - `generics.ts` / `identity`:
     `"function identity<T>(x: T): T {\n  return x;\n}\n"` (start_line 1, end_line 3).
3. **Arrow-fn naming rule (§5.3):** the arrow fn is extracted via the **`variable_declarator`**
   (name: identifier `bar`, value: arrow_function) → `SymbolType::Function` named by the
   declarator identifier. The **span node is the `variable_declarator`**, so the span starts at
   the identifier `bar` (excludes the `const ` keyword) and ends at the arrow body's `}`. Because
   the byte right after the declarator is `;` (not `\n`/`\r`), `extend_to_line_end` appends NO
   terminator — hence the expected text has no trailing `;` or `\n`. If the implementer instead
   spans the whole `lexical_declaration`/statement, this assertion catches it.
4. **Method typing:** `method_definition` inside `class_declaration` ⇒ `SymbolType::Method`,
   `parent_symbol = Some("<class name>")` (class name is a `type_identifier`); the class itself
   ⇒ `SymbolType::Class`, `parent_symbol = None`.
5. **Interfaces / type aliases are NOT chunks** in v0.1 — `Shape`/`Pair` must not appear; only
   functions / arrow-fns / classes / methods are emitted (§5.3 contract). Do not add a
   `SymbolType` variant — escalate to manager if you think one is needed.
6. **D2 parity** comes for free from the shared language-agnostic `error_rate`/`should_fall_back`
   once parsing is wired; verify the garbage fixture clears the 0.20 threshold.
7. All 14 existing Python `parser_tests.rs` cases must stay green (no Python regression).

### M9.2 — Go (2026-06-12) — RED ✅

**Tests added** — `tests/parser_go_tests.rs` (5 tests; mirrors `parser_tests.rs`/`parser_ts_tests.rs`
helpers `fixture_path`/`load_fixture`/`parse_fixture`/`chunks_of`/`one_named`/
`assert_span_slices_to_text`, all driving `Language::Go` against `tests/fixtures/go/`):
- `extracts_function_declaration_with_exact_span` — `func Foo` ⇒ `Function`, `Go`, exact span via
  `assert_span_slices_to_text` + hard-coded `chunk_text`, D7 lines 3-5, `parent_symbol = None`.
- `extracts_method_declaration_with_receiver` — `func (s *Server) Handle(...)` ⇒ `Method`,
  `parent_symbol = Some("Server")` (receiver TYPE name), exact span + hard-coded `chunk_text`,
  D7 lines 7-9.
- `extracts_struct_type_as_struct_symbol` — `type Point struct {...}` ⇒ `SymbolType::Struct`,
  name `Point`, exact span + hard-coded `chunk_text`, D7 lines 3-6, `parent_symbol = None`.
- `package_and_imports_handled` — `package main` + `import (...)` + `func Run` ⇒ only `Run`
  extracted (`chunks.len() == 1`, no `main`/import chunks), exact span, D7 lines 8-10, Ok no panic.
- `high_error_rate_go_file_flags_heuristic` — garbage Go ⇒ `error_rate >= HEURISTIC_FALLBACK_THRESHOLD`,
  `should_fall_back(rate)` true, parse/extract `Ok` no panic, survivors keep the span invariant (D2 parity).

**Fixtures added** — `tests/fixtures/go/` (all **LF**, gofmt-style **leading tabs**, exact bytes —
do NOT reformat): `top_level_function.go`, `method_with_receiver.go`, `struct_type.go`,
`package_and_imports.go`, `high_error.go`. (`tests/CLAUDE.md` updated with the layout row + a go
fixture table.) No interface fixture added (§5.3 has no Go interface query; interfaces not in scope).

**Receiver-type expectation (decision documented in-test):** the Go method receiver
`(s *Server)` ⇒ `parent_symbol = Some("Server")`. The implementer must walk the
`method_declaration`'s `receiver:` `parameter_list`, take the receiver's TYPE, and strip both the
pointer (`*Server` → `Server`, i.e. the `pointer_type`'s inner `type_identifier`) and the receiver
variable name `s`. A non-pointer receiver `(s Server)` must yield the same `Server`.

**RED output** — `cargo test --test parser_go_tests`: compiles clean; **0 passed / 5 failed**.
All fail for the right reason — `Parser::parse_file(.., Language::Go)` returns
`ParserError::UnsupportedLanguage(Go)` ("unsupported language for parsing: go"), i.e. Go is not yet
wired in `Parser::new`. No compile errors. `cargo test --test parser_tests` (14) and
`--test parser_ts_tests` (7) both still pass — no production code or existing tests/fixtures touched.

**What the implementer (M9.2 GREEN) must satisfy — exact expectations:**
1. **Wire Go in `Parser::new`** (`tree_sitter_go::LANGUAGE` + `queries/go.scm` validated against the
   grammar); add a `recognize_go` arm to the `recognize_definition` dispatch (Go currently returns
   `None`). Do NOT regress Python (14) or TS (7).
2. **Hard-coded `chunk_text` (strongest off-by-one guards) — must match byte-for-byte (note the
   leading TAB `\t` indentation inside bodies):**
   - `top_level_function.go` / `Foo`:
     `"func Foo(name string) string {\n\treturn \"hi \" + name\n}\n"` (Function, lines 3-5, parent None).
   - `method_with_receiver.go` / `Handle`:
     `"func (s *Server) Handle(path string) string {\n\treturn s.addr + path\n}\n"`
     (Method, lines 7-9, `parent_symbol = Some("Server")`).
   - `struct_type.go` / `Point`:
     `"type Point struct {\n\tX int\n\tY int\n}\n"` (Struct, lines 3-6, parent None).
   - `package_and_imports.go` / `Run`:
     `"func Run(name string) string {\n\treturn fmt.Sprint(strings.ToUpper(name))\n}\n"`
     (Function, lines 8-10).
3. **Struct typing (§5.3):** `type_declaration` → `type_spec` (name: `type_identifier`, type:
   `struct_type`) ⇒ `SymbolType::Struct`, named by the `type_spec` name; span node = the
   `type_declaration` (starts at the `type` keyword). Note `method_with_receiver.go` also contains
   a `Server` struct that WILL be emitted as a Struct chunk — that is expected (the test only
   asserts on `Handle`, and `one_named("Handle")` is unaffected by the `Server` chunk).
4. **Method receiver → parent_symbol:** `method_declaration` (the `func` form WITH a `receiver:`
   `parameter_list`) ⇒ `SymbolType::Method`, `parent_symbol` = the receiver TYPE name with pointer
   `*` and receiver variable stripped (`(s *Server)` → `Server`). A plain `function_declaration`
   (no receiver) ⇒ `Function`, `parent_symbol = None`.
5. **No spurious chunks:** the `package` clause and `import (...)` declarations must NOT be emitted
   — `package_and_imports.go` must yield exactly one chunk (`Run`).
6. **D2 parity** comes for free from the shared language-agnostic `error_rate`/`should_fall_back`
   once parsing is wired; verify `high_error.go` clears the 0.20 threshold.
7. **No interface chunks:** §5.3 lists no Go interface query — do not emit interfaces in v0.1.

## GREEN — engineering lead

### M9.1 — TypeScript (2026-06-12) — GREEN ✅

**Files added/changed:**
- `src/parser/queries/typescript.scm` — the §5.3 TS queries verbatim: `function_declaration`
  (name: identifier, parameters: formal_parameters, body: statement_block); arrow via
  `variable_declarator` (name: identifier, value: arrow_function with `parameters`/`body`);
  `class_declaration` (name: type_identifier, body: class_body); `method_definition` (name:
  property_identifier, parameters: formal_parameters, body: statement_block). Same
  `@function.*/@class.*/@method.*` capture convention as `python.scm`. Compiled+validated against
  the grammar in `Parser::new` — all captures/fields/node kinds match the grammar exactly.
- `src/parser/typescript.rs` — mirrors `python.rs`: `pub const TYPESCRIPT_QUERIES =
  include_str!("queries/typescript.scm")` + `pub fn config() -> LanguageConfig` using
  **`tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()`** (not TSX, per manager decision).
- `src/parser/mod.rs` — `mod typescript;`; in `Parser::new`, `Query::new(&ts.grammar, ts.queries)?`
  then insert under `Language::TypeScript` (mirrors the Python block).

**`collect_chunks` dispatch shape:** refactored the walk so the language-specific knowledge lives
in one place: `collect_chunks` calls `recognize_definition(node, lang, source) -> Option<Definition>`,
which `match lang`-dispatches to `recognize_python` / `recognize_typescript` (Go → `None`, lands
M9.2). A `Definition { span_node, name, symbol_type }` carries the per-language decision back to the
shared, unchanged helpers (`build_chunk`, `extend_to_line_end`, `field_text`, `node_text`). The
Python decorator/method helpers were renamed `python_span_node_for` / `python_parent_is_class`
(behavior identical) and a TS sibling `ts_parent_is_class` added. No shared-helper logic changed,
so Python behavior is byte-for-byte preserved.

**Grammar symbol:** `LANGUAGE_TYPESCRIPT` (TypeScript grammar, not TSX).

**Arrow-fn span handling:** the `variable_declarator` is recognized only when its `value` field is
an `arrow_function`; the **span node is the `variable_declarator` itself**, named by its `name`
identifier (`bar`). It therefore starts at `bar` (excludes `const `), and because the next byte is
`;` (not `\n`), `extend_to_line_end` appends no terminator — matching the expected
`"bar = (x: number) => {\n  return x + 1;\n}"` byte-for-byte. TS uses no decorator wrapper, so the
span node is the node itself for every TS kind. Method classification (`ts_parent_is_class`) climbs
ancestors and stops at the first definition: `class_declaration` ⇒ Method, otherwise Function.

**Interfaces/type-aliases:** not recognized (no match arm) ⇒ never emitted; `Shape`/`Pair` absent
as required. No `SymbolType` variant added; no plan deviation.

**Tests pass:**
- `cargo test --test parser_ts_tests` → **7 passed / 0 failed** (all RED scenarios green incl.
  exact `chunk_text` for `foo`/`bar`/`identity`, method parent `Some("Foo")`, type-only no-panic,
  D2 high-error parity, async span).
- `cargo test --test parser_tests` → **14 passed** (no Python regression;
  `unsupported_language_expects_typed_error` still passes — Go stays unsupported).
- `cargo test` (full suite) → **173 passed / 0 failed** (= 166 + 7), confirming the brief's count.
- `cargo clippy --all-targets -- -D warnings` → clean. `cargo fmt --check` → clean.

**Plan deviations raised:** none — the §5.3 contract and manager decisions held exactly.

### M9.2 — Go (2026-06-12) — GREEN ▲ (blocked by a stale M3 test — ESCALATION below)

**Files added/changed:**
- `src/parser/queries/go.scm` — the §5.3 Go queries verbatim: `function_declaration` (name:
  identifier, parameters: parameter_list, body: block); `method_declaration` (receiver:
  parameter_list, name: field_identifier, parameters: parameter_list, body: block);
  `type_declaration`→`type_spec` (name: type_identifier, type: struct_type). Same
  `@function.*/@method.*/@struct.*` capture convention as python/typescript. Compiled + validated
  against `tree_sitter_go::LANGUAGE` in `Parser::new` — all captures/fields/node kinds match the
  grammar exactly (`Parser::new` succeeds; `queries_compile_against_grammar` unit test green).
- `src/parser/go.rs` — mirrors `typescript.rs`: `pub const GO_QUERIES = include_str!("queries/go.scm")`
  + `pub fn config() -> LanguageConfig` using `tree_sitter_go::LANGUAGE.into()`. `mod go;` worked
  (no keyword collision); no path attribute needed.
- `src/parser/mod.rs` — `mod go;`; in `Parser::new`, `Query::new(&go.grammar, go.queries)?` then
  insert under `Language::Go`; `recognize_definition` now dispatches `Language::Go => recognize_go`.

**`recognize_go` arm:**
- `function_declaration` → `Function`, name from `name` (identifier), span = the node itself,
  `parent_override: None` (top-level ⇒ parent None).
- `method_declaration` → `Method`, name from `name` (field_identifier), span = the node,
  `parent_override: go_receiver_type_name(node, source)` (the receiver TYPE name).
- `type_declaration` → only emits when its `type_spec` child's `type:` field is a `struct_type`:
  `Struct`, name from the `type_spec`'s `name` (type_identifier), **span node = the
  `type_declaration`** (not the `type_spec`).
- package clause / import declarations have no arm ⇒ never emitted.

**Receiver-type helper `go_receiver_type_name(method, source) -> Option<&str>`:** drills
`method_declaration` → `child_by_field_name("receiver")` → first `parameter_declaration` child →
`child_by_field_name("type")`; if that node is a `pointer_type`, descends to its inner
`type_identifier`; else uses the type node directly; reads its text via `node_text`. Every step is
`?`/`Option` — no reachable unwrap/expect/panic. On the valid fixtures `(s *Server)` resolves to
`Some("Server")` (pointer + receiver var stripped); a plain `(s Server)` would resolve the same.

**`Definition.parent_override` (minimal cross-language change):** `parent_symbol` is normally the
nearest enclosing definition threaded down the walk, but a Go method's parent is the receiver type,
not a lexical ancestor. Added `parent_override: Option<&'a str>` to `Definition`; `collect_chunks`
uses `def.parent_override.or(parent)`. Python/TS recognizers set it to `None` (behavior byte-for-byte
unchanged — their 14 + 7 tests still green); only the Go method arm sets it.

**Struct span node choice (why it matches the expected bytes):** the RED test expects
`"type Point struct {\n\tX int\n\tY int\n}\n"` starting at line 3 — i.e. the span must begin at the
`type` keyword. The `type_spec` node starts at `Point` (after `type `), so spanning the `type_spec`
would drop the `type ` prefix and fail the byte assertion. Spanning the **`type_declaration`** (which
covers `type` … closing `}`) plus the shared `extend_to_line_end` (appends the trailing `\n`)
reproduces the expected bytes and lines 3-6 exactly. Same reasoning gives `method_with_receiver.go`'s
`Server` struct an extra (expected) chunk — `one_named("Handle")` is unaffected.

**Tests pass:**
- `cargo test --test parser_go_tests` → **5 passed / 0 failed** (exact `chunk_text` for Foo/Handle/
  Point/Run incl. leading-tab bodies; Handle parent `Some("Server")`; D7 lines 3-5/7-9/3-6/8-10;
  package+imports yields exactly one chunk; D2 high-error parity).
- `cargo test --test parser_ts_tests` → **7 passed** (no TS regression).
- `cargo clippy --all-targets -- -D warnings` → clean. `cargo fmt` → `src/` clean (only `mod.rs`
  reformatted; the two `tests/parser_go_tests.rs` fmt diffs are pre-existing test-lead formatting,
  not touched per the no-modify-tests rule).

**⚠ ESCALATION — stale M3 test now fails (NOT modified, per the hard rules):**
`tests/parser_tests.rs::unsupported_language_expects_typed_error` asserts that
`parse_file(.., Language::Go)` returns `Err` "an unsupported language (Go at M3) must return a typed
Err". Wiring Go in M9.2 makes Go a *supported* language, so this assertion is now structurally
impossible to satisfy while delivering the M9.2 mission. The `Language` enum has only
`{Python, TypeScript, Go}` — all three are now wired — so there is no remaining unsupported language
to use as the sentinel. The M9.2 RED note (this brief, lines 204-205) assumed `parser_tests` would
stay 14/14 green, which did not anticipate that Go-wiring invalidates this specific M3-era test.
I did **not** weaken, skip, or delete it. **Decision needed from manager/test-lead:** either
(a) the test-lead repoints `unsupported_language_expects_typed_error` at a genuinely-unsupported
input (e.g. assert `extract_chunks`/`parse_file` typed-error behavior via a construction the API can
still reject), or (b) the test is retired/relocated as a superseded M3 invariant. Until then the
full suite is **177 green + 1 failing (this stale test)**; the M9.2 Go behavior itself is fully green.

**Plan deviations raised:** none on the §5.3 contract or types (no enum/API change; `Struct` reused).
The only blocker is the stale M3 test above, raised rather than silently fixed.

## RED fixup — test lead (M9.2) — 2026-06-12

**Resolution (manager-approved, signed off in the M9.2 fixup brief): repoint, not weaken.** The
stale `tests/parser_tests.rs::unsupported_language_expects_typed_error` asserted that
`parse_file(.., Language::Go)` returns `Err(UnsupportedLanguage)` — true at M3 (Python-only), now
structurally impossible since M9.1/M9.2 wired all three `Language` variants. The engineering lead
correctly escalated rather than weaken it. Replaced the one now-false test with TWO honest
assertions that preserve and strengthen the original intent:

1. **Repointed integration test** — `tests/parser_tests.rs`:
   `unsupported_language_expects_typed_error` → renamed `all_v01_languages_parse_supported`. Now
   asserts the real M9 exit guarantee: `parse_file` returns `Ok` (and no ERROR nodes) for ALL THREE
   v0.1 languages on tiny inline snippets — Python `"def f():\n    pass\n"`, TypeScript
   `"function f() {}\n"`, Go `"package main\nfunc F() {}\n"` (no new fixtures). A comment cites M9
   so future readers know why the old "Go unsupported" claim is gone. `parser_tests.rs` stays at
   14 tests (one repointed in place — no coverage deleted, no `#[ignore]`).

2. **New focused unit test** — `src/parser/mod.rs` `#[cfg(test)] mod tests`:
   `unsupported_language_error_displays_typed_message` keeps the `ParserError::UnsupportedLanguage`
   variant + its `Display` ("unsupported language for parsing: go") under test — it is still live
   public API (the typed error a future, not-yet-wired language hits before its grammar lands).
   Asserts `matches!(.., UnsupportedLanguage(_))` and the exact `to_string()`. Test code only — no
   production line in `mod.rs` touched.

**Rationale recorded:** M3 baked in "Go = unsupported"; M9 deliberately makes that false by
delivering Go support. The intent — a typed error for an unwired language, and all v0.1 languages
parse — is preserved and strengthened, not weakened.

**Constraint compliance:** edited ONLY `tests/parser_tests.rs` and the `#[cfg(test)] mod tests`
block in `src/parser/mod.rs`. No production code (go.rs/typescript.rs/python.rs/queries/non-test
`mod.rs` lines) touched. `tests/parser_go_tests.rs` received a pure `cargo fmt` whitespace reflow
of two pre-existing `assert_eq!` calls (the diffs the M9.2 GREEN note flagged as pre-existing
test-lead formatting) so the `fmt --check` gate is clean — no assertion value or logic changed.

**Gate status (all green, Rust 1.85):**
- `cargo test` (full suite) → **179 passed / 0 failed** (prior 173 + 5 Go = 178; the repoint
  removes the 1 stale integration test and adds 1 new unit test → net 179, 0 failing).
  `parser_tests.rs` 14/14 (incl. `all_v01_languages_parse_supported`); new unit test green.
- `cargo clippy --all-targets -- -D warnings` → clean (exit 0).
- `cargo fmt --check` → clean (exit 0).

**Hand to:** principal-engineering-lead / manager — M9.2 GREEN is now fully unblocked (suite green);
proceed to REVIEW.

## Specialist / Perf notes
<tree-sitter node-kind findings (TS arrow/method/generics, Go receiver/struct), ERROR-node tuning, grammar API (LANGUAGE_TYPESCRIPT vs LANGUAGE_TSX)>

## REVIEW — code reviewer

### M9.1 — TypeScript (2026-06-12) — VERDICT: APPROVE (with two non-blocking findings)

**Gates re-run locally (Rust toolchain, this branch):**
- `cargo fmt --check` → clean (exit 0).
- `cargo clippy --all-targets -- -D warnings` → clean (exit 0).
- `cargo test` → 173 passed / 0 failed. `parser_tests.rs` 14/14 (no Python regression),
  `parser_ts_tests.rs` 7/7. Total reconciles to 173 exactly as the GREEN note claims.

**Correctness — verified:**
- Byte-exact span invariant holds for every TS chunk (`assert_span_slices_to_text` over all
  survivors incl. type_only + high_error). Hard-coded `chunk_text` for foo/bar/identity match
  byte-for-byte; D7 lines 1-based inclusive; appended terminator does not advance `end_line`.
- Arrow-fn rule correct: only `variable_declarator` whose `value` field is `arrow_function`
  is recognized; span node = the declarator (excludes `const `; trailing `;` not appended
  because `extend_to_line_end` sees `;` not `
`). Matches the expected `bar` text exactly.
- Method classification correct: `method_definition` under `class_declaration` → Method,
  `parent_symbol = Some("Foo")`; class → Class, parent None; top-level fn → Function, parent None.
  `ts_parent_is_class` climbs to first definition ancestor, stopping at
  function_declaration/arrow_function/method_definition (Function) or class_declaration (Method).
- No double-emission: `arrow_function` and inner `class_body`/`statement_block` have no recognize
  arm, so recursion finds nested defs without re-emitting wrappers.
- Generics + async: `identity<T>` and `async function fetchData` extract with exact/`async`-inclusive
  spans (type-param list does not break the byte span).
- D2 parity is free from the shared language-agnostic `error_rate`/`should_fall_back`; the garbage
  fixture clears the 0.20 threshold; parse/extract return Ok, no panic.
- Interfaces/type-aliases not emitted (`Shape`/`Pair` absent); no `SymbolType`/`Language` variant
  added.

**No reachable unwrap/expect/panic** on any library path in the changed src: `recognize_*`,
`build_chunk`, `field_text`, `node_text`, `extend_to_line_end` all use `?`/`Option` and drop a
degenerate (non-UTF-8) slice rather than emit a span-violating chunk. `unwrap`/`expect` appear only
in `#[cfg(test)]` integration tests — acceptable.

**Alignment:** `queries/typescript.scm` matches `project_plan.md` §5.3 verbatim (capture names,
field names, node kinds). Uses `LANGUAGE_TYPESCRIPT` (not TSX) per manager decision. Cargo.toml
unchanged by this slice — `tree-sitter-typescript = "0.23"` was already committed at M0 (verified
via `git show HEAD:Cargo.toml`); no `streaming-iterator`. Python refactor preserves semantics: the
renamed `python_span_node_for`/`python_parent_is_class` are byte-for-byte the old logic; shared
helpers unchanged; the new `Definition`/`recognize_definition` dispatch is clean and idiomatic.

**Test adequacy:** the 7 tests genuinely exercise the scenario matrix with real-value asserts
(exact chunk_text where it bites, method parent, type-only exclusion, D2 threshold). No weakened
or `is_ok()`-only assertions on behavior that matters. Fixtures are LF, minimal, deterministic.

**Findings (both non-blocking — do not gate the slice):**
- minor (hygiene/DoD) — `src/parser/CLAUDE.md` + `docs/TODO.md`:`235-237` — The per-slice DoD and
  the repo golden rule require the module CLAUDE.md and `docs/TODO.md` Phase 9 to be updated in the
  same change. `tests/CLAUDE.md` was updated (typescript fixture rows present), but
  `src/parser/CLAUDE.md` still reads "Python only … TS/Go land at M9" with no M9.1 section
  (LANGUAGE_TYPESCRIPT decision, arrow-fn span rule, interfaces-not-emitted scope), and
  `docs/TODO.md` Phase 9 RED/GREEN items are still unchecked. Fix: add a TS subsection to
  `src/parser/CLAUDE.md` (grammar choice, §5.3 captures, arrow/method rules, scope) and check off
  the M9.1 line in `docs/TODO.md`. Reasonable to fold into the manager's integration commit, but
  it must land with this slice's commit, not after.
- minor (robustness, out of fixture scope) — `src/parser/mod.rs:284-294` — a `variable_declarator`
  with a destructuring `name` (e.g. `const {a} = () => {}`) would be emitted with `symbol_name`
  taken from the pattern text rather than an identifier. §5.3 specifies `name: (identifier)`; the
  TreeCursor walk does not enforce the pattern. No fixture hits this and the span invariant still
  holds, so it is harmless for v0.1. Optional fix: guard the arm on
  `node.child_by_field_name("name").is_some_and(|n| n.kind() == "identifier")` to mirror the query
  contract. Same applies to computed `method_definition` names — defer unless recall data justifies.

**Verdict: APPROVE.** Correctness, alignment, idiomatic Rust, and test adequacy all hold; all four
gates green on this branch. The only must-handle item is folding the CLAUDE.md/TODO doc updates
into the slice commit (minor, process), which I am routing back to the manager rather than blocking.

## OUTCOME — manager

### M9.1 — TypeScript (2026-06-12) — DONE ✅
- **Aligned:** code matches §5.3 capture contract; per-language `recognize_definition` dispatch is
  clean and preserves Python semantics (14/14 Python tests green); reviewer APPROVE (0 blocking).
- **Decisions recorded** (in `src/parser/CLAUDE.md`): `LANGUAGE_TYPESCRIPT` grammar; `.tsx`/JSX
  deferred (not in `detect_language`); interfaces/type-aliases not emitted as chunks; no enum/dep
  change (`tree-sitter-typescript 0.23` already at M0, MSRV stays 1.85). No deviation to escalate.
- **Reviewer finding #1 (docs hygiene):** resolved — `src/parser/CLAUDE.md` + `docs/TODO.md` Phase 9
  updated in the slice commit alongside `tests/CLAUDE.md`.
- **Reviewer finding #2 (TS destructuring declarator robustness):** non-blocking, out of fixture
  scope — recorded as a tracked follow-up under "Known follow-ups" in `src/parser/CLAUDE.md`. Not
  expanding M9.1 scope.
- **TODO:** Phase 9 M9.1 checked off. **Slice marked DONE.**
- **Commit:** `fa0c0705a834fe31f54f7eb9ae9f0bdf315e3113` — "M9.1: TypeScript parser …". Working
  tree clean; 173 tests green; all four gates clean on Rust 1.85.

### M9.2 — Go (2026-06-12) — VERDICT: APPROVE (with one non-blocking doc-hygiene finding)

**Gates re-run locally (Rust 1.85, this branch):**
- `cargo fmt --check` → clean (exit 0).
- `cargo clippy --all-targets -- -D warnings` → clean (exit 0).
- `cargo test` → **179 passed / 0 failed** (reconciles exactly to the RED-fixup count).
  parser_go_tests 5/5, parser_tests 14/14 (incl. `all_v01_languages_parse_supported`),
  parser_ts_tests 7/7, lib unit tests 29 (incl. the new
  `unsupported_language_error_displays_typed_message`).

**Correctness — verified:**
- Byte-exact span invariant holds for every Go chunk (`assert_span_slices_to_text` over all
  survivors incl. high_error). Hard-coded `chunk_text` for Foo/Handle/Point/Run match
  byte-for-byte against the committed fixtures (leading-tab bodies confirmed by reading the
  raw fixtures); D7 lines 3-5 / 7-9 / 3-6 / 8-10 1-based inclusive; appended `\n` does not
  advance `end_line`.
- `function_declaration` → Function, `parent_override: None`; `method_declaration` → Method,
  `parent_override = go_receiver_type_name(..)`. `go_receiver_type_name` drills
  receiver → first `parameter_declaration` → `type:`; `pointer_type` descends to inner
  `type_identifier`, else uses the type node directly — so both `(s *Server)` and `(s Server)`
  yield `Some("Server")`, receiver var `s` correctly ignored. Every step is `?`/`Option`.
- Struct: `type_declaration` emitted only when its `type_spec`'s `type:` field is a
  `struct_type`; span node = the `type_declaration` (starts at `type` keyword) → reproduces
  the expected `"type Point struct {...}\n"` bytes and lines 3-6. The extra `Server` struct in
  `method_with_receiver.go` is correctly emitted and does not perturb `one_named("Handle")`.
- Package clause / import declarations have no recognize arm ⇒ never emitted;
  `package_and_imports.go` yields exactly one chunk (`Run`), asserted by `chunks.len() == 1`.
- Go func literals are `func_literal` (not `function_declaration`), so method/function bodies
  with closures do not double-emit — the recursive walk is sound.
- D2 parity is free from the shared language-agnostic `error_rate`/`should_fall_back`; the
  garbage fixture clears the 0.20 threshold; parse/extract return Ok, no panic.

**`parent_override` design (the one shared-code change) — verified safe:**
- New `Definition.parent_override: Option<&'a str>` threaded through `collect_chunks` as
  `def.parent_override.or(parent)`. Python (`recognize_python`) and TS (`recognize_typescript`)
  set it to `None` on every arm, so `.or(parent)` collapses to the prior `parent` exactly —
  Python (14) and TS (7) tests stay byte-for-byte green, confirming no behavioral drift. Only
  the Go method arm sets it. Minimal, idiomatic, and the right seam for a non-lexical parent.

**No reachable unwrap/expect/panic** on any library path in the new/changed src: `recognize_go`,
`go_receiver_type_name`, the `type_declaration` drill, `build_chunk`, `field_text`, `node_text`,
`extend_to_line_end` all use `?`/`Option`/`find`. `unwrap`/`expect` appear only in `#[cfg(test)]`
code (integration + the new mod-tests unit test) — acceptable.

**Alignment:** `queries/go.scm` matches `project_plan.md` §5.3 verbatim (capture names, field
names, node kinds: `function_declaration`/`method_declaration` with `receiver:`/
`type_declaration`→`type_spec`→`struct_type`). Grammar = `tree_sitter_go::LANGUAGE`. No new
`SymbolType`/`Language` variant (reuses `Struct`). `Cargo.toml` unchanged by this slice —
`tree-sitter-go = "0.23"` was committed at M0; no `streaming-iterator`. `Parser::new` validates
the Go query against the grammar up front (construction-time).

**Repointed test (`all_v01_languages_parse_supported`) — legitimate re-alignment, NOT a
weakening:** it now asserts real Ok-parsing AND `!has_error()` for all three v0.1 languages on
inline snippets (Python/TS/Go) — strictly stronger than the old single "Go unsupported" claim.
No `#[ignore]`, no deleted coverage; `parser_tests.rs` stays at 14. The `UnsupportedLanguage`
typed-error contract is preserved as the new focused unit test
`unsupported_language_error_displays_typed_message` in `src/parser/mod.rs` (`matches!` on the
variant + exact `Display` "unsupported language for parsing: go"). Coverage moved, not lost.

**Test adequacy:** the 5 Go tests genuinely exercise the matrix with real-value asserts — exact
chunk_text where it bites (off-by-one + leading-tab guard), receiver→`Some("Server")` parent,
`SymbolType::Struct` typing, `chunks.len() == 1` package/import exclusion, D2 threshold + span
invariant over survivors. Fixtures are LF, minimal, gofmt-tabbed, deterministic.

**Finding (non-blocking):**
- minor (hygiene/DoD) — `src/parser/CLAUDE.md` + `docs/TODO.md:242` — the per-slice DoD and the
  repo golden rule require the module CLAUDE.md and `docs/TODO.md` Phase 9 to be updated in the
  SAME change. `tests/CLAUDE.md` has the Go fixture rows, but `src/parser/CLAUDE.md` still reads
  "Go lands at M9.2" (future tense) with no M9.2 GREEN subsection (grammar `tree_sitter_go::LANGUAGE`,
  §5.3 captures, `recognize_go`/`go_receiver_type_name` receiver-strip rule, struct span-node
  choice, package/import exclusion, no-interfaces scope) and `go.rs` is absent from the Files
  list; `docs/TODO.md:242` M9.2 line is still `- [ ]` unchecked. Fix: add a Go subsection to
  `src/parser/CLAUDE.md` and check off M9.2 in `docs/TODO.md`. Same handling as the M9.1 finding —
  fold into the manager's integration commit, but it must land WITH this slice's commit.

**Verdict: APPROVE.** Correctness, the `parent_override` shared-code change, alignment, idiomatic
Rust (no reachable unwrap/expect/panic), the honest test repoint, and test adequacy all hold;
all four gates green at 179/0 on Rust 1.85. The only must-handle item is folding the
CLAUDE.md/TODO doc updates into the slice commit (minor, process), routed to the manager.

### M9.2 — Go (2026-06-12) — DONE ✅
- **Aligned:** code matches §5.3 Go capture contract; `recognize_go` reuses the shared
  span/line/D2 machinery; reviewer APPROVE (0 blocking). All three v0.1 languages now parse `Ok`.
- **Blocker handled (manager call):** the stale M3 `unsupported_language_expects_typed_error` test
  (which used `Language::Go` as the "unsupported" sentinel) became structurally false once Go was
  wired. Directed the test-lead to **repoint, not weaken** — `all_v01_languages_parse_supported`
  now asserts the real M9 exit guarantee (Python/TS/Go all parse Ok, no ERROR), and the
  `UnsupportedLanguage` typed-error contract is preserved at unit level
  (`unsupported_language_error_displays_typed_message`). Coverage strengthened, not lost. Rationale
  recorded in the RED-fixup note. No project_plan change needed (the variant stays as forward-compat
  API for a future language wired before its grammar lands).
- **Decisions recorded** (in `src/parser/CLAUDE.md`): `tree_sitter_go::LANGUAGE` grammar; receiver
  type → `parent_symbol` via `Definition.parent_override` (pointer-stripped); struct span node =
  `type_declaration`; package/import/interface not emitted; no enum/dep change (`tree-sitter-go
  0.23` already at M0, MSRV stays 1.85). No deviation to escalate.
- **Reviewer finding (docs hygiene):** resolved — `src/parser/CLAUDE.md` Go subsection + Files line
  + status, and `docs/TODO.md` M9.2, updated in the slice commit.
- **TODO:** M9.2 checked off. **Slice marked DONE.**
- **Commit:** see M9.2 commit hash below; working tree clean; 179 tests green; all four gates clean
  on Rust 1.85.
