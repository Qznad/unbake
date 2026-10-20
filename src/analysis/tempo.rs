use crate::analysis::chroma::{self, Chroma};
use crate::analysis::onsets;
use crate::analysis::spectrum::Spectrogram;
use crate::audio::math;
use crate::audio::signal::Signal;
use crate::music::Grid;

#[derive(Clone, Copy, Debug)]
pub struct Guess {
    pub bpm: f64,
    pub score: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Tempo {
    pub bpm: f64,
    pub first_beat: f64,
}

impl Tempo {
    // how many seconds one beat lasts
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm
    }
}

// finds the tempo and bar grid of a song, from a signal with clear hits and one with the harmony, with the tempos it passed over
pub fn analyze(rhythm: &Signal, harmony: &Signal) -> (Tempo, Grid, Vec<Guess>) {
    let spec = Spectrogram::standard(&rhythm.normalized());
    let novelty = onsets::novelty(&spec, 30.0, 16000.0);
    let low = onsets::novelty(&spec, 30.0, 150.0);
    let (tempo, others) = estimate(&novelty, spec.frame_rate());

    let harmonic = Spectrogram::harmonic(&harmony.normalized());
    let chroma = chroma::chromagram(&harmonic);
    let grid = grid(&tempo, &low, &chroma, spec.frame_rate(), rhythm.duration());
    (tempo, grid, others)
}

// finds the tempo and where the beats fall from an onset novelty curve, with how well related tempos fit next to it
pub fn estimate(novelty: &[f32], frame_rate: f64) -> (Tempo, Vec<Guess>) {
    let curve = prepare(novelty);
    let coarse = best_related(&curve, frame_rate, autocorrelation_bpm(&curve, frame_rate));

    let mut best_bpm = coarse;
    let mut best_score = f64::MIN;
    let mut bpm = coarse * 0.97;
    while bpm <= coarse * 1.03 {
        let (score, _) = comb(&curve, frame_rate * 60.0 / bpm);
        if score > best_score {
            best_score = score;
            best_bpm = bpm;
        }
        bpm += 0.01;
    }

    let bpm = snap(best_bpm);
    let (_, phase) = comb(&curve, frame_rate * 60.0 / bpm);
    let fit = |bpm: f64| comb(&curve, frame_rate * 60.0 / bpm).0 * preference(bpm);
    let chosen = fit(bpm).max(1e-12);
    let mut others = vec![Guess { bpm, score: 1.0 }];
    for ratio in [2.0, 0.5, 1.5, 2.0 / 3.0] {
        let other = snap(bpm * ratio);
        if (30.0..=400.0).contains(&other) {
            others.push(Guess {
                bpm: other,
                score: fit(other) / chosen,
            });
        }
    }
    let tempo = Tempo {
        bpm,
        first_beat: phase / frame_rate,
    };
    (tempo, others)
}

// keeps only the parts of the curve that stick out above their surroundings, then smooths it
fn prepare(novelty: &[f32]) -> Vec<f32> {
    let mut peaks = Vec::with_capacity(novelty.len());
    for i in 0..novelty.len() {
        let from = i.saturating_sub(16);
        let to = (i + 16).min(novelty.len() - 1);
        let local = math::mean(&novelty[from..=to]);
        peaks.push((novelty[i] - local).max(0.0));
    }

    let mut smooth = peaks.clone();
    for _ in 0..2 {
        let mut next = smooth.clone();
        for i in 2..smooth.len().saturating_sub(2) {
            next[i] = (smooth[i - 2]
                + 4.0 * smooth[i - 1]
                + 6.0 * smooth[i]
                + 4.0 * smooth[i + 1]
                + smooth[i + 2])
                / 16.0;
        }
        smooth = next;
    }
    smooth
}

