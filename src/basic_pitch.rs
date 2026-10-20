use crate::music::{Grid, Note, Part, STEP};
use std::path::Path;
use std::process::Command;

pub struct Event {
    pub start: f64,
    pub end: f64,
    pub key: u8,
    pub velocity: u8,
}

// runs basic pitch on a stem and reads back the notes it heard
pub fn transcribe(stem: &Path, work: &Path) -> Result<Vec<Event>, String> {
    let name = stem
        .file_stem()
        .ok_or("the stem path has no file name".to_string())?
        .to_string_lossy();
    let csv = work.join(format!("{}_basic_pitch.csv", name));
    std::fs::create_dir_all(work).map_err(|e| format!("can't create {}: {}", work.display(), e))?;
    for extension in ["csv", "mid"] {
        let _ = std::fs::remove_file(work.join(format!("{}_basic_pitch.{}", name, extension)));
    }

    let output = Command::new("basic-pitch")
        .env("PYTHONIOENCODING", "utf-8")
        .arg("--save-note-events")
        .arg(work)
        .arg(stem)
        .output()
        .map_err(|_| {
            "basic-pitch was not found, install it with \
             `uv tool install --python 3.10 basic-pitch --with \"setuptools<81\"`"
                .to_string()
        })?;
    if !output.status.success() || !csv.exists() {
        let said = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        let last = said.lines().map(str::trim).rfind(|l| !l.is_empty());
        return Err(format!(
            "basic-pitch failed on {} ({})",
            stem.display(),
            last.unwrap_or("no message")
        ));
    }
    read_csv(&csv)
}

// reads the start, end, note and velocity of every row basic pitch saved
fn read_csv(path: &Path) -> Result<Vec<Event>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("can't read {}: {}", path.display(), e))?;
    let mut events = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 4 {
            continue;
        }
        let (Ok(start), Ok(end), Ok(key), Ok(velocity)) = (
            fields[0].trim().parse::<f64>(),
            fields[1].trim().parse::<f64>(),
            fields[2].trim().parse::<u8>(),
            fields[3].trim().parse::<u8>(),
        ) else {
            continue;
        };
        events.push(Event {
            start,
            end,
            key,
            velocity,
        });
    }
    Ok(events)
}

// snaps events onto the step grid as notes of one part, keeping one note per key and step
pub fn to_notes(events: &[Event], grid: &Grid, part: Part) -> Vec<Note> {
    let mut notes = Vec::new();
    for event in events {
        let Some(start) = grid.step_tick(event.start) else {
            continue;
        };
        let end = grid.step_tick(event.end).unwrap_or(start);
        notes.push(Note {
            part,
            key: event.key,
            velocity: event.velocity.clamp(40, 127),
            start,
            length: end.saturating_sub(start).max(STEP),
        });
    }
    notes.sort_by_key(|n| (n.start, n.key, std::cmp::Reverse(n.length)));
    notes.dedup_by_key(|n| (n.start, n.key));
    notes
}

// keeps the lowest note starting on each step, so a single line like a bass loses the overtones heard above it
pub fn lowest_line(notes: &[Note]) -> Vec<Note> {
    let mut line = notes.to_vec();
    line.sort_by_key(|n| (n.start, n.key));
    line.dedup_by_key(|n| n.start);
    line
}

// the other notes starting on the same step as one note, with their positions
fn together(notes: &[Note], i: usize) -> impl Iterator<Item = (usize, &Note)> {
    notes
        .iter()
        .enumerate()
        .filter(move |&(j, other)| j != i && other.start == notes[i].start)
}

// for every note, whether it belongs to a held block chord, two or more others starting and ending with it
pub fn block_chords(notes: &[Note]) -> Vec<bool> {
    (0..notes.len())
        .map(|i| {
            let end = notes[i].start + notes[i].length;
            together(notes, i)
                .filter(|(_, other)| (other.start + other.length).abs_diff(end) <= STEP)
                .count()
                >= 2
        })
        .collect()
}

// splits notes into the melody, the loudest one starting on each step, and chords, the rest and any block chords
pub fn split_lead(notes: &[Note]) -> (Vec<Note>, Vec<Note>) {
    let block = block_chords(notes);

    let mut melody: Vec<Note> = Vec::new();
    let mut chords = Vec::new();
    for (i, note) in notes.iter().enumerate() {
        let topped = together(notes, i)
            .any(|(j, other)| !block[j] && (other.velocity, other.key) > (note.velocity, note.key));
        if block[i] || topped {
            chords.push(Note {
                part: Part::Chords,
                ..*note
            });
        } else {
            melody.push(Note {
                part: Part::Melody,
                ..*note
            });
        }
    }
    (melody, chords)
}

#[cfg(test)]
mod tests {
    use super::*;

    // the loudest note on each step is the melody, even under a chord, and a louder held triad doesn't drown a quiet lead
    #[test]
    fn splits_the_loudest_line() {
        let note = |key, velocity, start, length| Note {
            part: Part::Chords,
            key,
            velocity,
            start,
            length,
        };
        let notes = vec![
            note(55, 100, 0, 48),
            note(60, 70, 0, 96),
            note(64, 70, 0, 96),
            note(71, 90, 48, 48),
            note(65, 80, 192, 96),
            note(71, 80, 192, 96),
            note(57, 90, 384, 384),
            note(60, 100, 384, 384),
            note(64, 90, 384, 384),
            note(76, 60, 384, 96),
        ];
        let (melody, chords) = split_lead(&notes);
        let keys = |n: &[Note]| n.iter().map(|n| n.key).collect::<Vec<_>>();
        assert_eq!(keys(&melody), vec![55, 71, 71, 76]);
        assert_eq!(keys(&chords), vec![60, 64, 65, 57, 60, 64]);
        assert!(melody.iter().all(|n| n.part == Part::Melody));
    }

    // a line that jumps between registers stays one melody even when its notes ring over each other
    #[test]
    fn keeps_a_jumping_line_whole() {
        let note = |key, start| Note {
            part: Part::Chords,
            key,
            velocity: 100,
            start,
            length: 96,
        };
        let notes = vec![note(46, 0), note(62, 24), note(50, 48), note(72, 72)];
        let (melody, chords) = split_lead(&notes);
        assert!(chords.is_empty());
        assert_eq!(
            melody.iter().map(|n| n.key).collect::<Vec<_>>(),
            vec![46, 62, 50, 72]
        );
        assert!(melody.iter().all(|n| n.length == 96));
    }

    // an octave and a fifth heard over each bass note are dropped, leaving the bass line
    #[test]
    fn keeps_the_lowest_line() {
        let note = |key, start| Note {
            part: Part::Bass,
            key,
            velocity: 100,
            start,
            length: STEP,
        };
        let notes = vec![
            note(48, 0),
            note(60, 0),
            note(43, 24),
            note(50, 24),
            note(55, 24),
        ];
        let keys: Vec<u8> = lowest_line(&notes).iter().map(|n| n.key).collect();
        assert_eq!(keys, vec![48, 43]);
    }

    // events become notes snapped to sixteenth steps, at least one step long
    #[test]
    fn snaps_events_to_steps() {
        let grid = Grid {
            bpm: 120.0,
            origin: 0.0,
        };
        let events = vec![Event {
            start: 0.13,
            end: 0.14,
            key: 48,
            velocity: 20,
        }];
        let notes = to_notes(&events, &grid, Part::Bass);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, STEP);
        assert_eq!(notes[0].length, STEP);
        assert_eq!(notes[0].velocity, 40);
    }
}
