use crate::audio::wav;
use crate::combine;
use crate::flp::build::{
    self, ChannelSource, NewAudioClip, NewChannel, NewClip, NewNote, NewPattern,
};
use crate::flp::channels::{self, Block};
use crate::flp::events::{self, Project};
use crate::flp::info;
use crate::flp::mixer;
use crate::flp::timeline::{self, TimedNote};
use crate::midi::Marker;
use crate::music::{self, BAR, PPQ, STEP};
use crate::rebuild;
use crate::separate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FACTORY: &str = "%FLStudioFactoryData%";
const SUMMARY_CHANNELS: usize = 8;

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub name: String,
    pub tempo: f64,
    #[serde(default = "eight")]
    pub bars: u32,
    #[serde(default)]
    pub channels: Vec<ChannelSpec>,
    #[serde(default)]
    pub patterns: Vec<PatternSpec>,
    #[serde(default)]
    pub playlist: Vec<ClipSpec>,
    #[serde(default)]
    pub markers: Vec<MarkerSpec>,
    #[serde(default)]
    pub master: Option<String>,
    #[serde(default)]
    pub stems: Option<StemsSpec>,
    #[serde(default)]
    pub tracks: Vec<String>,
    #[serde(default)]
    pub mute: Vec<u16>,
    #[serde(default)]
    pub sections: Vec<SectionSpec>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SectionSpec {
    pub name: String,
    pub project: String,
    pub at: Vec<f64>,
    #[serde(default)]
    pub bars: Option<f64>,
    #[serde(default)]
    pub first_only: Vec<String>,
    #[serde(default)]
    pub last_only: Vec<String>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct StemsSpec {
    pub folder: String,
    #[serde(default = "one")]
    pub from_bar: f64,
    #[serde(default)]
    pub bars: Option<f64>,
    #[serde(default = "one")]
    pub at_bar: f64,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
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
    #[serde(default)]
    pub volume: Option<u32>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CopySpec {
    pub project: String,
    pub channel: String,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
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
    #[serde(default)]
    pub from_parts: Option<FromParts>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct FromParts {
    pub folder: String,
    pub part: String,
    pub letter: String,
    #[serde(default = "one")]
    pub from_bar: f64,
    #[serde(default = "eight_bars")]
    pub bars: f64,
    #[serde(default)]
    pub to_bar: Option<u32>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub channels: HashMap<String, String>,
    #[serde(default)]
    pub transpose: i32,
    #[serde(default)]
    pub low: Option<KeySpec>,
    #[serde(default)]
    pub high: Option<KeySpec>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
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

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
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

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct GridSpec {
    pub channel: String,
    pub hits: String,
    #[serde(default = "one")]
    pub bar: f64,
    #[serde(default)]
    pub key: Option<KeySpec>,
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum KeySpec {
    Number(u8),
    Name(String),
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ClipSpec {
    pub pattern: String,
    pub track: u16,
    #[serde(default = "one")]
    pub bar: f64,
    #[serde(default = "first")]
    pub times: u32,
    #[serde(default)]
    pub at: Vec<f64>,
    #[serde(default)]
    pub bars: Option<f64>,
    #[serde(default)]
    pub from_bar: Option<f64>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct MarkerSpec {
    pub bar: f64,
    pub name: String,
}

#[derive(Serialize)]
pub struct Reference {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub title: Option<String>,
    pub tempo: Option<f64>,
    pub channel_count: usize,
    pub pattern_count: usize,
    pub channels: Vec<String>,
    pub patterns: Vec<String>,
}

impl Reference {
    // the reference with only its first few sounds and no pattern list, for a short overview
    pub fn summary(mut self) -> Reference {
        self.channels.truncate(SUMMARY_CHANNELS);
        self.patterns.clear();
        self
    }
}

#[derive(Serialize)]
pub struct PatternNote {
    pub bar: u32,
    pub step: f64,
    pub note: String,
    pub key: u8,
    pub steps: f64,
    pub velocity: u8,
    pub channel: String,
}

#[derive(Serialize)]
pub struct Written {
    pub path: String,
    pub description: Option<String>,
    pub channels: usize,
    pub patterns: usize,
    pub clips: usize,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<Vec<crate::check::ChannelFit>>,
}

// eight bars, the default loop length
fn eight() -> u32 {
    8
}

// a default of one bar, step or step length
fn one() -> f64 {
    1.0
}

// eight bars, the usual chord cycle a song_parts letter spans
fn eight_bars() -> f64 {
    8.0
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
    let combined;
    let spec = if spec.sections.is_empty() {
        spec
    } else {
        let (expanded, found) = combine::expand(spec, out)?;
        warnings.extend(found);
        combined = expanded;
        &combined
    };

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
            let read = events::read(&project_path(project, out))
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
            volume: channel.volume,
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
        if let Some(from) = &pattern.from_parts {
            if pattern.bars < from.bars {
                warnings.push(format!(
                    "pattern {} is {} bars but its letter spans {}, so the rest is cut off",
                    pattern.name, pattern.bars, from.bars
                ));
            }
            notes.extend(lettered_notes(from, &channel_index)?);
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
    let playlist = default_playlist(spec);
    let placed: Vec<(usize, u16, f64, &ClipSpec)> = {
        let mut placed = Vec::new();
        for c in &playlist {
            let pattern = pattern_index(&c.pattern)?;
            let track = c.track.max(1) - 1;
            if c.at.is_empty() {
                placed.push((pattern, track, c.bar, c));
            } else {
                placed.extend(c.at.iter().map(|&bar| (pattern, track, bar, c)));
            }
        }
        placed
    };
    let mut clips = Vec::new();
    let mut track_names: Vec<String> = Vec::new();
    for &(pattern, track, bar, clip) in &placed {
        let full = spec.patterns[pattern].bars;
        let bars = clip.bars.unwrap_or(full);
        let length = (bars * BAR as f64).round() as u32;
        let cut = (clip.from_bar.is_some() || bars < full)
            .then(|| tick(clip.from_bar.unwrap_or(1.0), 1.0));
        for n in 0..clip.times.max(1) {
            clips.push(NewClip {
                pattern,
                start: tick(bar, 1.0) + n * length,
                length,
                track,
                cut,
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
    for (n, name) in spec.tracks.iter().enumerate() {
        if track_names.len() <= n {
            track_names.resize(n + 1, String::new());
        }
        track_names[n] = name.clone();
    }
    let mut audio = Vec::new();
    let mut muted: Vec<u16> = spec.mute.iter().map(|t| t.max(&1) - 1).collect();
    if let Some(stems) = &spec.stems {
        let first_track = track_names.len() as u16 + 1;
        for (n, (name, path, seconds)) in cut_stems(spec, stems, out, &mut warnings)?
            .into_iter()
            .enumerate()
        {
            let track = first_track + n as u16;
            audio.push(NewAudioClip {
                channel: new_channels.len(),
                start: tick(stems.at_bar, 1.0),
                seconds,
                track,
            });
            new_channels.push(NewChannel {
                name: name.clone(),
                source: ChannelSource::Audio { path },
                volume: None,
            });
            if track_names.len() <= track as usize {
                track_names.resize(track as usize + 1, String::new());
            }
            track_names[track as usize] = name;
            muted.push(track);
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
        &audio,
        &track_names,
        &muted,
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
    for (n, channel) in new_channels.iter().enumerate().skip(spec.channels.len()) {
        mixer::name_insert(&mut project, n + 1, &channel.name);
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
    let check = spec.stems.as_ref().and_then(|stems| {
        let song_bar = stems.from_bar - (stems.at_bar - 1.0);
        match crate::check::check(&project, &local_path(&stems.folder), song_bar) {
            Ok(found) => Some(found),
            Err(e) => {
                warnings.push(format!(
                    "the parts could not be checked against the song: {}",
                    e
                ));
                None
            }
        }
    });
    Ok(Written {
        path: path.display().to_string(),
        description: None,
        channels: new_channels.len(),
        patterns: built.patterns,
        clips: built.clips,
        warnings,
        check,
    })
}

// the playlist a description asks for, or every pattern repeated over the bars on its own track when it gives none
pub fn default_playlist(spec: &Loop) -> Vec<ClipSpec> {
    if !spec.playlist.is_empty() {
        return spec.playlist.clone();
    }
    spec.patterns
        .iter()
        .enumerate()
        .map(|(n, p)| ClipSpec {
            pattern: p.name.clone(),
            track: n as u16 + 1,
            bar: 1.0,
            times: (spec.bars as f64 / p.bars.max(0.25)).ceil() as u32,
            at: Vec::new(),
            bars: None,
            from_bar: None,
        })
        .collect()
}

// a project path with fl studio's factory folder filled in, and a bare file name looked for in the output folder
pub fn project_path(path: &str, out: &Path) -> PathBuf {
    let local = local_path(path);
    if local.is_relative() && !local.exists() {
        out.join(local)
    } else {
        local
    }
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

// writes a loop description given as json and saves the description next to the project, so it can be changed and written again
pub fn write_json(
    description: &serde_json::Value,
    out: &Path,
    template: Option<&Path>,
) -> Result<Written, String> {
    let spec: Loop = serde_json::from_value(description.clone())
        .map_err(|e| format!("not a loop description: {}", e))?;
    let mut written = write(&spec, out, template)?;
    let path = out.join(format!("{}.json", file_name(&spec.name)));
    let text = serde_json::to_string_pretty(description).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("can't write {}: {}", path.display(), e))?;
    written.description = Some(path.display().to_string());
    Ok(written)
}

// fl studio's own demo songs and genre templates whose name, title or channels hold every word, with their tempo and sounds
pub fn references(query: &str, min_bpm: f64, max_bpm: f64) -> Result<Vec<Reference>, String> {
    let root = factory_folder().ok_or("FL Studio's install folder was not found")?;
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    let data = root.join("Data");
    let places = [
        ("demo song", data.join("Demo projects").join("Demo songs")),
        (
            "template",
            data.join("Templates").join("Advanced").join("Electronic"),
        ),
        (
            "template",
            data.join("Templates").join("Advanced").join("Instrumental"),
        ),
    ];
    let installed = installed_plugins();
    let mut found = Vec::new();
    for (kind, folder) in places {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("flp")))
            .collect();
        files.sort();
        for file in files {
            let Ok(project) = events::read(&file) else {
                continue;
            };
            let summary = info::summarize(&project);
            if summary
                .tempo
                .is_some_and(|bpm| bpm < min_bpm || bpm > max_bpm)
            {
                continue;
            }
            let name = file
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let channels: Vec<String> = summary
                .channels
                .iter()
                .filter(|c| !c.automation)
                .map(|c| {
                    let sound = c
                        .sample
                        .as_deref()
                        .and_then(|s| s.rsplit(['\\', '/']).next())
                        .or(c.plugin.as_deref());
                    let missing = match (&c.plugin, &installed) {
                        (Some(plugin), Some(installed)) if c.sample.is_none() => {
                            !plugin.eq_ignore_ascii_case("fruity wrapper")
                                && !installed.contains(&plugin.to_lowercase())
                        }
                        _ => false,
                    };
                    match (sound, c.volume) {
                        _ if missing => {
                            format!("{} ({}, not installed)", c.name, sound.unwrap_or(""))
                        }
                        (_, Some(0)) => format!("{} (volume 0, automated)", c.name),
                        (Some(sound), _) => format!("{} ({})", c.name, sound),
                        (None, _) => c.name.clone(),
                    }
                })
                .collect();
            let text = format!(
                "{} {} {}",
                name,
                summary.title.as_deref().unwrap_or(""),
                channels.join(" ")
            )
            .to_lowercase();
            if !words.iter().all(|w| text.contains(w)) {
                continue;
            }
            let relative = file.strip_prefix(&root).unwrap_or(&file);
            let patterns: Vec<String> = summary
                .patterns
                .iter()
                .filter(|p| p.notes > 0)
                .map(|p| p.name.clone())
                .collect();
            found.push(Reference {
                name,
                kind: kind.to_string(),
                channel_count: channels.len(),
                pattern_count: patterns.len(),
                path: format!(
                    "{}/{}",
                    FACTORY,
                    relative.display().to_string().replace('\\', "/")
                ),
                title: summary.title,
                tempo: summary.tempo,
                channels,
                patterns,
            });
        }
    }
    found.sort_by(|a, b| a.tempo.unwrap_or(0.0).total_cmp(&b.tempo.unwrap_or(0.0)));
    Ok(found)
}

// the plugins fl studio lists as installed in its plugin database, in lowercase, if that list is found
fn installed_plugins() -> Option<std::collections::HashSet<String>> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let root = PathBuf::from(home)
        .join("Documents")
        .join("Image-Line")
        .join("FL Studio")
        .join("Presets")
        .join("Plugin database")
        .join("Installed");
    let mut names = std::collections::HashSet::new();
    let mut folders = vec![root];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                folders.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("fst"))
            {
                if let Some(stem) = path.file_stem() {
                    names.insert(stem.to_string_lossy().to_lowercase());
                }
            }
        }
    }
    (!names.is_empty()).then_some(names)
}

// the notes one song_parts letter agrees on, moved into a register and put on the loop's channels
fn lettered_notes(
    from: &FromParts,
    channel_index: &dyn Fn(&str) -> Result<usize, String>,
) -> Result<Vec<NewNote>, String> {
    let parts = crate::views::parts(
        &local_path(&from.folder),
        &from.part,
        from.from_bar.max(1.0) as u32,
        from.bars.max(1.0) as u32,
        from.to_bar,
    )?
    .only_letter(Some(&from.letter));
    let group = parts.groups.first().ok_or_else(|| {
        format!(
            "{} has no letter {} for {} from bar {} by {}",
            from.folder, from.letter, from.part, from.from_bar, from.bars
        )
    })?;
    let low = match &from.low {
        Some(key) => Some(key_of(key)?),
        None => None,
    };
    let high = match &from.high {
        Some(key) => Some(key_of(key)?),
        None => None,
    };
    let drum = |key: u8| match key {
        36 => "kick",
        38 => "snare",
        _ => "hat",
    };
    let mut notes = Vec::new();
    for note in &group.notes {
        let channel = if from.part == "drums" {
            match from.channels.get(drum(note.key)) {
                Some(name) => channel_index(name)?,
                None => continue,
            }
        } else {
            let name = from
                .channel
                .as_deref()
                .ok_or_else(|| format!("from_parts for {} needs a channel", from.part))?;
            channel_index(name)?
        };
        let mut key = (note.key as i32 + from.transpose).clamp(0, 127);
        if let (Some(low), Some(high)) = (low, high) {
            if high >= low + 11 {
                while key < low as i32 {
                    key += 12;
                }
                while key > high as i32 {
                    key -= 12;
                }
            }
        }
        notes.push(NewNote {
            channel,
            start: tick(note.bar as f64, note.step as f64),
            length: note.steps.max(1) * STEP,
            key: if from.part == "drums" { 60 } else { key as u8 },
            velocity: note.velocity.min(127),
        });
    }
    Ok(notes)
}

// the notes of a project's pattern with bars and steps counted from 1 and the channel each plays on
pub fn pattern_view(project: &Project, name: &str) -> Result<Vec<PatternNote>, String> {
    let names: HashMap<u16, String> = info::summarize(project)
        .channels
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();
    let bar = project.ppq as u32 * 4;
    let step = (project.ppq as u32 / 4).max(1);
    let round = |v: f64| (v * 100.0).round() / 100.0;
    Ok(reference_notes(project, name)?
        .into_iter()
        .map(|note| PatternNote {
            bar: note.start / bar + 1,
            step: round((note.start % bar) as f64 / step as f64 + 1.0),
            note: music::note_name(note.key),
            key: note.key,
            steps: round(note.length as f64 / step as f64),
            velocity: note.velocity,
            channel: names
                .get(&note.channel)
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
        })
        .collect())
}

// the song's four stems cut to the bars a loop or song comes from, saved next to the project, with their names and lengths
fn cut_stems(
    spec: &Loop,
    stems: &StemsSpec,
    out: &Path,
    warnings: &mut Vec<String>,
) -> Result<Vec<(String, String, f64)>, String> {
    let folder = local_path(&stems.folder);
    let session = rebuild::load_session(&folder)?;
    let grid = session.grid;
    if (grid.bpm - spec.tempo).abs() > grid.bpm * 0.005 {
        warnings.push(format!(
            "the song is {} bpm but the project is {} bpm, so the stems won't line up with the patterns",
            grid.bpm, spec.tempo
        ));
    }
    let start = grid.origin + (stems.from_bar - 1.0).max(0.0) * grid.bar_seconds();
    let end = stems
        .bars
        .map_or(f64::MAX, |bars| start + bars * grid.bar_seconds());
    std::fs::create_dir_all(out).map_err(|e| format!("can't create {}: {}", out.display(), e))?;
    let mut found = Vec::new();
    for stem in separate::STEMS {
        let file = separate::stem_file(stem);
        let source = folder.join(&file);
        if !source.exists() {
            warnings.push(format!("{} has no {}", stems.folder, file));
            continue;
        }
        let cut = wav::read_wav(&source)?.cut(start, end);
        let name = file.trim_end_matches(".wav").to_string();
        let target = out.join(format!("{} - {}", file_name(&spec.name), file));
        wav::write_wav(&target, &cut)?;
        found.push((
            name,
            fl_path(&target.display().to_string()),
            cut.info.duration_seconds(),
        ));
    }
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
pub fn local_path(path: &str) -> PathBuf {
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

    // a misspelled or unknown setting is refused with its name instead of being ignored
    #[test]
    fn refuses_unknown_settings() {
        let error = serde_json::from_str::<Loop>(
            r#"{ "name": "x", "tempo": 120, "channels": [], "patterns": [], "stemz": {} }"#,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("stemz"));
        let nested = serde_json::from_str::<Loop>(
            r#"{ "name": "x", "tempo": 120, "channels": [ { "name": "Kick", "sampel": "k.wav" } ], "patterns": [] }"#,
        );
        assert!(nested.err().unwrap().to_string().contains("sampel"));
    }

    // a clip with a list of bars is placed once at each of them
    #[test]
    fn places_at_listed_bars() {
        let spec: Loop = serde_json::from_str(
            r#"{
                "name": "listed bars",
                "tempo": 120,
                "bars": 16,
                "channels": [ { "name": "Kick", "sample": "C:\\kick.wav" } ],
                "patterns": [ { "name": "Kick", "grids": [ { "channel": "Kick", "hits": "x..............." } ] } ],
                "playlist": [ { "pattern": "Kick", "track": 1, "at": [1, 5, 13] } ]
            }"#,
        )
        .unwrap();
        let dir = std::env::temp_dir().join("unbake_listed_bars_test");
        let written = write(&spec, &dir, None).unwrap();
        assert_eq!(written.clips, 3);
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
