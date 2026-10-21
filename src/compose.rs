use crate::flp::build::{self, ChannelSource, NewChannel, NewClip, NewNote, NewPattern};
use crate::flp::channels::{self, Block};
use crate::flp::events::{self, Project};
use crate::flp::info;
use crate::flp::mixer;
use crate::flp::timeline::{self, TimedNote};
use crate::midi::Marker;
use crate::music::{self, BAR, PPQ, STEP};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FACTORY: &str = "%FLStudioFactoryData%";

#[derive(Deserialize)]
pub struct Loop {
    pub name: String,
    pub tempo: f64,
    #[serde(default = "eight")]
    pub bars: u32,
    pub channels: Vec<ChannelSpec>,
    pub patterns: Vec<PatternSpec>,
    #[serde(default)]
    pub playlist: Vec<ClipSpec>,
    #[serde(default)]
    pub markers: Vec<MarkerSpec>,
    #[serde(default)]
    pub master: Option<String>,
}

#[derive(Deserialize)]
pub struct ChannelSpec {
    pub name: String,
    #[serde(default)]
    pub sample: Option<String>,
    #[serde(default)]
    pub root: Option<KeySpec>,
    #[serde(default)]
    pub copy: Option<CopySpec>,
    #[serde(default = "yes")]
    pub effects: bool,
}

#[derive(Deserialize)]
pub struct CopySpec {
    pub project: String,
    pub channel: String,
}

#[derive(Deserialize)]
pub struct PatternSpec {
    pub name: String,
    #[serde(default = "one")]
    pub bars: f64,
    #[serde(default)]
    pub notes: Vec<NoteSpec>,
    #[serde(default)]
    pub grids: Vec<GridSpec>,
    #[serde(default)]
    pub copy: Option<PatternCopy>,
}

#[derive(Deserialize)]
pub struct PatternCopy {
    pub project: String,
    pub pattern: String,
    pub channels: HashMap<String, String>,
    #[serde(default)]
    pub transpose: i32,
    #[serde(default)]
    pub from_bar: Option<f64>,
    #[serde(default)]
    pub bars: Option<f64>,
}

#[derive(Deserialize)]
pub struct NoteSpec {
    pub channel: String,
    #[serde(default = "one")]
    pub bar: f64,
    #[serde(default = "one")]
    pub step: f64,
    #[serde(default = "one")]
    pub steps: f64,
    #[serde(default)]
    pub key: Option<KeySpec>,
    #[serde(default = "loud")]
    pub velocity: u8,
}

