use crate::analysis::onsets;
use crate::analysis::spectrum::Spectrogram;
use crate::audio::math;
use crate::audio::signal::Signal;
use crate::music::{Grid, Note, Part, STEP};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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
const BANDS: usize = 40;

pub type Bands = [f32; BANDS];

// finds kick, snare and hat hits in a drum stem by checking which frequency bands jump up after each attack
pub fn detect(spec: &Spectrogram) -> Vec<Hit> {
    let frame_rate = spec.frame_rate();
    let low = scaled(onsets::novelty(spec, 30.0, 120.0));
    let mid = scaled(onsets::novelty(spec, 150.0, 2500.0));
    let high = scaled(onsets::novelty(spec, 6000.0, 16000.0));

    let mut hits = Vec::new();
    for frame in attacks(spec) {
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

// the frames where any drum starts, from the whole spectrum and from the highs so quiet hats are not missed
fn attacks(spec: &Spectrogram) -> Vec<usize> {
    let frame_rate = spec.frame_rate();
    let full = scaled(onsets::novelty(spec, 30.0, 16000.0));
    let high = scaled(onsets::novelty(spec, 6000.0, 16000.0));
    let mut frames: Vec<usize> = Vec::new();
    for onset in onsets::pick_peaks(&full, frame_rate, CANDIDATE_FULL) {
        frames.push(onset.frame);
    }
    for onset in onsets::pick_peaks(&high, frame_rate, CANDIDATE_HIGH) {
        frames.push(onset.frame);
    }
    frames.sort();
    frames.dedup_by(|later, earlier| *later - *earlier < 5);
    frames
}

// the energy in 40 bands spaced like hearing, for every frame
pub fn band_frames(spec: &Spectrogram) -> Vec<Bands> {
    let mel = |hz: f32| 2595.0 * (1.0 + hz / 700.0).log10();
    let hz = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);
    let (low, high) = (mel(30.0), mel(16000.0));
    let mut edges = Vec::with_capacity(BANDS + 1);
    for i in 0..=BANDS {
        edges.push(spec.hz_bin(hz(low + (high - low) * i as f32 / BANDS as f32)));
    }

    let mut out = Vec::with_capacity(spec.frames.len());
    for frame in &spec.frames {
        let mut bands = [0.0f32; BANDS];
        for band in 0..BANDS {
            for bin in edges[band]..=edges[band + 1].max(edges[band]) {
                bands[band] += frame[bin];
            }
        }
        out.push(bands);
    }
    out
}

// what a drum sound looks like in the 40 bands just after it starts, scaled so the bands add up to 1
pub fn template(sound: &Signal) -> Bands {
    let frames = band_frames(&Spectrogram::standard(&sound.normalized()));
    let mut out = [0.0f32; BANDS];
    for frame in frames.iter().take(4).skip(1) {
        for band in 0..BANDS {
            out[band] += frame[band];
        }
    }
    let total: f32 = out.iter().sum::<f32>().max(1e-9);
    for value in out.iter_mut() {
        *value /= total;
    }
    out
}

// how much of each drum sound every frame holds, never below zero, by repeatedly nudging the amounts
pub fn activations(frames: &[Bands], templates: &[Bands]) -> Vec<Vec<f32>> {
    let n = templates.len();
    let mut gram = vec![vec![0.0f32; n]; n];
    for i in 0..n {
        for j in 0..n {
            gram[i][j] = (0..BANDS).map(|b| templates[i][b] * templates[j][b]).sum();
        }
    }

    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let wanted: Vec<f32> = templates
            .iter()
            .map(|t| (0..BANDS).map(|b| t[b] * frame[b]).sum())
            .collect();
        let mut amounts: Vec<f32> = (0..n).map(|i| wanted[i] / gram[i][i].max(1e-9)).collect();
        for _ in 0..30 {
            for i in 0..n {
                let made: f32 = (0..n).map(|j| gram[i][j] * amounts[j]).sum();
                amounts[i] *= wanted[i] / made.max(1e-9);
            }
        }
        out.push(amounts);
    }
    out
}

// finds hits by matching every attack against the given drum sounds and keeping the ones whose amount jumps up
pub fn detect_with_templates(spec: &Spectrogram, templates: &[(Part, Bands)]) -> Vec<Hit> {
    let frame_rate = spec.frame_rate();
    let shapes: Vec<Bands> = templates.iter().map(|(_, t)| *t).collect();
    let amounts = activations(&band_frames(spec), &shapes);

    let mut rises: Vec<Vec<f32>> = Vec::new();
    for (d, (part, _)) in templates.iter().enumerate() {
        let reach = if *part == Part::Kick { 5 } else { 2 };
        let mut rise = vec![0.0f32; amounts.len()];
        for f in 1..amounts.len() {
            let peak = (f..(f + reach + 1).min(amounts.len()))
                .map(|g| amounts[g][d])
                .fold(0.0f32, f32::max);
            rise[f] = (peak - amounts[f - 1][d]).max(0.0);
        }
        rises.push(scaled(rise));
    }

    let mut hits = Vec::new();
    for frame in attacks(spec) {
        for (d, (part, _)) in templates.iter().enumerate() {
            let strength = rises[d][frame];
            if strength >= match_threshold(*part) {
                hits.push(Hit {
                    time: frame as f64 / frame_rate,
                    part: *part,
                    strength,
                });
            }
        }
    }
    hits
}

// how big a jump in a drum's amount counts as a hit, hats are quiet so they need less
fn match_threshold(part: Part) -> f32 {
    match part {
        Part::Kick => 0.5,
        Part::Snare => 0.4,
        _ => 0.1,
    }
}

// joins both passes, kicks come from sound matching and snares and hats from either pass
pub fn merge_passes(first: &[Hit], matched: &[Hit]) -> Vec<Hit> {
    let mut out: Vec<Hit> = matched.to_vec();
    for hit in first {
        let matched_part = matched.iter().any(|m| m.part == hit.part);
        let already = matched
            .iter()
            .any(|m| m.part == hit.part && (m.time - hit.time).abs() < 0.03);
        let keep = !matched_part || (hit.part != Part::Kick && !already);
        if keep {
            out.push(*hit);
        }
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    // a hit at a time for a drum
    fn hit(time: f64, part: Part) -> Hit {
        Hit {
            time,
            part,
            strength: 1.0,
        }
    }

    // kicks only come from matching, while a snare found by just one pass is still kept
    #[test]
    fn merges_passes() {
        let first = vec![
            hit(1.0, Part::Kick),
            hit(2.0, Part::Snare),
            hit(3.0, Part::Snare),
        ];
        let matched = vec![hit(1.5, Part::Kick), hit(2.01, Part::Snare)];
        let merged = merge_passes(&first, &matched);
        let kicks: Vec<f64> = merged
            .iter()
            .filter(|h| h.part == Part::Kick)
            .map(|h| h.time)
            .collect();
        let snares: Vec<f64> = merged
            .iter()
            .filter(|h| h.part == Part::Snare)
            .map(|h| h.time)
            .collect();
        assert_eq!(kicks, vec![1.5]);
        assert_eq!(snares, vec![2.01, 3.0]);
    }
}