// the tempo whose beat spacing repeats most in the curve, between 60 and 200 bpm
fn autocorrelation_bpm(curve: &[f32], frame_rate: f64) -> f64 {
    let min_lag = (frame_rate * 60.0 / 200.0).floor() as usize;
    let max_lag = (frame_rate * 60.0 / 60.0).ceil() as usize;
    if curve.len() <= max_lag + 2 {
        return 120.0;
    }

    let mut scores = vec![0.0f64; max_lag + 2];
    for lag in (min_lag - 1)..=(max_lag + 1) {
        let mut sum = 0.0;
        for i in lag..curve.len() {
            sum += (curve[i] * curve[i - lag]) as f64;
        }
        scores[lag] = sum / (curve.len() - lag) as f64;
    }

    let mut best = min_lag;
    let mut best_score = f64::MIN;
    for (lag, &score) in scores.iter().enumerate().take(max_lag + 1).skip(min_lag) {
        let weighted = score * preference(60.0 * frame_rate / lag as f64);
        if weighted > best_score {
            best_score = weighted;
            best = lag;
        }
    }

    let (a, b, c) = (scores[best - 1], scores[best], scores[best + 1]);
    let curvature = a - 2.0 * b + c;
    let shift = if curvature != 0.0 {
        (0.5 * (a - c) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    60.0 * frame_rate / (best as f64 + shift)
}

// tries tempos related to a guess, double, half, three halves and two thirds, and keeps the best lined up one
fn best_related(curve: &[f32], frame_rate: f64, guess: f64) -> f64 {
    let mut best = guess;
    let mut best_score = f64::MIN;
    for ratio in [1.0, 2.0, 0.5, 1.5, 2.0 / 3.0] {
        let bpm = guess * ratio;
        if !(60.0..=200.0).contains(&bpm) {
            continue;
        }
        let (score, _) = comb(curve, frame_rate * 60.0 / bpm);
        let weighted = score * preference(bpm);
        if weighted > best_score {
            best_score = weighted;
            best = bpm;
        }
    }
    best
}

// how likely a tempo is before listening, highest at 140 bpm and halving about an octave away
fn preference(bpm: f64) -> f64 {
    let octaves = (bpm / 140.0).log2();
    (-0.5 * octaves * octaves).exp()
}

// how well the curve lines up with evenly spaced beats, and the offset that lines up best
fn comb(curve: &[f32], period: f64) -> (f64, f64) {
    let mut best_score = f64::MIN;
    let mut best_phase = 0.0;
    let end = (curve.len() - 1) as f64;

    let mut phase = 0.0;
    while phase < period {
        let mut sum = 0.0;
        let mut count = 0;
        let mut t = phase;
        while t < end {
            sum += sample(curve, t);
            count += 1;
            t += period;
        }
        let score = sum / count.max(1) as f64;
        if score > best_score {
            best_score = score;
            best_phase = phase;
        }
        phase += 0.5;
    }
    (best_score, best_phase)
}

// the curve value at a fractional frame, blending the two frames around it
fn sample(curve: &[f32], t: f64) -> f64 {
    let i = t.floor() as usize;
    let frac = t - i as f64;
    if i + 1 >= curve.len() {
        return curve[curve.len() - 1] as f64;
    }
    curve[i] as f64 * (1.0 - frac) + curve[i + 1] as f64 * frac
}

// rounds to a whole bpm when it is very close, most produced music uses whole numbers
fn snap(bpm: f64) -> f64 {
    if (bpm - bpm.round()).abs() < 0.1 {
        bpm.round()
    } else {
        (bpm * 100.0).round() / 100.0
    }
}

// decides which beat starts a bar by where chords change and low drums hit, and returns the bar grid
pub fn grid(
    tempo: &Tempo,
    low_novelty: &[f32],
    harmony: &[Chroma],
    frame_rate: f64,
    duration: f64,
) -> Grid {
    let beat = tempo.beat_seconds();
    let beats = ((duration - tempo.first_beat) / beat).floor().max(0.0) as usize;
    let low_max = math::percentile(low_novelty, 0.99).max(1e-6);

    let mut scores = [0.0f64; 4];
    for k in 0..beats {
        let t = tempo.first_beat + k as f64 * beat;
        let frame = (t * frame_rate).round() as usize;
        let mut low = 0.0f32;
        for f in frame.saturating_sub(2)..=(frame + 2).min(low_novelty.len().saturating_sub(1)) {
            low = low.max(low_novelty[f]);
        }
        let low = (low / low_max).min(1.0) as f64;

        let before = chroma::sum_between(harmony, frame_rate, t - beat, t);
        let after = chroma::sum_between(harmony, frame_rate, t, t + beat);
        let quiet = before.iter().sum::<f32>() < 1e-3 || after.iter().sum::<f32>() < 1e-3;
        let change = if quiet {
            0.0
        } else {
            1.0 - math::cosine(&before, &after) as f64
        };

        scores[k % 4] += low + 4.0 * change;
    }

    let mut best = 0;
    for b in 1..4 {
        if scores[b] > scores[best] {
            best = b;
        }
    }

    let downbeat = tempo.first_beat + best as f64 * beat;
    let bar = 4.0 * beat;
    Grid {
        bpm: tempo.bpm,
        origin: downbeat - (downbeat / bar).floor() * bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a click every 60 / 128 seconds is detected as 128 bpm with the first click as the first beat
    #[test]
    fn finds_128_bpm() {
        let frame_rate = 44100.0 / 512.0;
        let mut curve = vec![0.0f32; (frame_rate * 30.0) as usize];
        let mut t = 0.25f64;
        while t < 30.0 {
            curve[(t * frame_rate).round() as usize] = 1.0;
            t += 60.0 / 128.0;
        }
        let (tempo, guesses) = estimate(&curve, frame_rate);
        assert_eq!(tempo.bpm, 128.0);
        assert_eq!(guesses[0].bpm, 128.0);
        assert!(guesses[1..].iter().all(|g| g.score < 1.0));
        assert!((tempo.first_beat - 0.25).abs() < 0.03);
    }
}
