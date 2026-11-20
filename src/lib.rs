//! agent-context: shared context across agent tools and sessions.
//!
//! Any MCP client appends small records to `raw/`; a maintainer agent compiles
//! them into topic pages under `wiki/`. Everything is plain Markdown in a git
//! repository (the "vault"), so humans can read and edit it directly.
//! Alongside it, `board/` is a machine-local blackboard of short-lived working
//! state that agent tools overwrite by key.

pub mod assets;
pub mod board;
pub mod git;
pub mod lint;
pub mod mcp;
pub mod search;
pub mod vault;
pub mod web;

pub use vault::Vault;
