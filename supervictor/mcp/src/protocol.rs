//! JSON-RPC 2.0 framing and the MCP lifecycle for a tools-only server.
//!
//! Newline-delimited JSON on stdio (MCP spec, "stdio" transport): one
//! request or notification per line in, one response per request out,
//! nothing else on stdout. Methods served: `initialize`, `ping`,
//! `tools/list`, `tools/call`; `notifications/initialized` and
//! `notifications/cancelled` are accepted silently. Anything else is a
//! JSON-RPC "method not found".
//!
//! The tool layer is behind the [`Tools`] trait so this module is tested
//! with a stub and the real tools are tested against a real endpoint.

use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

/// Protocol revisions this server speaks, newest first. The server echoes
/// the client's version when it is one of these and otherwise offers the
/// newest, as the lifecycle spec requires.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
/// Request object is not valid JSON-RPC.
pub const INVALID_REQUEST: i64 = -32600;
/// Unknown method.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// Unknown tool, or arguments that are not an object.
pub const INVALID_PARAMS: i64 = -32602;

/// A tool execution failure, reported inside the result with `isError`
/// (the spec's "tool execution error"), so the model reads the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

impl<T: Into<String>> From<T> for ToolError {
    fn from(s: T) -> Self {
        ToolError(s.into())
    }
}

/// What a tool returns: a structured value plus the text the model reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Goes in `structuredContent`.
    pub structured: Value,
    /// Goes in `content[0].text`; when `None` the structured value is
    /// pretty-printed instead.
    pub text: Option<String>,
}

/// A boxed tool-call future (the trait is object-safe without async fns).
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// The tool set the protocol layer serves.
pub trait Tools: Send + Sync {
    /// `tools/list` entries: name, description, inputSchema, annotations.
    fn definitions(&self) -> Vec<Value>;
    /// Run one tool. `args` is always a JSON object.
    fn call<'a>(&'a self, name: &'a str, args: Value) -> ToolFuture<'a>;
}

/// Server identity for `initialize`.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// Implementation name.
    pub name: &'static str,
    /// Implementation version.
    pub version: &'static str,
    /// Instructions the client shows the model.
    pub instructions: &'static str,
}

/// Handle one line of stdin. `None` means nothing goes back (a
/// notification, or a response to a request we never sent).
pub async fn handle_line(line: &str, info: &ServerInfo, tools: &dyn Tools) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let message: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                PARSE_ERROR,
                &format!("parse error: {e}"),
            ))
        }
    };
    let Some(object) = message.as_object() else {
        return Some(error_response(
            Value::Null,
            INVALID_REQUEST,
            "a JSON-RPC message is an object",
        ));
    };
    let id = object.get("id").cloned();
    let method = object.get("method").and_then(Value::as_str);
    let params = object.get("params").cloned().unwrap_or(Value::Null);

    match (method, id) {
        // A response to a server→client request. We send none, so ignore.
        (None, Some(_)) if object.contains_key("result") || object.contains_key("error") => None,
        (None, id) => Some(error_response(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "missing method",
        )),
        // Notifications never get a reply, known or not.
        (Some(_), None) => None,
        (Some(method), Some(id)) => Some(match method {
            "initialize" => success(id, initialize_result(&params, info)),
            "ping" => success(id, json!({})),
            "tools/list" => success(id, json!({ "tools": tools.definitions() })),
            "tools/call" => call_tool(id, &params, tools).await,
            other => error_response(id, METHOD_NOT_FOUND, &format!("method not found: {other}")),
        }),
    }
}

fn initialize_result(params: &Value, info: &ServerInfo) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = match requested {
        Some(v) if PROTOCOL_VERSIONS.contains(&v) => v,
        _ => PROTOCOL_VERSIONS[0],
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": info.name, "version": info.version },
        "instructions": info.instructions,
    })
}

