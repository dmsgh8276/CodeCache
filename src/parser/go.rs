//! Go `LanguageConfig`: the tree-sitter-go grammar plus the `.scm` extraction
//! queries (project_plan.md §5.3).
//!
//! The query strings are embedded from `queries/go.scm` and validated against the
//! grammar in [`super::Parser::new`]; see that file's header for why extraction
//! walks the tree directly rather than driving the queries through `QueryCursor`.

use super::LanguageConfig;

/// The Go extraction queries (function/method/struct), §5.3.
pub const GO_QUERIES: &str = include_str!("queries/go.scm");

/// Build the Go [`LanguageConfig`]: tree-sitter-go grammar + queries.
pub fn config() -> LanguageConfig {
    LanguageConfig {
        grammar: tree_sitter_go::LANGUAGE.into(),
        queries: GO_QUERIES,
    }
}
