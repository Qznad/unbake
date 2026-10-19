use crate::analysis::chords::{Chord, ChordSpan};
use crate::analysis::key::Key;
use crate::audio::math;
use crate::music::{BAR, Note, Part, STEP};

const SLOT: u32 = BAR / 2;
const CHANGE_ON_BAR: f32 = 0.15;
const CHANGE_MID_BAR: f32 = 0.35;

// the chords modern songs in a key mostly use, its own major and minor triads plus the usual borrowed ones
pub fn palette(key: &Key) -> Vec<Chord> {
    let degrees: &[(usize, bool)] = if key.minor {
        &[
            (0, true),
            (3, false),
            (5, true),
            (7, true),
            (7, false),
            (8, false),
            (10, false),
        ]
    } else {
        &[
            (0, false),
            (2, true),
            (4, true),
            (5, false),
            (5, true),
            (7, false),
            (9, true),
            (10, false),
        ]
    };
    degrees
        .iter()
        .map(|&(step, minor)| Chord {
            root: (key.tonic + step) % 12,
            minor,
        })
        .collect()
}

// the chord of every half bar from the notes heard, kept to the key and changing mostly on bar lines
pub fn progression(upper: &[Note], low: &[Note], key: &Key, bars: u32) -> Vec<ChordSpan> {
    let slots = (bars * 2) as usize;
    if slots == 0 {
        return Vec::new();
    }
    let upper_profiles = profiles(upper, slots);
    let low_profiles = profiles(low, slots);
    let chords = palette(key);
    let templates: Vec<[f32; 12]> = chords.iter().map(template).collect();

    let mut energy = Vec::with_capacity(slots);
    let mut fit = Vec::with_capacity(slots);
    for slot in 0..slots {
        let up = &upper_profiles[slot];
        let lo = &low_profiles[slot];
        let up_total = up.iter().sum::<f32>();
        let lo_total = lo.iter().sum::<f32>();
        energy.push(up_total + lo_total);
        let mut combined = [0.0f32; 12];
        for pc in 0..12 {
            combined[pc] = up[pc] / up_total.max(1e-9) + 0.5 * lo[pc] / lo_total.max(1e-9);
        }
        let row: Vec<f32> = templates
            .iter()
            .map(|t| math::cosine(&combined, t))
            .collect();
        fit.push(row);
    }

    let path = best_path(&fit);
    let loudest = energy.iter().cloned().fold(0.0f32, f32::max);
    let mut spans: Vec<ChordSpan> = Vec::new();
    for (slot, &choice) in path.iter().enumerate() {
        if energy[slot] < 0.1 * loudest {
            continue;
        }
        let start = slot as u32 * SLOT;
        let chord = chords[choice];
        if let Some(last) = spans.last_mut() {
            if last.chord == chord && last.start + last.length == start {
                last.length += SLOT;
                continue;
            }
        }
        spans.push(ChordSpan {
            chord,
            start,
            length: SLOT,
        });
    }
    spans
}

// how long each note name sounds in every half bar
fn profiles(notes: &[Note], slots: usize) -> Vec<[f32; 12]> {
    let mut out = vec![[0.0f32; 12]; slots];
    for note in notes {
        let end = note.start + note.length;
        let first = (note.start / SLOT) as usize;
        let last = (end.saturating_sub(1) / SLOT) as usize;
        for (slot, profile) in out
            .iter_mut()
            .enumerate()
            .take(last.min(slots - 1) + 1)
            .skip(first)
        {
            let from = note.start.max(slot as u32 * SLOT);
            let to = end.min((slot as u32 + 1) * SLOT);
            profile[note.key as usize % 12] += to.saturating_sub(from) as f32;
        }
    }
    out
}

// the root, third and fifth of a chord as a note name profile
fn template(chord: &Chord) -> [f32; 12] {
    let mut out = [0.0f32; 12];
    for pc in tones(chord) {
        out[pc] = 1.0;
    }
    out
}

// the note names of a chord, root first
fn tones(chord: &Chord) -> [usize; 3] {
    let third = if chord.minor { 3 } else { 4 };
    [chord.root, (chord.root + third) % 12, (chord.root + 7) % 12]
}

