//! `codecache serve` handler (M7.3 stub).
//!
//! The MCP server lands at M8. For now this prints a clean "not yet" notice and succeeds — it must
//! not panic or crash (pinned by `serve_is_a_clean_stub`). M8 owns the final transport + exit
//! semantics.

use anyhow::Result;

/// Print a clean stub notice; the real MCP server is M8.
pub fn run() -> Result<()> {
    println!("serve: the MCP server is not implemented yet (M8).");
    Ok(())
}
