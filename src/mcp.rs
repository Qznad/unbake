use crate::compose;
use crate::doctor;
use crate::flp;
use crate::rebuild;
use crate::separate;
use crate::tools;
use crate::views;
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
            "serverInfo": { "name": "unbake", "version": doctor::VERSION },
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
    let name = json!({ "type": "string", "description": "drums, bass, melody (the chosen candidate), chords (the held chords), leftover (the other-stem notes that are neither), or a melody candidate's name" });
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
                    "simple_chords": { "type": "boolean", "description": "write the chords as plain three-note triads, without the extra notes heard in them, default false" },
                    "raw_melody": { "type": "boolean", "description": "keep every melody note heard instead of tidying it into one playable line (three octaves, no echoes, ghost notes or overlaps), default false" },
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
                    "simple_chords": { "type": "boolean", "description": "write the chords as plain three-note triads, without the extra notes heard in them, default false" },
                    "raw_melody": { "type": "boolean", "description": "keep every melody note heard instead of tidying it into one playable line (three octaves, no echoes, ghost notes or overlaps), default false" },
                    "loops": { "type": "boolean", "description": "arrange each part as up to 4 reused 4 bar loops instead of one pattern copying the whole song, default false" }
                },
                "required": ["folder"]
            }
        },
        {
            "name": "song_overview",
            "description": "One line per bar of a transcribed folder: the section starting there, how many kick, snare, hat, bass and other-stem notes it has, and the chord heard (like Am9, from the bass note and the notes above it). Use it to find where parts come in and drop out, and where the main section is.",
            "inputSchema": { "type": "object", "properties": { "folder": folder }, "required": ["folder"] }
        },
        {
            "name": "part_grid",
            "description": "A part of a transcribed folder as 16 steps per bar. Drums come as kick, snare and hat hit strings (X loud, x normal, o soft, g ghost, . rest), the same format compose_loop's grids take. Pitched parts (bass, melody, chords, leftover or a melody candidate) come as the note starting on each step, the loudest when several do.",
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
            "name": "song_parts",
            "description": "Cut a part of a transcribed folder (melody, bass, chords, leftover, drums or a melody candidate) into stretches of some bars from a first bar, usually the song's chord cycle from song_overview, and give each stretch a letter: stretches sharing at least half their notes (same step and note name, so octave slips still match) get the same letter when they share at least 30% of their notes (same step and note name, so octave slips still match), since a noisy repeat of a melody still shares that much. For each stretch: its letter, note count, how close it is to the first stretch of its letter, and the closest other letter. For each letter: the bars it plays, its note count and, when asked for with letter, its notes (bar and step from 1 inside the stretch), ready to write as that letter's pattern. Call it once without letter to see the letters, then once per letter you want to write. A letter heard several times gives the notes a third of its repeats agree on (at least two), one note per step for a melody or bassline, which is the cleanest. A letter heard once that shares 15% or more with a repeated letter is a variation of it (variation_of): its own notes with its empty steps filled from that letter. A letter heard once with no close relative is raw; in a quieter section it may be another instrument while the lead drops out, so ask the user to check it against the Other stem. Use it so every different melody, bassline or chord part of the song gets its own pattern instead of one repeated loop. The melody is tidied first, as write_flp does. The bass is cleaned first: every note that isn't its bar's root or fifth moves to the root, then it is tidied, so the letters hold the bassline a producer would write. Only melodies get gaps filled from the letter they vary, since a quiet bar in the isolated bass stem is real silence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": folder,
                    "name": name,
                    "from_bar": from_bar,
                    "bars": { "type": "integer", "description": "bars per stretch, default 8" },
                    "to_bar": { "type": "integer", "description": "last bar, the end of the song by default" },
                    "letter": { "type": "string", "description": "give the notes of this letter only; without it the answer holds the letters, their bars and note counts but no notes" }
                },
                "required": ["folder", "name"]
            }
        },
        {
            "name": "harmony",
            "description": "For each bar or span of bars of a transcribed folder, the note names sounding in the other stem as percentages (weighted by length and loudness), the bass note, and a chord name with its extensions (Am9, Gm11, Bbmaj9) built from them. Richer than the chords finding, which only names triads. The chord is named from the bass note, so when the bass walks under a held chord the name follows the bass (Cm9 over an Eb bass reads Ebmaj13, the same notes); check the tones. Use it to voice the chords of a loop.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": folder,
                    "from_bar": from_bar,
                    "bars": { "type": "integer", "description": "how many bars, the rest of the song by default" },
                    "per": { "type": "integer", "description": "bars per chord; 0, the default, starts a new chord where the bass note changes" }
                },
                "required": ["folder"]
            }
        },
        {
            "name": "flp_info",
            "description": "List what is inside an FL Studio project: version, title, tempo, every channel with its plugin or sample, mixer insert, volume and whether it is automation, the patterns with their note counts, and each mixer insert's effect chain. Paths may start with %FLStudioFactoryData%.",
            "inputSchema": {
                "type": "object",
                "properties": { "file": { "type": "string", "description": "path to an .flp file" } },
                "required": ["file"]
            }
        },
        {
            "name": "references",
            "description": "List FL Studio's own demo songs (by real producers, mostly stock plugins) and genre templates to learn from and copy sounds out of. Gives a short summary of each by default: name, kind, tempo, how many channels and patterns, and its first few sounds (channel with plugin or sample, automated ones marked). Call flp_info on one project, or pass detail true, for every channel and pattern. Filter by words and tempo. Paths start with %FLStudioFactoryData% and work in the other tools.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "words": { "type": "string", "description": "words that must appear in the name, title or channels, like break or sytrus" },
                    "min_bpm": { "type": "number", "description": "lowest tempo" },
                    "max_bpm": { "type": "number", "description": "highest tempo" },
                    "detail": { "type": "boolean", "description": "every channel and pattern of every project instead of the summary, default false; this can be very long" }
                }
            }
        },
        {
            "name": "reference_pattern",
            "description": "List the notes of one pattern of any FL Studio project, with bar and step counted from 1 (16 steps a bar), note name (C4 is 60), length in steps, velocity and channel. Use it to read how a producer wrote a drum groove, bassline or chords.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "path to an .flp file" },
                    "pattern": { "type": "string", "description": "the pattern's name" }
                },
                "required": ["file", "pattern"]
            }
        },
        {
            "name": "find_samples",
            "description": "Find samples in FL Studio's own packs by words in their path, like 'kick', 'snare power' or 'loops break'. Returns paths starting with %FLStudioFactoryData% to use as a channel sample in compose_loop.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "words": { "type": "string", "description": "words that must all appear in the path" },
                    "limit": { "type": "integer", "description": "how many at most, default 30" }
                },
                "required": ["words"]
            }
        },
        {
            "name": "compose_loop",
            "description": "Write an FL Studio project from a loop description and save the description next to it as json, to change and write again. The description: name, tempo, bars (default 8), channels, patterns, playlist, markers and master. A channel is {name, sample, root?, volume?} with an FL pack or local sample, or {name, copy: {project, channel}, effects?, volume?}, which copies a channel from another project with its plugin, preset and mixer effects (effects false leaves them out; channels saved at volume 0 are reset to 78%). A pattern is {name, bars, notes?, grids?, copy?}: notes are {channel, bar, step, steps, key, velocity} with bar and step from 1, 16 steps a bar, key a name like Eb4 (C4 is 60) or a number; grids are {channel, bar, hits} with 16 characters a bar, X loud, x normal, o soft, g ghost, anything else a rest; copy is {project, pattern, channels: {their channel: our channel}, transpose?, from_bar?, bars?}. The playlist is a list of {pattern, track, bar, times}; without it every pattern repeats on its own track over the bars. Markers are {bar, name}. master is a project whose master effects are copied. tracks is a list of playlist track names, from track 1. mute is a list of track numbers to leave muted, for parts the user should check before using. stems is {folder, from_bar, bars?, at_bar?}: the song's four stems from a transcribed folder, cut to the song bars from from_bar (all the rest when bars is left out) and placed muted from at_bar (default 1) on tracks after the parts, so the user can unmute one and compare; the project tempo must be the song's.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "loop": { "type": "object", "description": "the loop description" },
                    "out": { "type": "string", "description": "the folder to write the project into; Documents/Image-Line/FL Studio/Projects/<name> opens straight in FL" },
                    "template": { "type": "string", "description": "an FL Studio project to use as the template, the built in one when left out" }
                },
                "required": ["loop", "out"]
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
        "song_overview" => path_arg(args, "folder")
            .and_then(|folder| views::overview(&folder))
            .and_then(text),
        "part_grid" => grid_tool(args).and_then(text),
        "song_parts" => parts_tool(args).and_then(text),
        "harmony" => path_arg(args, "folder")
            .and_then(|folder| {
                views::harmony(
                    &folder,
                    number(args, "from_bar", 1),
                    number(args, "bars", 9999),
                    number(args, "per", 0),
                )
            })
            .and_then(text),
        "write_flp" => write_tool(args).and_then(text),
        "flp_info" => factory_arg(args, "file")
            .and_then(|file| flp::events::read(&file))
            .and_then(|project| text(flp::info::summarize(&project))),
        "references" => compose::references(
            args["words"].as_str().unwrap_or(""),
            args["min_bpm"].as_f64().unwrap_or(0.0),
            args["max_bpm"].as_f64().unwrap_or(999.0),
        )
        .map(|found| {
            if args["detail"].as_bool().unwrap_or(false) {
                found
            } else {
                found.into_iter().map(compose::Reference::summary).collect()
            }
        })
        .and_then(text),
        "reference_pattern" => reference_pattern(args).and_then(text),
        "find_samples" => compose::find_samples(
            args["words"].as_str().unwrap_or(""),
            number(args, "limit", 30) as usize,
        )
        .and_then(text),
        "compose_loop" => compose_tool(args).and_then(text),
        other => Err(format!("unknown tool {}", other)),
    };
    match outcome {
        Ok(content) => json!({ "content": content, "isError": false }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

// a path argument that may start with fl studio's factory data folder
fn factory_arg(args: &Value, key: &str) -> Result<std::path::PathBuf, String> {
    let path = args[key]
        .as_str()
        .ok_or(format!("missing argument {}", key))?;
    Ok(compose::local_path(path))
}

// the notes of one pattern of a project
fn reference_pattern(args: &Value) -> Result<Vec<compose::PatternNote>, String> {
    let project = flp::events::read(&factory_arg(args, "file")?)?;
    let name = args["pattern"]
        .as_str()
        .ok_or("missing argument pattern".to_string())?;
    compose::pattern_view(&project, name)
}

// a part cut into lettered stretches with the notes each letter agrees on
fn parts_tool(args: &Value) -> Result<views::Parts, String> {
    let folder = path_arg(args, "folder")?;
    let name = args["name"]
        .as_str()
        .ok_or("missing argument name".to_string())?;
    views::parts(
        &folder,
        name,
        number(args, "from_bar", 1),
        number(args, "bars", 8),
        args["to_bar"].as_u64().map(|n| n as u32),
    )
    .map(|parts| parts.only_letter(args["letter"].as_str()))
}

// a part of a transcribed folder as 16 steps per bar
fn grid_tool(args: &Value) -> Result<Vec<views::GridBar>, String> {
    let folder = path_arg(args, "folder")?;
    let name = args["name"]
        .as_str()
        .ok_or("missing argument name".to_string())?;
    views::grid(
        &folder,
        name,
        number(args, "from_bar", 1),
        number(args, "bars", 4),
    )
}

// writes a loop description into a folder as an fl studio project
fn compose_tool(args: &Value) -> Result<compose::Written, String> {
    let out = path_arg(args, "out")?;
    let template = optional_path(args, "template");
    compose::write_json(&args["loop"], &out, template.as_deref())
}

// an answer as one block of compact json text, which agents read as well as indented json at a fraction of the size
fn text(value: impl serde::Serialize) -> Result<Vec<Value>, String> {
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
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
        raw_melody: args["raw_melody"].as_bool().unwrap_or(false),
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
        raw_melody: false,
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
        assert_eq!(reply["result"]["tools"].as_array().unwrap().len(), 19);
        let call = handle(&json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "nope", "arguments": {} }
        }))
        .unwrap();
        assert_eq!(call["result"]["isError"], true);
    }
}
