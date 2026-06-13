; Go extraction queries (project_plan.md §5.3).
;
; These S-expression queries are the documented contract for what the Go
; `LanguageConfig` extracts. They are compiled and validated against the grammar
; in `Parser::new` (a malformed query is a construction-time error), which proves
; the capture/field/node names below match the tree-sitter-go grammar.
;
; NOTE (extraction seam): like Python and TypeScript, extraction walks the tree
; with a `TreeCursor` rather than driving these queries through `QueryCursor`. The
; walk gives direct ancestor access and, for Go, lets the receiver-type drill
; (`method_declaration` → `receiver:` → `parameter_declaration` → `type:`) decide
; a method's `parent_symbol`. It also avoids the external `streaming-iterator`
; crate that tree-sitter 0.24's `QueryCursor::matches` requires (keep Cargo.toml
; lean). The queries are kept here, validated, and ready for richer query-driven
; enrichment (D3) in M4.

; Function declarations (name + params + body).
(function_declaration
  name: (identifier) @function.name
  parameters: (parameter_list) @function.params
  body: (block) @function.body) @function.definition

; Method declarations (the `func` form WITH a receiver) → typed as Method; the
; receiver type name becomes `parent_symbol`.
(method_declaration
  receiver: (parameter_list) @method.receiver
  name: (field_identifier) @method.name
  parameters: (parameter_list) @method.params
  body: (block) @method.body) @method.definition

; Struct definitions: a `type_declaration` whose `type_spec`'s type is a struct.
(type_declaration
  (type_spec
    name: (type_identifier) @struct.name
    type: (struct_type) @struct.body)) @struct.definition
