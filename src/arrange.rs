use crate::music::{BAR, Note, Part, STEP};
use std::collections::{HashMap, HashSet};

pub const PHRASE: u32 = BAR * 4;
const MAX_PATTERNS: usize = 4;
const SAME_LOOP: f32 = 0.5;

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

struct Phrase {
    start: u32,
    notes: Vec<Note>,
}

// turns each lane into a few 4 bar loops, like a producer would, using the notes most repetitions agree on
pub fn arrange(lanes: &[Lane]) -> Arrangement {
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut clips = Vec::new();

    for (track, lane) in lanes.iter().enumerate() {
        let phrases = strong_phrases(phrases(&lane.notes));
        let groups = group_phrases(&phrases);

        for (number, group) in groups.iter().enumerate() {
            let notes = consensus(&phrases, group);
            if notes.is_empty() {
                continue;
            }
            patterns.push(Pattern {
                name: format!("{} {}", lane.name, (b'A' + number as u8) as char),
                notes,
            });
            for &member in group {
                clips.push(Clip {
                    pattern: patterns.len() - 1,
                    track: track as u16,
                    start: phrases[member].start,
                    length: PHRASE,
                });
            }
        }
    }

    clips.sort_by_key(|c| (c.track, c.start));
    Arrangement { patterns, clips }
}

// plays the arrangement back as one list of notes for each lane
pub fn expand(arrangement: &Arrangement, lanes: usize) -> Vec<Vec<Note>> {
    let mut out = vec![Vec::new(); lanes];
    for clip in &arrangement.clips {
        for note in &arrangement.patterns[clip.pattern].notes {
            out[clip.track as usize].push(Note {
                start: clip.start + note.start,
                ..*note
            });
        }
    }
    for notes in out.iter_mut() {
        notes.sort_by_key(|n| (n.start, n.part, n.key));
    }
    out
}

// cuts notes into 4 bar phrases, splitting notes that cross into the next phrase
fn phrases(notes: &[Note]) -> Vec<Phrase> {
    let mut by_start: HashMap<u32, Vec<Note>> = HashMap::new();
    for note in notes {
        let mut start = note.start;
        let end = note.start + note.length;
        while start < end {
            let phrase = start / PHRASE * PHRASE;
            let piece_end = end.min(phrase + PHRASE);
            by_start.entry(phrase).or_default().push(Note {
                start: start - phrase,
                length: piece_end - start,
                ..*note
            });
            start = piece_end;
        }
    }
    let mut out: Vec<Phrase> = by_start
        .into_iter()
        .map(|(start, notes)| Phrase { start, notes })
        .collect();
    out.sort_by_key(|p| p.start);
    out
}

// drops phrases with far fewer notes than usual, those are mostly bleed from other instruments
fn strong_phrases(phrases: Vec<Phrase>) -> Vec<Phrase> {
    let mut counts: Vec<usize> = phrases.iter().map(|p| p.notes.len()).collect();
    counts.sort();
    let usual = counts.get(counts.len() / 2).copied().unwrap_or(0);
    phrases
        .into_iter()
        .filter(|p| p.notes.len() >= 2 && p.notes.len() * 4 >= usual)
        .collect()
}

// what a note is for comparing loops, its step, key and part, ignoring length and velocity
fn signature(note: &Note) -> (u32, u8, Part) {
    (note.start / STEP, note.key, note.part)
}

// how much two phrases have in common, from 0 for nothing to 1 for the same notes
fn similarity(a: &Phrase, b: &Phrase) -> f32 {
    let x: HashSet<_> = a.notes.iter().map(signature).collect();
    let y: HashSet<_> = b.notes.iter().map(signature).collect();
    if x.is_empty() && y.is_empty() {
        return 1.0;
    }
    2.0 * x.intersection(&y).count() as f32 / (x.len() + y.len()) as f32
}

