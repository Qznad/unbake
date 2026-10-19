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

// splits notes into the highest line, as the melody, and the rest, as chords, with block chords kept whole
pub fn split_top(notes: &[Note]) -> (Vec<Note>, Vec<Note>) {
    let mut melody = Vec::new();
    let mut chords = Vec::new();
    for note in notes {
        let covered = notes.iter().any(|other| {
            other.key > note.key
                && other.start <= note.start
                && other.start + other.length > note.start
        });
        let voiced = notes
            .iter()
            .filter(|other| {
                other.key < note.key
                    && other.start.abs_diff(note.start) <= STEP
                    && (other.start + other.length).abs_diff(note.start + note.length) <= STEP
            })
            .count()
            >= 2;
        if covered || voiced {
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

    // a line over a held chord splits off it, a lone triad stays whole, and a two note pair keeps its top as melody
    #[test]
    fn splits_the_top_line() {
        let note = |key, start, length| Note {
            part: Part::Chords,
            key,
            velocity: 100,
            start,
            length,
        };
        let notes = vec![
            note(60, 0, 96),
            note(64, 0, 96),
            note(72, 0, 48),
            note(71, 48, 48),
            note(65, 192, 96),
            note(71, 192, 96),
            note(57, 384, 384),
            note(60, 384, 384),
            note(64, 384, 384),
        ];
        let (melody, chords) = split_top(&notes);
        let keys = |n: &[Note]| n.iter().map(|n| n.key).collect::<Vec<_>>();
        assert_eq!(keys(&melody), vec![72, 71, 71]);
        assert_eq!(keys(&chords), vec![60, 64, 65, 57, 60, 64]);
        assert!(melody.iter().all(|n| n.part == Part::Melody));
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
