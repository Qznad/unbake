use crate::music::{Note, PPQ};
use std::collections::HashMap;
use std::path::Path;

pub struct Track {
    pub name: String,
    pub channel: u8,
    pub notes: Vec<Note>,
}

pub struct Marker {
    pub tick: u32,
    pub name: String,
}

pub struct ReadTrack {
    pub notes: Vec<(u32, u32, u8)>,
}

struct Event {
    tick: u32,
    order: u8,
    data: Vec<u8>,
}

// saves a type 1 midi file, a tempo track followed by one track per part
pub fn write(path: &Path, bpm: f64, tracks: &[Track], markers: &[Marker]) -> Result<(), String> {
    std::fs::write(path, to_bytes(bpm, tracks, markers))
        .map_err(|e| format!("can't write {}: {}", path.display(), e))
}

// the bytes of a type 1 midi file
pub fn to_bytes(bpm: f64, tracks: &[Track], markers: &[Marker]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&((tracks.len() + 1) as u16).to_be_bytes());
    out.extend_from_slice(&(PPQ as u16).to_be_bytes());

    let micros = (60_000_000.0 / bpm).round() as u32;
    let mut tempo_track = vec![
        Event {
            tick: 0,
            order: 0,
            data: vec![
                0xFF,
                0x51,
                0x03,
                (micros >> 16) as u8,
                (micros >> 8) as u8,
                micros as u8,
            ],
        },
        Event {
            tick: 0,
            order: 0,
            data: vec![0xFF, 0x58, 0x04, 4, 2, 24, 8],
        },
    ];
    for marker in markers {
        tempo_track.push(Event {
            tick: marker.tick,
            order: 0,
            data: meta(0x06, marker.name.as_bytes()),
        });
    }
    out.extend(track_chunk(tempo_track));

    for track in tracks {
        let mut events = vec![Event {
            tick: 0,
            order: 0,
            data: meta(0x03, track.name.as_bytes()),
        }];
        for note in &track.notes {
            events.push(Event {
                tick: note.start,
                order: 1,
                data: vec![0x90 | track.channel, note.key, note.velocity.max(1)],
            });
            events.push(Event {
                tick: note.start + note.length,
                order: 0,
                data: vec![0x80 | track.channel, note.key, 0],
            });
        }
        out.extend(track_chunk(events));
    }
    out
}

// a meta event with a type byte and some text, like a track name or a marker
fn meta(kind: u8, text: &[u8]) -> Vec<u8> {
    let mut data = vec![0xFF, kind];
    push_varlen(&mut data, text.len() as u32);
    data.extend_from_slice(text);
    data
}

// a track chunk with events sorted by time, each stored as the time since the previous one
fn track_chunk(mut events: Vec<Event>) -> Vec<u8> {
    events.sort_by_key(|e| (e.tick, e.order));
    let mut body = Vec::new();
    let mut last = 0;
    for event in &events {
        push_varlen(&mut body, event.tick - last);
        body.extend_from_slice(&event.data);
        last = event.tick;
    }
    body.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

    let mut chunk = b"MTrk".to_vec();
    chunk.extend_from_slice(&(body.len() as u32).to_be_bytes());
    chunk.extend(body);
    chunk
}

// writes a number in groups of 7 bits, biggest first, with the top bit saying another group follows
fn push_varlen(out: &mut Vec<u8>, value: u32) {
    let mut groups = vec![(value & 0x7F) as u8];
    let mut rest = value >> 7;
    while rest > 0 {
        groups.push((rest & 0x7F) as u8 | 0x80);
        rest >>= 7;
    }
    groups.reverse();
    out.extend(groups);
}

