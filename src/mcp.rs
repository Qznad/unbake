use crate::flp;
use crate::rebuild;
use crate::separate;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

// answers model context protocol requests, one json message per line on stdin, until stdin closes
pub fn serve() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle(&request),
            Err(e) => Some(error_reply(
                Value::Null,
                -32700,
                &format!("parse error: {}", e),
            )),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{}", reply).map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// answers one request, notifications have no id and get no answer
fn handle(request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "unbake", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => call_tool(&request["params"]),
        _ => {
            return Some(error_reply(
                id,
                -32601,
                &format!("unknown method {}", method),
            ));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

// a json-rpc error answer
fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// the tools an agent can call and the arguments each one takes
fn tools() -> Value {
    let file = json!({ "type": "string", "description": "path to an audio file" });
    json!([
        {
            "name": "analyze",
            "description": "Find the tempo, first bar, number of bars and key of a song.",
            "inputSchema": { "type": "object", "properties": { "file": file }, "required": ["file"] }
        },
        {
            "name": "split",
            "description": "Separate a song into drums, bass, other and vocals stems with demucs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "out": { "type": "string", "description": "folder for the stems" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "rebuild",
            "description": "Rebuild a song as stems, midi, an optional FL Studio project, a synth preview and a scored report. Returns the report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "out": { "type": "string", "description": "output folder" },
                    "stems": { "type": "string", "description": "folder with stems to use instead of running demucs" },
                    "template": { "type": "string", "description": "FL Studio project with channels named Kick, Snare, Hat, Bass, Lead and Pad" },
                    "match_sounds": { "type": "boolean", "description": "tune the synth sounds to the stems, default true" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "flp_info",
            "description": "List the version, tempo, channels, patterns and playlist size of an FL Studio project.",
            "inputSchema": {
                "type": "object",
                "properties": { "file": { "type": "string", "description": "path to an .flp file" } },
                "required": ["file"]
            }
        }
    ])
}

// runs a tool and wraps its answer, or its error, as text for the agent
fn call_tool(params: &Value) -> Value {
    let args = &params["arguments"];
    let outcome = match params["name"].as_str().unwrap_or("") {
        "analyze" => path_arg(args, "file")
            .and_then(|file| rebuild::analyze_song(&file))
            .and_then(|info| serde_json::to_value(info).map_err(|e| e.to_string())),
        "split" => split(args),
        "rebuild" => rebuild_tool(args),
        "flp_info" => path_arg(args, "file")
            .and_then(|file| flp::events::read(&file))
            .and_then(|project| {
                serde_json::to_value(flp::info::summarize(&project)).map_err(|e| e.to_string())
            }),
        other => Err(format!("unknown tool {}", other)),
    };
    match outcome {
        Ok(value) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
            "isError": false
        }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

// separates a song and says where the stems went
fn split(args: &Value) -> Result<Value, String> {
    let file = path_arg(args, "file")?;
    let options = rebuild::Options {
        input: file.clone(),
        out: None,
        stems: None,
        template: None,
        match_sounds: false,
    };
    let dir =
        optional_path(args, "out").unwrap_or_else(|| rebuild::output_dir(&options).join("stems"));
    separate::split(&file, &dir)?;
    Ok(json!({ "stems": dir.display().to_string() }))
}

// rebuilds a song and returns the report
fn rebuild_tool(args: &Value) -> Result<Value, String> {
    let options = rebuild::Options {
        input: path_arg(args, "file")?,
        out: optional_path(args, "out"),
        stems: optional_path(args, "stems"),
        template: optional_path(args, "template"),
        match_sounds: args["match_sounds"].as_bool().unwrap_or(true),
    };
    let report = rebuild::run(&options)?;
    let mut value = serde_json::to_value(report).map_err(|e| e.to_string())?;
    value["output_folder"] = json!(rebuild::output_dir(&options).display().to_string());
    Ok(value)
}

// reads a required path argument
fn path_arg(args: &Value, name: &str) -> Result<PathBuf, String> {
    optional_path(args, name).ok_or(format!("missing argument {}", name))
}

// reads a path argument that may be left out
fn optional_path(args: &Value, name: &str) -> Option<PathBuf> {
    args[name].as_str().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    // the handshake answers with the server name and a notification gets no answer
    #[test]
    fn handshake() {
        let reply = handle(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18" }
        }))
        .unwrap();
        assert_eq!(reply["result"]["serverInfo"]["name"], "unbake");
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert!(
            handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none()
        );
    }

    // the tool list has four tools and an unknown tool comes back as an error result
    #[test]
    fn lists_tools() {
        let reply = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert_eq!(reply["result"]["tools"].as_array().unwrap().len(), 4);
        let call = handle(&json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "nope", "arguments": {} }
        }))
        .unwrap();
        assert_eq!(call["result"]["isError"], true);
    }
}
