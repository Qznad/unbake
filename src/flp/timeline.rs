use crate::flp::events::{self, Project, Value};
use crate::flp::info::{self, NOTE_SIZE};
use std::collections::HashMap;

const PATTERN_BASE: u16 = 20480;
const LAST_TRACK: u16 = 499;

#[derive(Clone, Copy, Debug)]
pub struct AudioClip {
    pub channel: u16,
    pub start: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipSource {
    Pattern(u16),
    Channel(u16),
}

#[derive(Clone, Copy, Debug)]
pub struct Clip {
    pub source: ClipSource,
    pub start: u32,
    pub length: u32,
    pub track: u16,
    pub cut: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub number: u32,
    pub name: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TimedNote {
    pub channel: u16,
    pub start: u32,
    pub length: u32,
    pub key: u8,
    pub velocity: u8,
}

// every note the project plays, with each pattern's notes placed wherever the playlist puts that pattern
pub fn timeline(project: &Project) -> Vec<TimedNote> {
    let patterns = pattern_notes(project);
    let mut out = Vec::new();
    for clip in clips(project) {
        let ClipSource::Pattern(id) = clip.source else {
            continue;
        };
        let Some(notes) = patterns.get(&id) else {
            continue;
        };
        let from = clip.cut.unwrap_or(0);
        for note in notes {
            if note.start >= from && note.start < from + clip.length {
                out.push(TimedNote {
                    start: clip.start + note.start - from,
                    ..*note
                });
            }
        }
    }
    out.sort_by_key(|n| n.start);
    out
}

// every clip in the first playlist, patterns by number and audio or automation by channel, on tracks counted from 0, with where a cut pattern clip starts inside its pattern
pub fn clips(project: &Project) -> Vec<Clip> {
    let size = info::playlist_item_size(project);
    let Some(data) = project.events.iter().find_map(|e| match (e.id, &e.value) {
        (events::PLAYLIST, Value::Data(data)) => Some(data),
        _ => None,
    }) else {
        return Vec::new();
    };
    data.chunks_exact(size)
        .filter_map(|item| {
            let source = match u16::from_le_bytes([item[6], item[7]]) {
                index if index > PATTERN_BASE => ClipSource::Pattern(index - PATTERN_BASE),
                index if index < PATTERN_BASE => ClipSource::Channel(index),
                _ => return None,
            };
            let offset = i32::from_le_bytes([item[24], item[25], item[26], item[27]]);
            Some(Clip {
                source,
                start: u32::from_le_bytes([item[0], item[1], item[2], item[3]]),
                length: u32::from_le_bytes([item[8], item[9], item[10], item[11]]),
                track: LAST_TRACK.saturating_sub(u16::from_le_bytes([item[12], item[13]])),
                cut: match source {
                    ClipSource::Pattern(_) if offset >= 0 => Some(offset as u32),
                    _ => None,
                },
            })
        })
        .collect()
}

// the first arrangement's playlist tracks, numbered from 1, with their names and whether they are switched on
pub fn tracks(project: &Project) -> Vec<Track> {
    let mut out: Vec<Track> = Vec::new();
    let Some(start) = project.find(events::ARRANGEMENT_NEW) else {
        return out;
    };
    for event in &project.events[start + 1..] {
        match (event.id, &event.value) {
            (events::ARRANGEMENT_NEW, _) => break,
            (events::TRACK_DATA, Value::Data(data)) if data.len() > 12 => out.push(Track {
                number: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
                name: None,
                enabled: data[12] != 0,
            }),
            (events::TRACK_NAME, Value::Data(data)) => {
                if let Some(last) = out.last_mut() {
                    last.name = Some(events::decode_text(data)).filter(|n| !n.is_empty());
                }
            }
            _ => {}
        }
    }
    out
}

// the notes stored in each pattern, by pattern number, with times inside the pattern
pub fn pattern_notes(project: &Project) -> HashMap<u16, Vec<TimedNote>> {
    let mut patterns: HashMap<u16, Vec<TimedNote>> = HashMap::new();
    let mut current: Option<u16> = None;
    for event in &project.events {
        match (event.id, &event.value) {
            (events::PATTERN_NEW, Value::Word(id)) => current = Some(*id),
            (events::CHANNEL_NEW, _) | (events::ARRANGEMENT_NEW, _) => current = None,
            (events::PATTERN_NOTES, Value::Data(data)) => {
                let Some(id) = current else {
                    continue;
                };
                for note in data.chunks_exact(NOTE_SIZE) {
                    patterns.entry(id).or_default().push(TimedNote {
                        start: u32::from_le_bytes([note[0], note[1], note[2], note[3]]),
                        channel: u16::from_le_bytes([note[6], note[7]]),
                        length: u32::from_le_bytes([note[8], note[9], note[10], note[11]]),
                        key: u16::from_le_bytes([note[12], note[13]]).min(127) as u8,
                        velocity: ((note[21] as u32 * 127 + 64) / 128) as u8,
                    });
                }
            }
            _ => {}
        }
    }
    patterns
}

// every audio or automation clip in the playlist, with the channel it plays
pub fn audio_clips(project: &Project) -> Vec<AudioClip> {
    let mut out: Vec<AudioClip> = clips(project)
        .into_iter()
        .filter_map(|clip| match clip.source {
            ClipSource::Channel(channel) => Some(AudioClip {
                channel,
                start: clip.start,
                length: clip.length,
            }),
            ClipSource::Pattern(_) => None,
        })
        .collect();
    out.sort_by_key(|c| c.start);
    out
}

// how many different patterns the playlist actually uses
pub fn patterns_used(project: &Project) -> usize {
    clips(project)
        .iter()
        .filter_map(|clip| match clip.source {
            ClipSource::Pattern(id) => Some(id),
            ClipSource::Channel(_) => None,
        })
        .collect::<std::collections::HashSet<_>>()
        .len()
}
