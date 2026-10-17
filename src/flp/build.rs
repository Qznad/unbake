use crate::arrange::Arrangement;
use crate::flp::events::{self, Event, Project, Value};
use crate::flp::info::{self, Channel};
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
    pub warnings: Vec<String>,
}

// fills a copy of a template project with our tempo, patterns, playlist clips, markers and track names
pub fn build(
    template: &Project,
    arrangement: &Arrangement,
    track_names: &[String],
    bpm: f64,
    markers: &[Marker],
) -> Result<Built, String> {
    let mut project = template.clone();
    let mut warnings = Vec::new();
    let summary = info::summarize(template);
    let ppq = template.ppq as u64;
    let scale = |tick: u32| (tick as u64 * ppq / PPQ as u64) as u32;

    set_tempo(&mut project, bpm);

    let mut channels: Vec<(Part, u16)> = Vec::new();
    for part in PARTS {
        let used = arrangement
            .patterns
            .iter()
            .any(|p| p.notes.iter().any(|n| n.part == part));
        match find_channel(&summary.channels, part) {
            Some(id) => channels.push((part, id)),
            None if used => warnings.push(format!(
                "the template has no channel with \"{}\" in its name, those notes were left out",
                part.name()
            )),
            None => {}
        }
    }

    let first_id = summary.patterns.iter().map(|p| p.id).max().unwrap_or(0) + 1;
    let mut pattern_events = Vec::new();
    for (i, pattern) in arrangement.patterns.iter().enumerate() {
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
        pattern_events.push(Event {
            id: events::PATTERN_NEW,
            value: Value::Word(first_id + i as u16),
        });
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

    let mut items = Vec::new();
    for clip in &arrangement.clips {
        items.extend(item_bytes(
            first_id + clip.pattern as u16,
            scale(clip.start),
            scale(clip.length),
            clip.track,
            &extra,
        ));
    }
    project.events[playlist].value = Value::Data(items);

    let mut marker_events = Vec::new();
    for marker in markers {
        marker_events.push(Event {
            id: events::MARKER_POSITION,
            value: Value::Dword(scale(marker.tick)),
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

    Ok(Built { project, warnings })
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

// the channel whose name suggests a part, like a channel called "808 Kick" for the kick
fn find_channel(channels: &[Channel], part: Part) -> Option<u16> {
    let words: &[&str] = match part {
        Part::Kick => &["kick"],
        Part::Snare => &["snare", "clap"],
        Part::Hat => &["hat", "hh"],
        Part::Bass => &["bass"],
        Part::Melody => &["lead", "melody", "pluck"],
        Part::Chords => &["chord", "pad", "keys", "piano"],
    };
    for channel in channels {
        let name = channel.name.to_lowercase();
        if words.iter().any(|w| name.contains(w)) {
            return Some(channel.id);
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

// the bytes fl studio stores for one pattern clip in the playlist
fn item_bytes(pattern: u16, start: u32, length: u32, track: u16, extra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + extra.len());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&PATTERN_BASE.to_le_bytes());
    out.extend_from_slice(&(PATTERN_BASE + pattern).to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&(LAST_TRACK - track).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[120, 0]);
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&[64, 100, 128, 128]);
    out.extend_from_slice(&(-1i32).to_le_bytes());
    out.extend_from_slice(&(-1i32).to_le_bytes());
    out.extend_from_slice(extra);
    out
}

// gives a playlist track a name, tracks are numbered from 1 in their data
fn name_track(project: &mut Project, arrangement_start: usize, number: u32, name: &str) {
    let mut i = arrangement_start + 1;
    while i < project.events.len() {
        let event = &project.events[i];
        if event.id == events::ARRANGEMENT_NEW {
            return;
        }
        if event.id == events::TRACK_DATA {
            if let Value::Data(data) = &event.value {
                if data.len() >= 4
                    && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == number
                {
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
                    return;
                }
            }
        }
        i += 1;
    }
}

// puts a list of events into the project at a position
fn insert_events(project: &mut Project, at: usize, new: Vec<Event>) {
    let rest = project.events.split_off(at);
    project.events.extend(new);
    project.events.extend(rest);
}
