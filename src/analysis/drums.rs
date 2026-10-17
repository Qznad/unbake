use crate::analysis::onsets;
use crate::analysis::spectrum::Spectrogram;
use crate::audio::math;
use crate::music::{Grid, Note, Part, STEP};

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub time: f64,
    pub part: Part,
    pub strength: f32,
}

const CANDIDATE_FULL: f32 = 0.15;
const CANDIDATE_HIGH: f32 = 0.2;
const KICK_LOW: f32 = 0.6;
const SNARE_MID: f32 = 0.7;
const SNARE_HIGH: f32 = 0.1;
const SNARE_MID_OVER_LOW: f32 = 0.5;
const HAT_HIGH: f32 = 0.1;

// finds kick, snare and hat hits in a drum stem by checking which frequency bands jump up after each attack
pub fn detect(spec: &Spectrogram) -> Vec<Hit> {
    let frame_rate = spec.frame_rate();
    let low = scaled(onsets::novelty(spec, 30.0, 120.0));
    let mid = scaled(onsets::novelty(spec, 150.0, 2500.0));
    let high = scaled(onsets::novelty(spec, 6000.0, 16000.0));
    let full = scaled(onsets::novelty(spec, 30.0, 16000.0));

    let mut candidates: Vec<usize> = Vec::new();
    for onset in onsets::pick_peaks(&full, frame_rate, CANDIDATE_FULL) {
        candidates.push(onset.frame);
    }
    for onset in onsets::pick_peaks(&high, frame_rate, CANDIDATE_HIGH) {
        candidates.push(onset.frame);
    }
    candidates.sort();
    candidates.dedup_by(|later, earlier| *later - *earlier < 5);

    let mut hits = Vec::new();
    for frame in candidates {
        let time = frame as f64 / frame_rate;
        let l = around(&low, frame, 5);
        let m = around(&mid, frame, 3);
        let h = around(&high, frame, 2);

        if l >= KICK_LOW {
            hits.push(Hit {
                time,
                part: Part::Kick,
                strength: l,
            });
        }
        let snare = m >= SNARE_MID && h >= SNARE_HIGH && m >= SNARE_MID_OVER_LOW * l;
        if snare {
            hits.push(Hit {
                time,
                part: Part::Snare,
                strength: m,
            });
        }
        if !snare && h >= HAT_HIGH {
            hits.push(Hit {
                time,
                part: Part::Hat,
                strength: h,
            });
        }
    }
    hits
}

// divides a curve by its loud end so the same thresholds work for loud and quiet songs
fn scaled(curve: Vec<f32>) -> Vec<f32> {
    let top = math::percentile(&curve, 0.99).max(1e-9);
    curve.iter().map(|v| v / top).collect()
}

// the biggest value from one frame before to some frames after, lows need longer to build up than highs
fn around(values: &[f32], frame: usize, after: usize) -> f32 {
    let mut best = 0.0f32;
    for f in frame.saturating_sub(1)..=(frame + after).min(values.len() - 1) {
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