// reads the notes (start, length, key) of every track in a midi file, with its ticks per beat
pub fn read(path: &Path) -> Result<(u16, Vec<ReadTrack>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {}", path.display(), e))?;
    if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
        return Err(format!("{} is not a midi file", path.display()));
    }
    let ppq = u16::from_be_bytes([bytes[12], bytes[13]]);
    let mut pos = 8 + u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;

    let mut tracks = Vec::new();
    while pos + 8 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let end = (pos + 8 + length).min(bytes.len());
        if &bytes[pos..pos + 4] == b"MTrk" {
            tracks.push(read_track(&bytes[pos + 8..end]));
        }
        pos = end;
    }
    Ok((ppq, tracks))
}

// the notes of one track chunk, pairing each note on with its note off
fn read_track(body: &[u8]) -> ReadTrack {
    let mut notes = Vec::new();
    let mut held: HashMap<u8, u32> = HashMap::new();
    let mut tick = 0u32;
    let mut status = 0u8;
    let mut i = 0;

    while i < body.len() {
        tick += read_varlen(body, &mut i);
        if i >= body.len() {
            break;
        }
        if body[i] & 0x80 != 0 {
            status = body[i];
            i += 1;
        }
        match status {
            0xFF => {
                i += 1;
                let length = read_varlen(body, &mut i) as usize;
                i = (i + length).min(body.len());
            }
            0xF0 | 0xF7 => {
                let length = read_varlen(body, &mut i) as usize;
                i += length;
            }
            s if s & 0xF0 == 0x80 || s & 0xF0 == 0x90 => {
                if i + 1 >= body.len() {
                    break;
                }
                let (key, velocity) = (body[i], body[i + 1]);
                i += 2;
                if s & 0xF0 == 0x90 && velocity > 0 {
                    held.insert(key, tick);
                } else if let Some(start) = held.remove(&key) {
                    notes.push((start, tick - start, key));
                }
            }
            s if s & 0xF0 == 0xC0 || s & 0xF0 == 0xD0 => i += 1,
            _ => i += 2,
        }
    }
    notes.sort();
    ReadTrack { notes }
}

// reads a number stored in groups of 7 bits, biggest first
fn read_varlen(bytes: &[u8], i: &mut usize) -> u32 {
    let mut value = 0u32;
    while *i < bytes.len() {
        let byte = bytes[*i];
        *i += 1;
        value = (value << 7) | (byte & 0x7F) as u32;
        if byte < 0x80 {
            break;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Part;

    // variable length numbers match the examples from the midi specification
    #[test]
    fn varlen_examples() {
        let cases: [(u32, Vec<u8>); 4] = [
            (0, vec![0x00]),
            (127, vec![0x7F]),
            (128, vec![0x81, 0x00]),
            (0x0FFF_FFFF, vec![0xFF, 0xFF, 0xFF, 0x7F]),
        ];
        for (value, expected) in cases {
            let mut out = Vec::new();
            push_varlen(&mut out, value);
            assert_eq!(out, expected);
        }
    }

    // a file with one note has the header, the tempo track and a note track, and reads back the same
    #[test]
    fn one_note_file() {
        let track = Track {
            name: "bass".to_string(),
            channel: 0,
            notes: vec![Note {
                part: Part::Bass,
                key: 45,
                velocity: 100,
                start: 0,
                length: 96,
            }],
        };
        let bytes = to_bytes(120.0, &[track], &[]);
        assert_eq!(&bytes[0..4], b"MThd");
        assert_eq!(&bytes[10..12], &[0, 2]);
        assert_eq!(&bytes[12..14], &[0, 96]);
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("MTrk").count(), 2);
        assert!(bytes.windows(3).any(|w| w == [0x90, 45, 100]));
        let path = std::env::temp_dir().join("unbake_midi_test.mid");
        std::fs::write(&path, &bytes).unwrap();
        let (ppq, tracks) = read(&path).unwrap();
        assert_eq!(ppq, 96);
        assert_eq!(tracks[1].notes, vec![(0, 96, 45)]);
        assert!(bytes.windows(4).any(|w| w == [0x60, 0x80, 45, 0]));
    }
}
