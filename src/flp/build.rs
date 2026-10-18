use crate::arrange::Arrangement;
use crate::flp::channels::{self, AUDIO_CLIP, Block, SAMPLER};
use crate::flp::events::{self, Event, Project, Value};
use crate::flp::info;
use crate::midi::Marker;
use crate::music::{PPQ, Part};

const PATTERN_BASE: u16 = 20480;
const LAST_TRACK: u16 = 499;
const PARTS: [Part; 6] = [
    Part::Kick,
    Part::Snare,
    Part::Hat,
    Part::Bass,
    Part::Melody,
    Part::Chords,
];

pub struct Built {
    pub project: Project,
    pub patterns: usize,
    pub clips: usize,
    pub warnings: Vec<String>,
}

pub struct Stem {
    pub name: String,
    pub path: String,
    pub seconds: f64,
}

struct ChannelSetup {
    parts: Vec<(Part, u16)>,
    stems: Vec<(u16, f64)>,
}

pub struct Sound {
    pub part: Part,
    pub path: String,
    pub root: u8,
}

pub struct Extras {
    pub sounds: Vec<Sound>,
    pub stems: Vec<Stem>,
    pub data_folder: Option<String>,
    pub reuse_audio_channels: bool,
    pub shift: u32,
    pub stem_start: u32,
}

// fills a copy of a template with our channels, sounds, tempo, patterns, clips, stems, markers and track names
pub fn build(
    template: &Project,
    arrangement: &Arrangement,
    track_names: &[String],
    bpm: f64,
    markers: &[Marker],
    extras: &Extras,
) -> Result<Built, String> {
    let mut project = template.clone();
    let mut warnings = Vec::new();
    let ppq = template.ppq as u64;
    let scale = |tick: u32| (tick as u64 * ppq / PPQ as u64) as u32;

    set_tempo(&mut project, bpm);
    if let Some(folder) = &extras.data_folder {
        set_data_folder(&mut project, folder);
    }

    let setup = set_up_channels(&mut project, arrangement, extras, &mut warnings);
    let channels = setup.parts;
    let stem_channels = setup.stems;

    let summary = info::summarize(&project);
    let mut next_id = summary.patterns.iter().map(|p| p.id).max().unwrap_or(0) + 1;
    let mut pattern_ids: Vec<Option<u16>> = Vec::new();
    let mut pattern_events = Vec::new();
    for pattern in &arrangement.patterns {
        let mut notes = Vec::new();
        for note in &pattern.notes {
            let Some(&(_, channel)) = channels.iter().find(|(p, _)| *p == note.part) else {
                continue;
            };
            let key = if note.part.is_drum() { 60 } else { note.key };
            notes.extend(note_bytes(
                channel,
                scale(note.start),
                scale(note.length),
                key,
                note.velocity,
            ));
        }
        if notes.is_empty() {
            pattern_ids.push(None);
            continue;
        }
        pattern_ids.push(Some(next_id));
        pattern_events.push(Event {
            id: events::PATTERN_NEW,
            value: Value::Word(next_id),
        });
        next_id += 1;
        pattern_events.push(Event {
            id: events::PATTERN_NAME,
            value: Value::Data(events::encode_text(&pattern.name)),
        });
        pattern_events.push(Event {
            id: events::PATTERN_NOTES,
            value: Value::Data(notes),
        });
    }
    let first_channel = project
        .find(events::CHANNEL_NEW)
        .ok_or("the template has no channels")?;
    insert_events(&mut project, first_channel, pattern_events);

    let arrangement_start = project
        .find(events::ARRANGEMENT_NEW)
        .ok_or("the template has no arrangement, save it with FL Studio 20 or newer".to_string())?;
    let playlist = (arrangement_start..project.events.len())
        .find(|&i| project.events[i].id == events::PLAYLIST)
        .ok_or("the template has no playlist".to_string())?;

    let item_size = info::playlist_item_size(template);
    let extra = match &project.events[playlist].value {
        Value::Data(data) if item_size > 32 && data.len() >= item_size => {
            data[32..item_size].to_vec()
        }
        _ if item_size > 32 => {
            warnings.push(
                "FL Studio 21+ template has no clip in its playlist to copy from, \
                 place one pattern clip in the template if the project won't open"
                    .to_string(),
            );
            vec![0; item_size - 32]
        }
        _ => Vec::new(),
    };

    let mut items: Vec<Vec<u8>> = Vec::new();
    for clip in &arrangement.clips {
        let Some(id) = pattern_ids[clip.pattern] else {
            continue;
        };
        items.push(item_bytes(
            PATTERN_BASE + id,
            scale(clip.start + extras.shift),
            scale(clip.length),
            clip.track,
            (-1i32).to_le_bytes(),
            &extra,
        ));
    }
    for (n, (channel, seconds)) in stem_channels.iter().enumerate() {
        let length = (seconds * bpm / 60.0 * ppq as f64).round() as u32;
        items.push(item_bytes(
            *channel,
            scale(extras.stem_start),
            length,
            (track_names.len() + n) as u16,
            (-1f32).to_le_bytes(),
            &extra,
        ));
    }
    items.sort_by_key(|item| u32::from_le_bytes([item[0], item[1], item[2], item[3]]));
    for (n, item) in items.iter_mut().enumerate() {
        if item.len() >= 36 {
            item[32..36].copy_from_slice(&(n as u32 + 1).to_le_bytes());
        }
    }
    let clips = items.len();
    project.events[playlist].value = Value::Data(items.concat());

    let mut marker_events = Vec::new();
    for marker in markers {
        marker_events.push(Event {
            id: events::MARKER_POSITION,
            value: Value::Dword(scale(marker.tick + extras.shift)),
        });
        marker_events.push(Event {
            id: events::MARKER_NAME,
            value: Value::Data(events::encode_text(&marker.name)),
        });
    }
    insert_events(&mut project, playlist + 1, marker_events);

    for (track, name) in track_names.iter().enumerate() {
        name_track(&mut project, arrangement_start, track as u32 + 1, name);
    }
    for (n, stem) in extras.stems.iter().take(stem_channels.len()).enumerate() {
        let number = (track_names.len() + n) as u32 + 1;
        name_track(&mut project, arrangement_start, number, &stem.name);
        mute_track(&mut project, arrangement_start, number);
    }

    Ok(Built {
        project,
        patterns: pattern_ids.iter().flatten().count(),
        clips,
        warnings,
    })
}

