use crate::doctor;
use crate::flp;
use crate::rebuild;
use crate::separate;
use crate::tools;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const SKILL: &str = include_str!("../skills/unbake/SKILL.md");

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
            "serverInfo": { "name": "unbake", "version": env!("CARGO_PKG_VERSION") },
            "instructions": instructions()
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

// the skill's workflow without its front matter, sent to agents when they connect
pub fn instructions() -> &'static str {
    let text = SKILL.trim_start();
    text.strip_prefix("---")
        .and_then(|rest| {
            rest.split_once(
                "
---",
            )
        })
        .map_or(text, |(_, body)| body.trim_start())
}

// a json-rpc error answer
fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// the tools an agent can call and the arguments each one takes
fn tools() -> Value {
    let file = json!({ "type": "string", "description": "path to an audio file" });
    let folder = json!({ "type": "string", "description": "a rebuild folder made by transcribe" });
    let name = json!({ "type": "string", "description": "drums, bass, melody (the chosen candidate), chords, or a melody candidate's name" });
    let from_bar =
        json!({ "type": "integer", "description": "first bar, counting from 1, default 1" });
    json!([
        {
            "name": "doctor",
            "description": "Check that ffmpeg, demucs, basic-pitch and the GPU are found. Each check has a status (ok, warning or missing), what it is needed for, and the command that fixes it. Run it first when another tool fails.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "analyze",
            "description": "Find the tempo, first bar, number of bars and key of a song, with how sure each is and what else could fit.",
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
                    "template": { "type": "string", "description": "your own FL Studio template, parts go to channels named Kick, Snare, Hat, Bass, Lead and Pad; leave out to use the built in one" },
                    "sounds": { "type": "string", "description": "folder with the user's own sounds named kick, snare, hat, bass, lead, pad" },
                    "fl_folder": { "type": "string", "description": "windows folder the rebuild will be copied to" },
                    "match_sounds": { "type": "boolean", "description": "tune the synth sounds to the stems, default true" },
                    "simple_chords": { "type": "boolean", "description": "write the chords as clean triads that follow the key, easier to edit but less exact, default false" },
                    "loops": { "type": "boolean", "description": "arrange each part as up to 4 reused 4 bar loops instead of one pattern copying the whole song, default false" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "transcribe",
            "description": "Listen to a song (the slow step: stems, tempo, key, drums, bass and notes) and save it as a session in the rebuild folder. Returns how sure each finding is and the melody candidates. Show the user the candidates and unknowns, let them choose, then call write_flp.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "out": { "type": "string", "description": "rebuild folder, also the FL Studio project folder" },
                    "stems": { "type": "string", "description": "folder with stems to use instead of running demucs" },
                    "sounds": { "type": "string", "description": "folder with the user's own drum sounds named kick, snare, hat, used to find the hits" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "melody_candidates",
            "description": "List the melody candidates of a transcribed folder: how each was picked, how many notes it shares with the first, its range, how clearly it stood out and its unknowns. One is marked chosen.",
            "inputSchema": { "type": "object", "properties": { "folder": folder }, "required": ["folder"] }
        },
        {
            "name": "choose_melody",
            "description": "Choose which melody candidate the project uses. Ask the user first when the candidates differ; the other stem's notes outside the chosen line become the chords.",
            "inputSchema": {
                "type": "object",
                "properties": { "folder": folder, "candidate": { "type": "string", "description": "a candidate's name, like loudest, highest or vocals" } },
                "required": ["folder", "candidate"]
            }
        },
        {
            "name": "inspect_part",
            "description": "List the notes of a part or candidate in some bars, with bar and step counted from 1 like FL Studio, note name, length in 1/16 steps and velocity, plus its finding.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": folder,
                    "name": name,
                    "from_bar": from_bar,
                    "bars": { "type": "integer", "description": "how many bars, default 4" }
                },
                "required": ["folder", "name"]
            }
        },
        {
            "name": "piano_roll",
            "description": "Draw a piano roll image of one to four parts or candidates over each other in different colours (orange, cyan, pink, green), time across and pitch up, bars as bright lines. Use it to compare melody candidates and to show the user. Also saved as a png in the folder's extras.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": folder,
                    "names": { "type": "array", "items": name, "description": "the parts or candidates to draw, first one in orange" },
                    "from_bar": from_bar,
                    "bars": { "type": "integer", "description": "how many bars, default 8" }
                },
                "required": ["folder", "names"]
            }
        },
        {
            "name": "write_flp",
            "description": "Write the FL Studio project of a transcribed folder with the chosen melody: patterns, sounds cut from the stems, the stems as muted clips, midi and a report. Returns the findings and where the project is.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": folder,
                    "template": { "type": "string", "description": "your own FL Studio template, parts go to channels named Kick, Snare, Hat, Bass, Lead and Pad; leave out to use the built in one" },
                    "sounds": { "type": "string", "description": "folder with the user's own sounds named kick, snare, hat, bass, lead, pad" },
                    "fl_folder": { "type": "string", "description": "windows folder the rebuild will be copied to" },
                    "match_sounds": { "type": "boolean", "description": "tune the synth sounds to the stems, default true" },
                    "simple_chords": { "type": "boolean", "description": "write the chords as clean triads that follow the key, easier to edit but less exact, default false" },
                    "loops": { "type": "boolean", "description": "arrange each part as up to 4 reused 4 bar loops instead of one pattern copying the whole song, default false" }
                },
                "required": ["folder"]
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

// runs a tool and wraps its answer, or its error, for the agent
fn call_tool(params: &Value) -> Value {
    let args = &params["arguments"];
    let outcome = match params["name"].as_str().unwrap_or("") {
        "analyze" => path_arg(args, "file")
            .and_then(|file| rebuild::analyze_song(&file))
            .and_then(text),
        "split" => split(args).and_then(text),
        "doctor" => text(doctor::run()),
        "rebuild" => rebuild_tool(args).and_then(text),
        "transcribe" => transcribe_tool(args).and_then(text),
        "melody_candidates" => path_arg(args, "folder")
            .and_then(|folder| tools::candidates(&folder))
            .and_then(text),
        "choose_melody" => choose_tool(args).and_then(text),
        "inspect_part" => inspect_tool(args).and_then(text),
        "piano_roll" => piano_roll_tool(args),
        "write_flp" => write_tool(args).and_then(text),
        "flp_info" => path_arg(args, "file")
            .and_then(|file| flp::events::read(&file))
            .and_then(|project| text(flp::info::summarize(&project))),
        other => Err(format!("unknown tool {}", other)),
    };
    match outcome {
        Ok(content) => json!({ "content": content, "isError": false }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

// an answer as one block of indented json text
fn text(value: impl serde::Serialize) -> Result<Vec<Value>, String> {
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    Ok(vec![json!({ "type": "text", "text": text })])
}

// transcribes a song into a session and returns its findings and melody candidates
fn transcribe_tool(args: &Value) -> Result<Value, String> {
    let mut options = song_options(args)?;
    options.match_sounds = false;
    let folder = rebuild::output_dir(&options);
    options.out = Some(folder.clone());
    let session = rebuild::transcribe(&options)?;
    Ok(json!({
        "folder": folder.display().to_string(),
        "tempo_bpm": session.grid.bpm,
        "key": session.key.name(),
        "bars": session.bars,
        "findings": session.findings,
        "melody_candidates": tools::candidates(&folder)?,
        "warnings": session.warnings,
    }))
}

// chooses a melody candidate and returns the candidates with the new choice marked
fn choose_tool(args: &Value) -> Result<Value, String> {
    let folder = path_arg(args, "folder")?;
    let name = args["candidate"]
        .as_str()
        .ok_or("missing argument candidate".to_string())?;
    rebuild::choose_melody(&folder, name)?;
    serde_json::to_value(tools::candidates(&folder)?).map_err(|e| e.to_string())
}

// lists a part's notes in some bars
fn inspect_tool(args: &Value) -> Result<tools::PartView, String> {
    let folder = path_arg(args, "folder")?;
    let name = args["name"]
        .as_str()
        .ok_or("missing argument name".to_string())?;
    tools::inspect(
        &folder,
        name,
        number(args, "from_bar", 1),
        number(args, "bars", 4),
    )
}

// draws a piano roll and returns it as an image with a line explaining its colours and scale
fn piano_roll_tool(args: &Value) -> Result<Vec<Value>, String> {
    let folder = path_arg(args, "folder")?;
    let names: Vec<String> = args["names"]
        .as_array()
        .ok_or("missing argument names".to_string())?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let roll = tools::piano_roll(
        &folder,
        &names,
        number(args, "from_bar", 1),
        number(args, "bars", 8),
    )?;
    Ok(vec![
        json!({ "type": "image", "data": base64(&roll.png), "mimeType": "image/png" }),
        json!({ "type": "text", "text": format!("{}. {}. Saved as {}", roll.legend.join(", "), roll.scale, roll.path) }),
    ])
}

// writes the project of a transcribed folder and returns a short summary
fn write_tool(args: &Value) -> Result<Value, String> {
    let folder = path_arg(args, "folder")?;
    let mut options = project_options(args, folder.clone());
    options.out = Some(folder.clone());
    let report = tools::write(&folder, options)?;
    Ok(json!({
        "flp": report.flp,
        "report": folder.join("report.html").display().to_string(),
        "sounds": report.sounds,
        "findings": report.findings,
        "warnings": report.warnings,
    }))
}

// the rebuild options from a song's arguments
fn song_options(args: &Value) -> Result<rebuild::Options, String> {
    let mut options = project_options(args, path_arg(args, "file")?);
    options.out = optional_path(args, "out");
    options.stems = optional_path(args, "stems");
    Ok(options)
}

// the rebuild options from the project arguments
fn project_options(args: &Value, input: PathBuf) -> rebuild::Options {
    rebuild::Options {
        input,
        out: None,
        stems: None,
        template: optional_path(args, "template"),
        sounds: optional_path(args, "sounds"),
        fl_folder: args["fl_folder"].as_str().map(|s| s.to_string()),
        match_sounds: args["match_sounds"].as_bool().unwrap_or(true),
        simple_chords: args["simple_chords"].as_bool().unwrap_or(false),
        loops: args["loops"].as_bool().unwrap_or(false),
    }
}

// a whole number argument, or a default when it is missing
fn number(args: &Value, name: &str, default: u32) -> u32 {
    args[name].as_u64().map_or(default, |n| n as u32)
}

// bytes as base64 text, the way images travel inside json
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// separates a song and says where the stems went
fn split(args: &Value) -> Result<Value, String> {
    let file = path_arg(args, "file")?;
    let options = rebuild::Options {
        input: file.clone(),
        out: None,
        stems: None,
        template: None,
        sounds: None,
        fl_folder: None,
        match_sounds: false,
        simple_chords: false,
        loops: false,
    };
    let dir = optional_path(args, "out").unwrap_or_else(|| rebuild::output_dir(&options));
    separate::split(&file, &dir)?;
    Ok(json!({ "stems": dir.display().to_string() }))
}

// rebuilds a song and returns the report
fn rebuild_tool(args: &Value) -> Result<Value, String> {
    let options = song_options(args)?;
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

    // base64 matches the examples from its specification, with padding
    #[test]
    fn base64_examples() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    // the handshake answers with the server name and the workflow, and a notification gets no answer
    #[test]
    fn handshake() {
        let reply = handle(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18" }
        }))
        .unwrap();
        assert_eq!(reply["result"]["serverInfo"]["name"], "unbake");
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        let text = reply["result"]["instructions"].as_str().unwrap();
        assert!(text.starts_with("# unbake"));
        assert!(text.contains("choose_melody"));
        assert!(
            handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none()
        );
    }

    // the tool list has eleven tools and an unknown tool comes back as an error result
    #[test]
    fn lists_tools() {
        let reply = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert_eq!(reply["result"]["tools"].as_array().unwrap().len(), 11);
        let call = handle(&json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "nope", "arguments": {} }
        }))
        .unwrap();
        assert_eq!(call["result"]["isError"], true);
    }
}
