use crate::analysis::onsets;
use crate::analysis::spectrum::Spectrogram;
use crate::music::{Grid, Note, Part, STEP};

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub time: f64,
    pub part: Part,
    pub strength: f32,
}

const KICK_LOW: f32 = 0.8;
const SNARE_MID: f32 = 0.25;
const SNARE_HIGH: f32 = 0.25;
const HAT_HIGH: f32 = 0.1;

// finds kick, snare and hat hits in a drum stem by checking which frequency bands jump up
pub fn detect(spec: &Spectrogram) -> Vec<Hit> {
    let low = onsets::novelty(spec, 30.0, 120.0);
    let mid = onsets::novelty(spec, 150.0, 2500.0);
    let high = onsets::novelty(spec, 6000.0, 16000.0);

    let mut hits = Vec::new();
    for onset in onsets::detect(spec) {
        let l = around(&low, onset.frame);
        let m = around(&mid, onset.frame);
        let h = around(&high, onset.frame);

        if l >= KICK_LOW {
            hits.push(Hit {
                time: onset.time,
                part: Part::Kick,
                strength: l,
            });
        }
        let snare = m >= SNARE_MID && h >= SNARE_HIGH;
        if snare {
            hits.push(Hit {
                time: onset.time,
                part: Part::Snare,
                strength: m,
            });
        }
        if !snare && h >= HAT_HIGH {
            hits.push(Hit {
                time: onset.time,
                part: Part::Hat,
                strength: h,
            });
        }
    }
    hits
}

// the biggest value from one frame before to two frames after
fn around(values: &[f32], frame: usize) -> f32 {
    let mut best = 0.0f32;
    for f in frame.saturating_sub(1)..=(frame + 2).min(values.len() - 1) {
        best = best.max(values[f]);
    }
    best
}

// snaps hits to the nearest sixteenth step, with louder hits getting higher velocities
pub fn to_notes(hits: &[Hit], grid: &Grid) -> Vec<Note> {
    let mut loudest = [0.0f32; 3];
    for hit in hits {
        let i = drum_index(hit.part);
        loudest[i] = loudest[i].max(hit.strength);
    }

    let mut notes: Vec<Note> = Vec::new();
    for hit in hits {
        let Some(start) = grid.step_tick(hit.time) else {
            continue;
        };
        let level = hit.strength / loudest[drum_index(hit.part)].max(1e-6);
        notes.push(Note {
            part: hit.part,
            key: hit.part.drum_key(),
            velocity: (50.0 + 77.0 * level.min(1.0)) as u8,
            start,
            length: STEP,
        });
    }

    notes.sort_by_key(|n| (n.start, n.part));
    notes.dedup_by(|a, b| a.start == b.start && a.part == b.part);
    notes
}

// the position of a drum part in a list of three
fn drum_index(part: Part) -> usize {
    match part {
        Part::Kick => 0,
        Part::Snare => 1,
        _ => 2,
    }
}
