//! supervictor-mcp: a Model Context Protocol server that answers "how is the
//! fleet, how is the backend" from the endpoint's admin API.
//!
//! Layers, each testable on its own:
//!
//! - [`config`]: where the endpoint is and which admin identity to present,
//!   from `SUPERVICTOR_*` environment variables.
//! - [`http`]: HTTP/1.1 request text and response parsing (Content-Length,
//!   chunked, read-to-EOF), no I/O.
//! - [`client`]: one mTLS connection per request over tokio-rustls, with
//!   error messages that say what to fix.
//! - [`tools`]: the MCP tools, each one HTTPS GET plus shaping and filtering.
//! - [`protocol`]: JSON-RPC 2.0 framing and the MCP lifecycle, independent
//!   of what the tools do.

pub mod client;
pub mod config;
pub mod http;
pub mod protocol;
pub mod tools;