#[derive(Deserialize)]
pub struct GridSpec {
    pub channel: String,
    pub hits: String,
    #[serde(default = "one")]
    pub bar: f64,
    #[serde(default)]
    pub key: Option<KeySpec>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum KeySpec {
    Number(u8),
    Name(String),
}

#[derive(Deserialize)]
pub struct ClipSpec {
    pub pattern: String,
    pub track: u16,
    #[serde(default = "one")]
    pub bar: f64,
    #[serde(default = "first")]
    pub times: u32,
}

#[derive(Deserialize)]
pub struct MarkerSpec {
    pub bar: f64,
    pub name: String,
}

#[derive(Serialize)]
pub struct Written {
    pub path: String,
    pub channels: usize,
    pub patterns: usize,
    pub clips: usize,
    pub warnings: Vec<String>,
}

// eight bars, the default loop length
fn eight() -> u32 {
    8
}

// a default of one bar, step or step length
fn one() -> f64 {
    1.0
}

// a clip placed once
fn first() -> u32 {
    1
}

// copying a channel's mixer effects unless told not to
fn yes() -> bool {
    true
}

// the default velocity of a note
fn loud() -> u8 {
    100
}

// reads a loop description and writes it as an fl project into a folder, returning where it went
pub fn write(spec: &Loop, out: &Path, template: Option<&Path>) -> Result<Written, String> {
    let template = match template {
        Some(path) => events::read(path)?,
        None => events::parse(crate::rebuild::BUILT_IN_TEMPLATE)?,
    };
    let mut warnings = Vec::new();

    let mut sources: HashMap<String, Project> = HashMap::new();
    let wanted = spec
        .channels
        .iter()
        .filter_map(|c| c.copy.as_ref().map(|c| &c.project))
        .chain(spec.master.iter())
        .chain(
            spec.patterns
                .iter()
                .filter_map(|p| p.copy.as_ref().map(|c| &c.project)),
        );
    for project in wanted {
        if !sources.contains_key(project) {
            let read = events::read(&local_path(project))
                .map_err(|e| format!("can't read {}: {}", project, e))?;
            sources.insert(project.clone(), read);
        }
    }

    let mut new_channels = Vec::new();
    for channel in &spec.channels {
        let source = match (&channel.copy, &channel.sample) {
            (Some(copy), _) => {
                let from = &sources[&copy.project];
                let block = find_block(from, &copy.channel).ok_or_else(|| {
                    format!("{} has no channel called {}", copy.project, copy.channel)
                })?;
                ChannelSource::Copy { from, block }
            }
            (None, Some(sample)) => {
                if !local_path(sample).exists() {
                    warnings.push(format!("sample not found: {}", sample));
                }
                let root = match &channel.root {
                    Some(key) => key_of(key)?,
                    None => 60,
                };
                ChannelSource::Sample {
                    path: fl_path(sample),
                    root,
                }
            }
            (None, None) => {
                return Err(format!(
                    "channel {} needs a sample or a channel to copy",
                    channel.name
                ));
            }
        };
        new_channels.push(NewChannel {
            name: channel.name.clone(),
            source,
        });
    }

    let channel_index = |name: &str| -> Result<usize, String> {
        spec.channels
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| format!("no channel called {}", name))
    };
    let mut patterns = Vec::new();
    for pattern in &spec.patterns {
        let mut notes = Vec::new();
        for note in &pattern.notes {
            notes.push(NewNote {
                channel: channel_index(&note.channel)?,
                start: tick(note.bar, note.step),
                length: ((note.steps * STEP as f64).round() as u32).max(1),
                key: match &note.key {
                    Some(key) => key_of(key)?,
                    None => 60,
                },
                velocity: note.velocity.min(127),
            });
        }
        for grid in &pattern.grids {
            let channel = channel_index(&grid.channel)?;
            let key = match &grid.key {
                Some(key) => key_of(key)?,
                None => 60,
            };
            let start = tick(grid.bar, 1.0);
            let steps = grid
                .hits
                .chars()
                .filter(|c| !c.is_whitespace() && *c != '|');
            for (n, c) in steps.enumerate() {
                let velocity = match c {
                    'X' => 120,
                    'x' => 100,
                    'o' => 70,
                    'g' => 45,
                    _ => continue,
                };
                notes.push(NewNote {
                    channel,
                    start: start + n as u32 * STEP,
                    length: STEP,
                    key,
                    velocity,
                });
            }
        }
        if let Some(copy) = &pattern.copy {
            let from = &sources[&copy.project];
            let mut mapping = HashMap::new();
            for (theirs, ours) in &copy.channels {
                let block = find_block(from, theirs)
                    .ok_or_else(|| format!("{} has no channel called {}", copy.project, theirs))?;
                mapping.insert(block.id, channel_index(ours)?);
            }
            let scale = |tick: u32| (tick as u64 * PPQ as u64 / from.ppq.max(1) as u64) as u32;
            let first = copy.from_bar.map_or(0, |bar| tick(bar, 1.0));
            let end = copy
                .bars
                .map_or(u32::MAX, |bars| first + (bars * BAR as f64).round() as u32);
            for note in reference_notes(from, &copy.pattern)? {
                let Some(&channel) = mapping.get(&note.channel) else {
                    continue;
                };
                let start = scale(note.start);
                if start < first || start >= end {
                    continue;
                }
                notes.push(NewNote {
                    channel,
                    start: start - first,
                    length: scale(note.length).max(1),
                    key: (note.key as i32 + copy.transpose).clamp(0, 127) as u8,
                    velocity: note.velocity,
                });
            }
        }
        patterns.push(NewPattern {
            name: pattern.name.clone(),
            notes,
        });
    }