// gives every part a channel and every stem an audio channel, copying the template's own channels as models
fn set_up_channels(
    project: &mut Project,
    arrangement: &Arrangement,
    extras: &Extras,
    warnings: &mut Vec<String>,
) -> ChannelSetup {
    let start_blocks = channels::blocks(project);
    let mut next = start_blocks.iter().map(|b| b.id).max().map_or(0, |m| m + 1);
    let mut claimed: Vec<u16> = Vec::new();
    let mut new_events: Vec<Event> = Vec::new();

    let mut stem_channels = Vec::new();
    if !extras.stems.is_empty() {
        match start_blocks.iter().find(|b| b.kind == AUDIO_CLIP).cloned() {
            None => warnings.push(
                "the template has no audio clip channel to copy, so the stems were not added"
                    .to_string(),
            ),
            Some(model) => {
                let mut reusable: Vec<Block> = if extras.reuse_audio_channels {
                    start_blocks
                        .iter()
                        .filter(|b| b.kind == AUDIO_CLIP)
                        .cloned()
                        .collect()
                } else {
                    Vec::new()
                };
                for stem in &extras.stems {
                    if !reusable.is_empty() {
                        let block = reusable.remove(0);
                        channels::set_channel(project, block.id, Some(&stem.name), &stem.path, 60);
                        claimed.push(block.id);
                        stem_channels.push((block.id, stem.seconds));
                    } else {
                        new_events.extend(channels::copy_channel(
                            project, &model, next, &stem.name, &stem.path, 60,
                        ));
                        stem_channels.push((next, stem.seconds));
                        next += 1;
                    }
                }
            }
        }
    }

    let mut parts = Vec::new();
    for part in PARTS {
        let used = arrangement
            .patterns
            .iter()
            .any(|p| p.notes.iter().any(|n| n.part == part));
        if !used {
            continue;
        }
        let blocks = channels::blocks(project);
        let available: Vec<&Block> = blocks.iter().filter(|b| !claimed.contains(&b.id)).collect();
        let sound = extras.sounds.iter().find(|s| s.part == part);

        if let Some(block) = find_channel(&available, part) {
            if let (Some(sound), true) = (sound, block.kind == SAMPLER) {
                let name = extras.reuse_audio_channels.then(|| display_name(part));
                channels::set_channel(project, block.id, name, &sound.path, sound.root);
            }
            claimed.push(block.id);
            parts.push((part, block.id));
            continue;
        }

        let model = blocks.iter().find(|b| b.kind == SAMPLER);
        match (sound, model) {
            (Some(sound), Some(model)) => {
                new_events.extend(channels::copy_channel(
                    project,
                    model,
                    next,
                    display_name(part),
                    &sound.path,
                    sound.root,
                ));
                parts.push((part, next));
                next += 1;
            }
            _ => warnings.push(format!(
                "the template has no channel with \"{}\" in its name, those notes were left out",
                part.name()
            )),
        }
    }

    if let Some(at) = project.find(events::ARRANGEMENT_NEW) {
        insert_events(project, at, new_events);
    }
    project.channel_count = channels::blocks(project).len() as u16;
    ChannelSetup {
        parts,
        stems: stem_channels,
    }
}

