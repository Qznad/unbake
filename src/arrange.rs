use crate::music::{BAR, Note, STEP};

pub struct Lane {
    pub name: String,
    pub notes: Vec<Note>,
}

pub struct Pattern {
    pub name: String,
    pub notes: Vec<Note>,
}

pub struct Clip {
    pub pattern: usize,
    pub track: u16,
    pub start: u32,
    pub length: u32,
}

pub struct Arrangement {
    pub patterns: Vec<Pattern>,
    pub clips: Vec<Clip>,
}

// cuts each lane into one bar patterns and reuses a pattern wherever bars are exactly the same
pub fn arrange(lanes: &[Lane]) -> Arrangement {
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut clips = Vec::new();

    for (track, lane) in lanes.iter().enumerate() {
        let notes = split_at_bars(&lane.notes);
        let last_bar = notes.iter().map(|n| n.start / BAR).max();
        let Some(last_bar) = last_bar else {
            continue;
        };

        let first_pattern = patterns.len();
        let mut used = 0;
        for bar in 0..=last_bar {
            let mut inside: Vec<Note> = Vec::new();
            for note in &notes {
                if note.start / BAR == bar {
                    let mut moved = *note;
                    moved.start -= bar * BAR;
                    inside.push(moved);
                }
            }
            if inside.is_empty() {
                continue;
            }
            inside.sort_by_key(|n| (n.start, n.part, n.key));

            let existing =
                (first_pattern..patterns.len()).find(|&i| same(&patterns[i].notes, &inside));
            let pattern = match existing {
                Some(i) => i,
                None => {
                    used += 1;
                    patterns.push(Pattern {
                        name: format!("{} {}", lane.name, used),
                        notes: inside,
                    });
                    patterns.len() - 1
                }
            };
            clips.push(Clip {
                pattern,
                track: track as u16,
                start: bar * BAR,
                length: BAR,
            });
        }
    }

    Arrangement { patterns, clips }
}

// cuts notes that cross a bar line into one piece per bar
fn split_at_bars(notes: &[Note]) -> Vec<Note> {
    let mut out = Vec::new();
    for note in notes {
        let mut start = note.start;
        let end = note.start + note.length;
        while start < end {
            let bar_end = (start / BAR + 1) * BAR;
            let piece_end = end.min(bar_end);
            out.push(Note {
                start,
                length: piece_end - start,
                ..*note
            });
            start = piece_end;
        }
    }
    out
}

// true when two bars play the same notes at the same steps, ignoring velocity and tiny length differences
fn same(a: &[Note], b: &[Note]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.part == y.part
                && x.key == y.key
                && x.start == y.start
                && x.length.abs_diff(y.length) <= STEP / 4
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Part;

    // two identical bars share one pattern and a different third bar gets its own
    #[test]
    fn reuses_identical_bars() {
        let note = |start: u32, key: u8| Note {
            part: Part::Bass,
            key,
            velocity: 100,
            start,
            length: 48,
        };
        let lane = Lane {
            name: "Bass".to_string(),
            notes: vec![note(0, 45), note(BAR, 45), note(2 * BAR, 41)],
        };
        let arrangement = arrange(&[lane]);
        assert_eq!(arrangement.patterns.len(), 2);
        assert_eq!(arrangement.clips.len(), 3);
        assert_eq!(arrangement.clips[1].pattern, 0);
        assert_eq!(arrangement.clips[2].pattern, 1);
    }

    // a note held across a bar line becomes two notes, one in each bar
    #[test]
    fn splits_long_notes() {
        let held = Note {
            part: Part::Chords,
            key: 60,
            velocity: 90,
            start: BAR - 96,
            length: 192,
        };
        let pieces = split_at_bars(&[held]);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].length, 96);
        assert_eq!(pieces[1].start, BAR);
    }
}
