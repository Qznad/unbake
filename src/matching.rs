use crate::analysis::spectrum::Spectrogram;
use crate::audio::math;
use crate::audio::signal::Signal;
use crate::compare;
use crate::music::{Grid, Note, Part};
use crate::synth::{self, Sound, Voices, Wave};
use serde::Serialize;

const EXCERPT_SECONDS: f64 = 20.0;
const MAX_ROUNDS: usize = 8;

#[derive(Clone, Debug, Serialize)]
pub struct Matched {
    pub stem: String,
    pub score_before: f64,
    pub score_after: f64,
    pub renders: usize,
    pub voices: Voices,
}

// tunes the synth sounds of some parts so the render sounds more like the stem, keeping every change that helps
pub fn match_sounds(
    stem_name: &str,
    notes: &[Note],
    parts: &[Part],
    stem: &Signal,
    grid: &Grid,
    start: Voices,
) -> Matched {
    let (from, to) = busiest_window(notes, grid, stem.duration());
    let excerpt = stem.slice(from, to);
    let shifted = Grid {
        bpm: grid.bpm,
        origin: grid.origin - from,
    };
    let window_notes: Vec<Note> = notes
        .iter()
        .filter(|n| {
            let t = grid.tick_time(n.start);
            t >= from && t < to
        })
        .copied()
        .collect();

    let target = Spectrogram::standard(&excerpt.normalized());
    let active = compare::active_frames(&target);
    let target_profile = compare::band_profile(&target, &active);
    let target_levels = compare::frame_levels(&target);

    let mut renders = 0;
    let mut score = |voices: &Voices| -> f64 {
        renders += 1;
        let render = synth::render(&window_notes, &shifted, voices, to - from, stem.sample_rate);
        let spec = Spectrogram::standard(&render.normalized());
        let timbre = math::correlation(&target_profile, &compare::band_profile(&spec, &active));
        let envelope = math::correlation(&target_levels, &compare::frame_levels(&spec));
        (timbre.max(0.0) + envelope.max(0.0)) as f64 / 2.0
    };

    let mut best = start;
    let mut best_score = score(&best);
    let score_before = best_score;

    for _ in 0..MAX_ROUNDS {
        let mut improved = false;
        for &part in parts {
            for candidate in neighbours(best.get(part)) {
                let trial = best.with(part, candidate);
                let s = score(&trial);
                if s > best_score + 0.001 {
                    best = trial;
                    best_score = s;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }

    Matched {
        stem: stem_name.to_string(),
        score_before: (score_before * 1000.0).round() / 1000.0,
        score_after: (best_score * 1000.0).round() / 1000.0,
        renders,
        voices: best,
    }
}

// the stretch of up to 20 seconds where the most notes start
fn busiest_window(notes: &[Note], grid: &Grid, duration: f64) -> (f64, f64) {
    let length = EXCERPT_SECONDS.min(duration);
    let mut best = (0.0, length);
    let mut most = 0;
    for note in notes {
        let from = grid.tick_time(note.start).max(0.0);
        let to = from + length;
        let count = notes
            .iter()
            .filter(|n| {
                let t = grid.tick_time(n.start);
                t >= from && t < to
            })
            .count();
        if count > most {
            most = count;
            best = (from, to.min(duration));
        }
    }
    best
}

// small changes to try from a sound, other waveforms, and half or double the cutoff, release and volume
fn neighbours(sound: Sound) -> Vec<Sound> {
    let mut out = Vec::new();
    for wave in [Wave::Sine, Wave::Triangle, Wave::Saw, Wave::Square] {
        if wave != sound.wave {
            out.push(Sound { wave, ..sound });
        }
    }
    for factor in [0.5, 2.0] {
        out.push(Sound {
            cutoff: (sound.cutoff * factor).clamp(100.0, 16000.0),
            ..sound
        });
        out.push(Sound {
            release: (sound.release * factor).clamp(0.01, 2.0),
            ..sound
        });
        out.push(Sound {
            gain: (sound.gain * factor).clamp(0.05, 4.0),
            ..sound
        });
    }
    out.retain(|s| *s != sound);
    out
}