    let pattern_index = |name: &str| -> Result<usize, String> {
        spec.patterns
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| format!("no pattern called {}", name))
    };
    let placed: Vec<(usize, u16, f64, u32)> = if spec.playlist.is_empty() {
        spec.patterns
            .iter()
            .enumerate()
            .map(|(n, p)| {
                let times = (spec.bars as f64 / p.bars.max(0.25)).ceil() as u32;
                (n, n as u16, 1.0, times)
            })
            .collect()
    } else {
        spec.playlist
            .iter()
            .map(|c| {
                Ok((
                    pattern_index(&c.pattern)?,
                    c.track.max(1) - 1,
                    c.bar,
                    c.times,
                ))
            })
            .collect::<Result<_, String>>()?
    };
    let mut clips = Vec::new();
    let mut track_names: Vec<String> = Vec::new();
    for &(pattern, track, bar, times) in &placed {
        let length = (spec.patterns[pattern].bars * BAR as f64).round() as u32;
        for n in 0..times.max(1) {
            clips.push(NewClip {
                pattern,
                start: tick(bar, 1.0) + n * length,
                length,
                track,
            });
        }
        let track = track as usize;
        if track_names.len() <= track {
            track_names.resize(track + 1, String::new());
        }
        if track_names[track].is_empty() {
            track_names[track] = spec.patterns[pattern].name.clone();
        }
    }
    let markers: Vec<Marker> = spec
        .markers
        .iter()
        .map(|m| Marker {
            tick: tick(m.bar, 1.0),
            name: m.name.clone(),
        })
        .collect();

    let built = build::compose(
        &template,
        &new_channels,
        &patterns,
        &clips,
        &track_names,
        spec.tempo,
        &markers,
    )?;
    warnings.extend(built.warnings);
    let mut project = built.project;
    for (n, (channel, new)) in spec.channels.iter().zip(&new_channels).enumerate() {
        let insert = n + 1;
        mixer::name_insert(&mut project, insert, &channel.name);
        let ChannelSource::Copy { from, block } = &new.source else {
            continue;
        };
        if !channel.effects {
            continue;
        }
        let Some(source) = mixer::channel_insert(from, block.start, block.end) else {
            continue;
        };
        if source > 0 && !mixer::copy_effects(&mut project, insert, from, source) {
            warnings.push(format!(
                "the mixer effects of {} could not be copied",
                channel.name
            ));
        }
    }
    if let Some(master) = &spec.master {
        if !mixer::copy_effects(&mut project, 0, &sources[master], 0) {
            warnings.push(format!(
                "the master effects of {} could not be copied",
                master
            ));
        }
    }
    std::fs::create_dir_all(out).map_err(|e| format!("can't create {}: {}", out.display(), e))?;
    let path = out.join(format!("{}.flp", file_name(&spec.name)));
    events::write(&path, &project)?;
    Ok(Written {
        path: path.display().to_string(),
        channels: new_channels.len(),
        patterns: built.patterns,
        clips: built.clips,
        warnings,
    })
}

// the samples in fl studio's own packs whose path holds every word of a search, as paths fl studio understands
pub fn find_samples(query: &str, limit: usize) -> Result<Vec<String>, String> {
    let root = factory_folder().ok_or("FL Studio's install folder was not found")?;
    let packs = root.join("Data").join("Patches").join("Packs");
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    let mut found = Vec::new();
    let mut folders = vec![packs.clone()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let audio = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e.to_lowercase().as_str(),
                    "wav" | "wv" | "ogg" | "mp3" | "flac"
                )
            });
            let Ok(relative) = path.strip_prefix(&root) else {
                continue;
            };
            let text = relative.display().to_string();
            let lower = text.to_lowercase();
            if audio && words.iter().all(|w| lower.contains(w)) {
                found.push(format!("{}\\{}", FACTORY, text.replace('/', "\\")));
            }
        }
    }
    found.sort();
    found.truncate(limit);
    Ok(found)
}