// the chord for every slot that fits best overall, where changing chord costs more off the bar line
fn best_path(fit: &[Vec<f32>]) -> Vec<usize> {
    let states = fit[0].len();
    let mut score = fit[0].clone();
    let mut back = vec![vec![0usize; states]; fit.len()];
    for slot in 1..fit.len() {
        let cost = if slot % 2 == 0 {
            CHANGE_ON_BAR
        } else {
            CHANGE_MID_BAR
        };
        let mut next = vec![0.0f32; states];
        for to in 0..states {
            let mut best = f32::MIN;
            for (from, &value) in score.iter().enumerate() {
                let value = value - if from == to { 0.0 } else { cost };
                if value > best {
                    best = value;
                    back[slot][to] = from;
                }
            }
            next[to] = best + fit[slot][to];
        }
        score = next;
    }

    let mut path = vec![0usize; fit.len()];
    let mut best = f32::MIN;
    for (state, &value) in score.iter().enumerate() {
        if value > best {
            best = value;
            path[fit.len() - 1] = state;
        }
    }
    for slot in (1..fit.len()).rev() {
        path[slot - 1] = back[slot][path[slot]];
    }
    path
}

// stacks each chord in close position, picking the inversion nearest the chord before so the voices move smoothly
pub fn voice(spans: &[ChordSpan]) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut previous: Option<[u8; 3]> = None;
    for span in spans {
        let pcs = tones(&span.chord);
        let mut best: Option<([u8; 3], u32)> = None;
        for low in 48u8..60 {
            if !pcs.contains(&(low as usize % 12)) {
                continue;
            }
            let mut keys = [low, 0, 0];
            let mut key = low;
            for i in 1..3 {
                key += 1;
                while !pcs.contains(&(key as usize % 12))
                    || keys[..i].iter().any(|&k| k % 12 == key % 12)
                {
                    key += 1;
                }
                keys[i] = key;
            }
            let cost = match previous {
                Some(before) => (0..3).map(|i| before[i].abs_diff(keys[i]) as u32).sum(),
                None => u32::from(low as usize % 12 != span.chord.root),
            };
            if best.is_none_or(|(_, c)| cost < c) {
                best = Some((keys, cost));
            }
        }
        let Some((keys, _)) = best else {
            continue;
        };
        for key in keys {
            notes.push(Note {
                part: Part::Chords,
                key,
                velocity: 90,
                start: span.start,
                length: span.length - STEP / 2,
            });
        }
        previous = Some(keys);
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    // a note of a part at a step, a beat long
    fn note(key: u8, start: u32, length: u32) -> Note {
        Note {
            part: Part::Chords,
            key,
            velocity: 90,
            start,
            length,
        }
    }

    // a minor key uses its own triads plus the major five
    #[test]
    fn minor_palette() {
        let key = Key {
            tonic: 9,
            minor: true,
            confidence: 1.0,
        };
        let names: Vec<String> = palette(&key).iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["Am", "C", "Dm", "Em", "E", "F", "G"]);
    }

    // held a minor, f, c and g triads with a stray note come out as one chord per bar
    #[test]
    fn finds_a_progression() {
        let key = Key {
            tonic: 9,
            minor: true,
            confidence: 1.0,
        };
        let mut upper = Vec::new();
        for (bar, keys) in [[57, 60, 64], [53, 57, 60], [55, 60, 64], [55, 59, 62]]
            .iter()
            .enumerate()
        {
            for &k in keys {
                upper.push(note(k, bar as u32 * BAR, BAR));
            }
        }
        upper.push(note(61, BAR / 2, STEP));
        let low = vec![
            note(45, 0, BAR),
            note(41, BAR, BAR),
            note(48, 2 * BAR, BAR),
            note(43, 3 * BAR, BAR),
        ];
        let spans = progression(&upper, &low, &key, 4);
        let names: Vec<String> = spans.iter().map(|s| s.chord.name()).collect();
        assert_eq!(names, vec!["Am", "F", "C", "G"]);
        assert!(spans.iter().all(|s| s.length == BAR));
    }

    // f major after a minor keeps the shared a and c in place and moves e up to f
    #[test]
    fn voices_move_smoothly() {
        let span = |root, minor, start| ChordSpan {
            chord: Chord { root, minor },
            start,
            length: BAR,
        };
        let notes = voice(&[span(9, true, 0), span(5, false, BAR)]);
        let keys: Vec<u8> = notes.iter().map(|n| n.key).collect();
        assert_eq!(keys, vec![57, 60, 64, 57, 60, 65]);
    }
}
