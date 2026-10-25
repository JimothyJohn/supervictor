//! stdio entrypoint: one JSON-RPC message per line in, one per line out.
//!
//! stdout carries nothing but MCP messages (the transport rule); every
//! diagnostic goes to stderr, where MCP clients collect server logs.

use std::io::Write;

use tokio::io::{AsyncBufReadExt, BufReader};

use supervictor_mcp::config::Config;
use supervictor_mcp::protocol::{handle_line, ServerInfo};
use supervictor_mcp::tools::FleetTools;

const INFO: ServerInfo = ServerInfo {
    name: "supervictor",
    version: env!("CARGO_PKG_VERSION"),
    instructions: "Read-only status of a supervictor fleet: the backend endpoint and the edge \
                   devices (boards) posting to it. Start with supervictor_backend_status when \
                   unsure the endpoint is reachable; supervictor_fleet_status lists devices \
                   worst-first. Every tool accepts an `endpoint` URL to ask a different \
                   deployment.",
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // rustls needs exactly one process-wide crypto provider; `Client` builds
    // its config with ring explicitly, this just settles the default.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("supervictor-mcp: {e}");
            std::process::exit(2);
        }
    };
    eprintln!(
        "supervictor-mcp {}: endpoint {} (admin cert {})",
        env!("CARGO_PKG_VERSION"),
        config.target.display(),
        config.cert.display()
    );
    let tools = FleetTools::new(config);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let stdout = std::io::stdout();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break, // client closed stdin: shut down
            Err(e) => {
                eprintln!("supervictor-mcp: stdin: {e}");
                break;
            }
        };
        if let Some(reply) = handle_line(&line, &INFO, &tools).await {
            // serde_json never emits a raw newline inside a string, so one
            // message is one line.
            let mut line = serde_json::to_vec(&reply).expect("a Value always serializes");
            line.push(b'\n');
            let mut out = stdout.lock();
            if out.write_all(&line).and_then(|()| out.flush()).is_err() {
                break; // client went away
            }
        }
    }
}