// puts phrases that share at least half their notes together, then merges rare groups until a few are left
fn group_phrases(phrases: &[Phrase]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..phrases.len() {
        let mut best: Option<usize> = None;
        let mut best_similarity = SAME_LOOP;
        for (g, members) in groups.iter().enumerate() {
            let s = similarity(&phrases[i], &phrases[members[0]]);
            if s >= best_similarity {
                best_similarity = s;
                best = Some(g);
            }
        }
        match best {
            Some(g) => groups[g].push(i),
            None => groups.push(vec![i]),
        }
    }

    while groups.len() > MAX_PATTERNS {
        let mut smallest = 0;
        for g in 1..groups.len() {
            if groups[g].len() < groups[smallest].len() {
                smallest = g;
            }
        }
        let lonely = groups.remove(smallest);
        let mut target = 0;
        let mut target_similarity = -1.0;
        for (g, members) in groups.iter().enumerate() {
            let s = similarity(&phrases[lonely[0]], &phrases[members[0]]);
            if s > target_similarity {
                target_similarity = s;
                target = g;
            }
        }
        groups[target].extend(lonely);
    }

    for group in groups.iter_mut() {
        group.sort();
    }
    groups.sort_by_key(|g| g[0]);
    groups
}

// the notes that show up in at least half of a group's phrases, with their usual length and velocity
fn consensus(phrases: &[Phrase], group: &[usize]) -> Vec<Note> {
    let mut seen: HashMap<(u32, u8, Part), Vec<Note>> = HashMap::new();
    for &member in group {
        let mut in_this: HashSet<(u32, u8, Part)> = HashSet::new();
        for note in &phrases[member].notes {
            if in_this.insert(signature(note)) {
                seen.entry(signature(note)).or_default().push(*note);
            }
        }
    }

    let needed = group.len().div_ceil(2);
    let mut out = Vec::new();
    for ((step, key, part), mut notes) in seen {
        if notes.len() < needed {
            continue;
        }
        notes.sort_by_key(|n| n.length);
        let length = notes[notes.len() / 2].length;
        notes.sort_by_key(|n| n.velocity);
        let velocity = notes[notes.len() / 2].velocity;
        out.push(Note {
            part,
            key,
            velocity,
            start: step * STEP,
            length,
        });
    }
    out.sort_by_key(|n| (n.start, n.part, n.key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // a note at a step and key, a sixteenth long
    fn note(start: u32, key: u8) -> Note {
        Note {
            part: Part::Bass,
            key,
            velocity: 100,
            start,
            length: STEP,
        }
    }

    // three repeats of a loop with a stray note in one become one pattern without the stray note
    #[test]
    fn loops_ignore_stray_notes() {
        let mut notes = Vec::new();
        for phrase in 0..3 {
            let base = phrase * PHRASE;
            for step in [0, 4, 8, 12] {
                notes.push(note(base + step * STEP, 45));
            }
        }
        notes.push(note(PHRASE + 6 * STEP, 70));
        let lane = Lane {
            name: "Bass".to_string(),
            notes,
        };
        let arrangement = arrange(&[lane]);
        assert_eq!(arrangement.patterns.len(), 1);
        assert_eq!(arrangement.patterns[0].notes.len(), 4);
        assert_eq!(arrangement.clips.len(), 3);
        assert_eq!(arrangement.patterns[0].name, "Bass A");
    }

    // two different loops get two patterns, and expanding gives the notes back in place
    #[test]
    fn different_loops_and_expand() {
        let mut notes = Vec::new();
        for step in [0, 4, 8, 12] {
            notes.push(note(step * STEP, 45));
            notes.push(note(PHRASE + step * STEP, 41));
        }
        let lane = Lane {
            name: "Bass".to_string(),
            notes: notes.clone(),
        };
        let arrangement = arrange(&[lane]);
        assert_eq!(arrangement.patterns.len(), 2);
        let back = expand(&arrangement, 1);
        notes.sort_by_key(|n| (n.start, n.part, n.key));
        assert_eq!(back[0], notes);
    }

    // a note held into the next phrase is split at the phrase line
    #[test]
    fn splits_long_notes() {
        let held = Note {
            part: Part::Chords,
            key: 60,
            velocity: 90,
            start: PHRASE - 96,
            length: 192,
        };
        let cut = phrases(&[held]);
        assert_eq!(cut.len(), 2);
        assert_eq!(cut[0].notes[0].length, 96);
        assert_eq!(cut[1].start, PHRASE);
    }
}
