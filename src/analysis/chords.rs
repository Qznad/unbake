use crate::analysis::chroma::{self, Chroma};
use crate::audio::math;
use crate::music::{BAR, Grid, NOTE_NAMES, Note, Part, STEP};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chord {
    pub root: usize,
    pub minor: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ChordSpan {
    pub chord: Chord,
    pub start: u32,
    pub length: u32,
}

impl Chord {
    // the chord written out, like am for a minor or f for f major
    pub fn name(&self) -> String {
        format!(
            "{}{}",
            NOTE_NAMES[self.root],
            if self.minor { "m" } else { "" }
        )
    }

    // the root, third and fifth stacked from the root, in the octave below middle c
    pub fn keys(&self) -> [u8; 3] {
        let root = 48 + self.root as u8;
        let third = if self.minor { 3 } else { 4 };
        [root, root + third, root + 7]
    }
}

// finds the chord of every half bar from the harmony with the bass helping pick the root
pub fn detect(
    harmony: &[Chroma],
    bass: &[Chroma],
    frame_rate: f64,
    grid: &Grid,
    bars: u32,
) -> Vec<ChordSpan> {
    let half = grid.bar_seconds() / 2.0;
    let mut found: Vec<Option<Chord>> = Vec::new();
    let mut energies = Vec::new();

    for slot in 0..bars * 2 {
        let start = grid.origin + slot as f64 * half;
        let upper = chroma::sum_between(harmony, frame_rate, start, start + half);
        let low = chroma::sum_between(bass, frame_rate, start, start + half);
        let upper_total = upper.iter().sum::<f32>();
        let low_total = low.iter().sum::<f32>();
        energies.push(upper_total);

        let mut combined = [0.0f32; 12];
        for pc in 0..12 {
            combined[pc] = upper[pc] / upper_total.max(1e-9) + 0.5 * low[pc] / low_total.max(1e-9);
        }
        found.push(best_chord(&combined));
    }

    let loudest = energies.iter().cloned().fold(0.0f32, f32::max);
    let mut spans: Vec<ChordSpan> = Vec::new();
    for (slot, chord) in found.iter().enumerate() {
        let Some(chord) = chord else {
            continue;
        };
        if energies[slot] < 0.1 * loudest {
            continue;
        }
        let start = slot as u32 * BAR / 2;
        if let Some(last) = spans.last_mut() {
            if last.chord == *chord && last.start + last.length == start {
                last.length += BAR / 2;
                continue;
            }
        }
        spans.push(ChordSpan {
            chord: *chord,
            start,
            length: BAR / 2,
        });
    }
    spans
}

// the major or minor triad that best fits a chroma, if any fits well enough
fn best_chord(chroma: &Chroma) -> Option<Chord> {
    let mut best: Option<Chord> = None;
    let mut best_score = 0.5;
    for root in 0..12 {
        for minor in [false, true] {
            let mut template = [0.0f32; 12];
            template[root] = 1.0;
            template[(root + if minor { 3 } else { 4 }) % 12] = 1.0;
            template[(root + 7) % 12] = 1.0;
            let score = math::cosine(chroma, &template);
            if score > best_score {
                best_score = score;
                best = Some(Chord { root, minor });
            }
        }
    }
    best
}

// turns chord spans into held notes, three per chord, ending a 32nd early so each chord is played again
pub fn to_notes(spans: &[ChordSpan]) -> Vec<Note> {
    let mut notes = Vec::new();
    for span in spans {
        for key in span.chord.keys() {
            notes.push(Note {
                part: Part::Chords,
                key,
                velocity: 90,
                start: span.start,
                length: span.length - STEP / 2,
            });
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    // the notes a, c and e with a little noise are heard as a minor
    #[test]
    fn a_minor() {
        let mut chroma = [0.05f32; 12];
        chroma[9] = 1.0;
        chroma[0] = 0.8;
        chroma[4] = 0.9;
        assert_eq!(best_chord(&chroma).unwrap().name(), "Am");
    }
}
