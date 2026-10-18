use crate::flp::events::{self, Project, Value};
use crate::flp::info::{self, NOTE_SIZE};
use std::collections::HashMap;

const PATTERN_BASE: u16 = 20480;

#[derive(Clone, Copy, Debug)]
pub struct AudioClip {
    pub channel: u16,
    pub start: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct TimedNote {
    pub channel: u16,
    pub start: u32,
    pub length: u32,
    pub key: u8,
}

// every note the project plays, with each pattern's notes placed wherever the playlist puts that pattern
pub fn timeline(project: &Project) -> Vec<TimedNote> {
    let patterns = pattern_notes(project);
    let size = info::playlist_item_size(project);
    let mut out = Vec::new();

    let Some(data) = project.events.iter().find_map(|e| match (e.id, &e.value) {
        (events::PLAYLIST, Value::Data(data)) => Some(data),
        _ => None,
    }) else {
        return out;
    };

    for item in data.chunks_exact(size) {
        let start = u32::from_le_bytes([item[0], item[1], item[2], item[3]]);
        let index = u16::from_le_bytes([item[6], item[7]]);
        let length = u32::from_le_bytes([item[8], item[9], item[10], item[11]]);
        let offset = i32::from_le_bytes([item[24], item[25], item[26], item[27]]);
        if index <= PATTERN_BASE {
            continue;
        }
        let Some(notes) = patterns.get(&(index - PATTERN_BASE)) else {
            continue;
        };
        let from = offset.max(0) as u32;
        for note in notes {
            if note.start >= from && note.start < from + length {
                out.push(TimedNote {
                    start: start + note.start - from,
                    ..*note
                });
            }
        }
    }
    out.sort_by_key(|n| n.start);
    out
}

// the notes stored in each pattern, by pattern number, with times inside the pattern
fn pattern_notes(project: &Project) -> HashMap<u16, Vec<TimedNote>> {
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
    let size = info::playlist_item_size(project);
    let mut out = Vec::new();
    for event in &project.events {
        if let (events::PLAYLIST, Value::Data(data)) = (event.id, &event.value) {
            for item in data.chunks_exact(size) {
                let index = u16::from_le_bytes([item[6], item[7]]);
                if index < PATTERN_BASE {
                    out.push(AudioClip {
                        channel: index,
                        start: u32::from_le_bytes([item[0], item[1], item[2], item[3]]),
                        length: u32::from_le_bytes([item[8], item[9], item[10], item[11]]),
                    });
                }
            }
            break;
        }
    }
    out.sort_by_key(|c| c.start);
    out
}

// how many different patterns the playlist actually uses
pub fn patterns_used(project: &Project) -> usize {
    let size = info::playlist_item_size(project);
    let mut used = std::collections::HashSet::new();
    for event in &project.events {
        if let (events::PLAYLIST, Value::Data(data)) = (event.id, &event.value) {
            for item in data.chunks_exact(size) {
                let index = u16::from_le_bytes([item[6], item[7]]);
                if index > PATTERN_BASE {
                    used.insert(index);
                }
            }
            break;
        }
    }
    used.len()
}