// the channel name we give a part
fn display_name(part: Part) -> &'static str {
    match part {
        Part::Kick => "Kick",
        Part::Snare => "Snare",
        Part::Hat => "Hat",
        Part::Bass => "Bass",
        Part::Melody => "Lead",
        Part::Chords => "Pad",
    }
}

// sets the project tempo, which fl studio stores as bpm times 1000
fn set_tempo(project: &mut Project, bpm: f64) {
    let value = Value::Dword((bpm * 1000.0).round() as u32);
    match project.find(events::TEMPO) {
        Some(i) => project.events[i].value = value,
        None => {
            let at = project.find(events::VERSION).map(|i| i + 1).unwrap_or(0);
            insert_events(
                project,
                at,
                vec![Event {
                    id: events::TEMPO,
                    value,
                }],
            );
        }
    }
}

// sets the folder fl studio uses for the project's own files, where it also looks for missing samples
fn set_data_folder(project: &mut Project, folder: &str) {
    let value = Value::Data(events::encode_text(&format!(
        "{}\\",
        folder.trim_end_matches('\\')
    )));
    if let Some(i) = project.find(events::DATA_PATH) {
        project.events[i].value = value;
    }
}

// the channel whose name suggests a part, earlier words win, so a snare channel beats a clap channel
fn find_channel<'a>(channels: &[&'a Block], part: Part) -> Option<&'a Block> {
    let words: &[&str] = match part {
        Part::Kick => &["kick"],
        Part::Snare => &["snare", "clap"],
        Part::Hat => &["hat", "hh"],
        Part::Bass => &["bass"],
        Part::Melody => &["lead", "melody", "pluck"],
        Part::Chords => &["chord", "pad", "keys", "piano"],
    };
    for word in words {
        for channel in channels {
            if channel.name.to_lowercase().contains(word) {
                return Some(channel);
            }
        }
    }
    None
}

// the 24 bytes fl studio stores for one note in a pattern
fn note_bytes(channel: u16, start: u32, length: u32, key: u8, velocity: u8) -> Vec<u8> {
    let velocity = (velocity as u32 * 128 / 127) as u8;
    let mut out = Vec::with_capacity(info::NOTE_SIZE);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0x4000u16.to_le_bytes());
    out.extend_from_slice(&channel.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&(key as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[120, 0, 64, 0, 64, velocity, 128, 128]);
    out
}

// the bytes fl studio stores for one playlist clip, a pattern or an audio channel, with "whole clip" offsets
fn item_bytes(
    index: u16,
    start: u32,
    length: u32,
    track: u16,
    offset: [u8; 4],
    extra: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + extra.len());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&PATTERN_BASE.to_le_bytes());
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&(LAST_TRACK - track).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[120, 0]);
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&[64, 100, 128, 128]);
    out.extend_from_slice(&offset);
    out.extend_from_slice(&offset);
    out.extend_from_slice(extra);
    out
}

// the position of a playlist track's data event, tracks are numbered from 1
fn track_event(project: &Project, arrangement_start: usize, number: u32) -> Option<usize> {
    for i in arrangement_start + 1..project.events.len() {
        let event = &project.events[i];
        if event.id == events::ARRANGEMENT_NEW {
            return None;
        }
        if let (events::TRACK_DATA, Value::Data(data)) = (event.id, &event.value) {
            if data.len() >= 4 && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == number
            {
                return Some(i);
            }
        }
    }
    None
}

// gives a playlist track a name
fn name_track(project: &mut Project, arrangement_start: usize, number: u32, name: &str) {
    let Some(i) = track_event(project, arrangement_start, number) else {
        return;
    };
    let text = Value::Data(events::encode_text(name));
    let has_name = project
        .events
        .get(i + 1)
        .is_some_and(|e| e.id == events::TRACK_NAME);
    if has_name {
        project.events[i + 1].value = text;
    } else {
        insert_events(
            project,
            i + 1,
            vec![Event {
                id: events::TRACK_NAME,
                value: text,
            }],
        );
    }
}

// turns a playlist track off so it stays silent until the user enables it, byte 12 of its data is the switch
fn mute_track(project: &mut Project, arrangement_start: usize, number: u32) {
    let Some(i) = track_event(project, arrangement_start, number) else {
        return;
    };
    if let Value::Data(data) = &mut project.events[i].value {
        if data.len() > 12 {
            data[12] = 0;
        }
    }
}

// puts a list of events into the project at a position
fn insert_events(project: &mut Project, at: usize, new: Vec<Event>) {
    let rest = project.events.split_off(at);
    project.events.extend(new);
    project.events.extend(rest);
}