async fn call_tool(id: Value, params: &Value, tools: &dyn Tools) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error_response(id, INVALID_PARAMS, "tools/call needs a string `name`");
    };
    let args = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(Value::Object(_)) => params["arguments"].clone(),
        Some(_) => {
            return error_response(id, INVALID_PARAMS, "`arguments` must be an object");
        }
    };
    if !tools
        .definitions()
        .iter()
        .any(|d| d.get("name").and_then(Value::as_str) == Some(name))
    {
        return error_response(id, INVALID_PARAMS, &format!("unknown tool: {name}"));
    }
    match tools.call(name, args).await {
        Ok(output) => {
            let text = output.text.unwrap_or_else(|| {
                serde_json::to_string_pretty(&output.structured).unwrap_or_default()
            });
            success(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": output.structured,
                    "isError": false,
                }),
            )
        }
        Err(ToolError(message)) => success(
            id,
            json!({
                "content": [{ "type": "text", "text": message }],
                "isError": true,
            }),
        ),
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub;

    impl Tools for Stub {
        fn definitions(&self) -> Vec<Value> {
            vec![
                json!({ "name": "echo", "description": "echo", "inputSchema": { "type": "object" } }),
            ]
        }
        fn call<'a>(&'a self, name: &'a str, args: Value) -> ToolFuture<'a> {
            Box::pin(async move {
                match name {
                    "echo" if args.get("fail").is_some() => Err("told to fail".into()),
                    "echo" => Ok(ToolOutput {
                        structured: args,
                        text: Some("echoed".into()),
                    }),
                    _ => Err("not here".into()),
                }
            })
        }
    }

    const INFO: ServerInfo = ServerInfo {
        name: "test",
        version: "0",
        instructions: "none",
    };

    async fn run(line: &str) -> Option<Value> {
        handle_line(line, &INFO, &Stub).await
    }

    #[tokio::test]
    async fn initialize_echoes_supported_version_and_offers_newest_otherwise() {
        let r = run(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#)
            .await
            .unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], false);
        assert_eq!(r["result"]["serverInfo"]["name"], "test");

        let r = run(r#"{"jsonrpc":"2.0","id":"x","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#)
            .await
            .unwrap();
        assert_eq!(r["id"], "x");
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
        let r = run(r#"{"jsonrpc":"2.0","id":2,"method":"initialize"}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[tokio::test]
    async fn notifications_and_stray_responses_are_silent() {
        assert!(
            run(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .await
                .is_none()
        );
        assert!(run(
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#
        )
        .await
        .is_none());
        assert!(run(r#"{"jsonrpc":"2.0","method":"anything/unknown"}"#)
            .await
            .is_none());
        assert!(run(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
            .await
            .is_none());
        assert!(run("").await.is_none());
        assert!(run("   \t ").await.is_none());
    }

    #[tokio::test]
    async fn malformed_input_gets_jsonrpc_errors() {
        let r = run("{not json").await.unwrap();
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        assert_eq!(r["id"], Value::Null);
        let r = run("[1,2]").await.unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        let r = run(r#"{"jsonrpc":"2.0","id":3}"#).await.unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(r["id"], 3);
        let r = run(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn ping_and_list() {
        let r = run(r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#)
            .await
            .unwrap();
        assert_eq!(r["result"], json!({}));
        let r = run(r#"{"jsonrpc":"2.0","id":6,"method":"tools/list"}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["tools"][0]["name"], "echo");
    }

    #[tokio::test]
    async fn call_success_error_and_validation() {
        let r = run(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"echo","arguments":{"a":1}}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(r["result"]["structuredContent"], json!({"a":1}));
        assert_eq!(r["result"]["content"][0]["text"], "echoed");

        // No arguments at all is an empty object.
        let r = run(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"echo"}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["structuredContent"], json!({}));

        let r = run(r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"echo","arguments":{"fail":1}}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "told to fail");
        assert!(r["result"].get("structuredContent").is_none());

        let r = run(r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"nope"}}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        assert!(r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
        let r = run(r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"echo","arguments":[1]}}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = run(r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{}}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
    }
}