// the notes of a project's pattern found by name, exact first and then ignoring case
pub fn reference_notes(project: &Project, name: &str) -> Result<Vec<TimedNote>, String> {
    let summary = info::summarize(project);
    let id = summary
        .patterns
        .iter()
        .find(|p| p.name == name)
        .or_else(|| {
            summary
                .patterns
                .iter()
                .find(|p| p.name.to_lowercase() == name.to_lowercase())
        })
        .map(|p| p.id)
        .ok_or_else(|| format!("no pattern called {}", name))?;
    let mut notes = timeline::pattern_notes(project)
        .remove(&id)
        .unwrap_or_default();
    notes.sort_by_key(|n| (n.start, n.channel, n.key));
    Ok(notes)
}

// where fl studio is installed, the folder its factory data paths point to
pub fn factory_folder() -> Option<PathBuf> {
    let base = PathBuf::from(r"C:\Program Files\Image-Line");
    let mut installs: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("FL Studio 2") || n.starts_with("FL Studio 1"))
                && p.join("Data").is_dir()
        })
        .collect();
    installs.sort();
    installs.pop()
}

// a path with fl studio's factory data folder filled in, so it can be opened here
fn local_path(path: &str) -> PathBuf {
    match (path.strip_prefix(FACTORY), factory_folder()) {
        (Some(rest), Some(root)) => root.join(rest.trim_start_matches(['\\', '/'])),
        _ => PathBuf::from(path),
    }
}

// a sample path written the way fl studio stores it, with backslashes on windows
fn fl_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

// the channel of a project with a name, exact first and then ignoring case
fn find_block(project: &Project, name: &str) -> Option<Block> {
    let blocks = channels::blocks(project);
    blocks
        .iter()
        .find(|b| b.name == name)
        .or_else(|| {
            blocks
                .iter()
                .find(|b| b.name.to_lowercase() == name.to_lowercase())
        })
        .cloned()
}

// the tick of a bar and a 1/16 step, both counted from 1
fn tick(bar: f64, step: f64) -> u32 {
    (((bar - 1.0).max(0.0) * BAR as f64) + ((step - 1.0).max(0.0) * STEP as f64)).round() as u32
}

// the midi key of a note given as a number or a name
fn key_of(key: &KeySpec) -> Result<u8, String> {
    match key {
        KeySpec::Number(n) => Ok((*n).min(127)),
        KeySpec::Name(name) => music::parse_note(name)
            .ok_or_else(|| format!("{} is not a note name like C4 or Eb3", name)),
    }
}

// a name made safe to use as a file name
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // a loop with a sampled kick on every beat for one bar written and read back
    #[test]
    fn writes_a_loop() {
        let spec: Loop = serde_json::from_str(
            r#"{
                "name": "test loop",
                "tempo": 170,
                "bars": 4,
                "channels": [
                    { "name": "Kick", "sample": "C:\\kick.wav" },
                    { "name": "Bass", "sample": "C:\\bass.wav", "root": "C3" }
                ],
                "patterns": [
                    { "name": "Drums", "grids": [ { "channel": "Kick", "hits": "x...x...x...x..." } ] },
                    { "name": "Bass", "bars": 2, "notes": [ { "channel": "Bass", "key": "C2", "steps": 8 } ] }
                ]
            }"#,
        )
        .unwrap();
        let dir = std::env::temp_dir().join("unbake_compose_test");
        let written = write(&spec, &dir, None).unwrap();
        assert_eq!(written.channels, 2);
        assert_eq!(written.patterns, 2);
        assert_eq!(written.clips, 4 + 2);
        let project = events::read(Path::new(&written.path)).unwrap();
        let summary = crate::flp::info::summarize(&project);
        assert_eq!(summary.channels.len(), 2);
        assert_eq!(summary.channels[1].name, "Bass");
        assert_eq!(summary.tempo, Some(170.0));
        assert_eq!(summary.patterns[0].notes, 4);
    }

    // note names follow c4 as 60 with sharps and flats
    #[test]
    fn parses_note_names() {
        assert_eq!(music::parse_note("C4"), Some(60));
        assert_eq!(music::parse_note("Eb3"), Some(51));
        assert_eq!(music::parse_note("F#5"), Some(78));
        assert_eq!(music::parse_note("H2"), None);
    }
}
