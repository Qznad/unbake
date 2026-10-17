use crate::analysis::spectrum::Spectrogram;
use crate::audio::math;
use crate::audio::signal::Signal;

const WINDOW: usize = 512;
const THRESHOLD: f32 = 0.15;

#[derive(Clone, Copy, Debug)]
pub struct PitchFrame {
    pub hz: f32,
    pub clarity: f32,
    pub level: f32,
}

// estimates the main pitch of every frame with the yin method, and how loud the frame's middle is
pub fn track(signal: &Signal, hop: usize, low_hz: f32, high_hz: f32) -> Vec<PitchFrame> {
    let rate = signal.sample_rate as f32;
    let min_lag = (rate / high_hz).floor().max(2.0) as usize;
    let max_lag = (rate / low_hz).ceil() as usize;
    let needed = WINDOW + max_lag + 1;

    let count = signal.samples.len() / hop + 1;
    let mut frames = Vec::with_capacity(count);
    let mut buffer = vec![0.0f32; needed];
    let mut diff = vec![0.0f32; max_lag + 2];

    for i in 0..count {
        let start = (i * hop) as isize - (WINDOW / 2) as isize;
        for k in 0..needed {
            let pos = start + k as isize;
            buffer[k] = if pos >= 0 && (pos as usize) < signal.samples.len() {
                signal.samples[pos as usize]
            } else {
                0.0
            };
        }

        let mut energy = 0.0;
        for k in (WINDOW - hop) / 2..(WINDOW + hop) / 2 {
            energy += buffer[k] * buffer[k];
        }
        let level = (energy / hop as f32).sqrt();

        difference(&buffer, max_lag, &mut diff);
        normalize(&mut diff, max_lag);
        let (lag, clarity) = best_lag(&diff, min_lag, max_lag);

        frames.push(PitchFrame {
            hz: if lag > 0.0 { rate / lag } else { 0.0 },
            clarity,
            level,
        });
    }
    frames
}

// how different the window is from itself shifted by each lag, zero means it repeats exactly
fn difference(buffer: &[f32], max_lag: usize, diff: &mut [f32]) {
    diff[0] = 0.0;
    for lag in 1..=max_lag + 1 {
        let mut sum = 0.0;
        for k in 0..WINDOW {
            let d = buffer[k] - buffer[k + lag];
            sum += d * d;
        }
        diff[lag] = sum;
    }
}

// divides each difference by the average of the smaller lags so short lags don't win by default
fn normalize(diff: &mut [f32], max_lag: usize) {
    let mut running = 0.0;
    diff[0] = 1.0;
    for lag in 1..=max_lag + 1 {
        running += diff[lag];
        diff[lag] = if running > 0.0 {
            diff[lag] * lag as f32 / running
        } else {
            1.0
        };
    }
}

// the first clear dip below the threshold, refined between samples, with how clear it is
fn best_lag(diff: &[f32], min_lag: usize, max_lag: usize) -> (f32, f32) {
    let mut chosen = 0;
    let mut lag = min_lag;
    while lag <= max_lag {
        if diff[lag] < THRESHOLD {
            while lag < max_lag && diff[lag + 1] < diff[lag] {
                lag += 1;
            }
            chosen = lag;
            break;
        }
        lag += 1;
    }

    if chosen == 0 {
        let mut best = min_lag;
        for l in min_lag..=max_lag {
            if diff[l] < diff[best] {
                best = l;
            }
        }
        chosen = best;
    }

    let (a, b, c) = (diff[chosen - 1], diff[chosen], diff[chosen + 1]);
    let curvature = a - 2.0 * b + c;
    let shift = if curvature > 0.0 {
        (0.5 * (a - c) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    (chosen as f32 + shift, (1.0 - b).clamp(0.0, 1.0))
}

// finds the strongest note in a mix every frame, taking the octave above when it is nearly as strong
pub fn salience_track(spec: &Spectrogram, low_key: u8, high_key: u8) -> Vec<PitchFrame> {
    let mut frames = Vec::with_capacity(spec.frames.len());
    for frame in &spec.frames {
        let mut best_key = low_key;
        let mut best = 0.0f32;
        let mut total = 0.0f32;
        for key in low_key..=high_key {
            let s = harmonic_sum(spec, frame, key);
            total += s;
            if s > best {
                best = s;
                best_key = key;
            }
        }
        if best_key + 12 <= high_key && harmonic_sum(spec, frame, best_key + 12) > 0.6 * best {
            best_key += 12;
            best = harmonic_sum(spec, frame, best_key);
        }
        let average = total / (high_key - low_key + 1) as f32;
        let ratio = if average > 0.0 { best / average } else { 0.0 };
        frames.push(PitchFrame {
            hz: math::midi_to_hz(best_key as f32),
            clarity: if ratio > 1.0 { 1.0 - 1.0 / ratio } else { 0.0 },
            level: best,
        });
    }
    frames
}

// the spectrum at a note's first six harmonics, each one counting a little less than the one before
fn harmonic_sum(spec: &Spectrogram, frame: &[f32], key: u8) -> f32 {
    let f0 = math::midi_to_hz(key as f32);
    let mut sum = 0.0;
    let mut weight = 1.0;
    for harmonic in 1..=6 {
        let bin = spec.hz_bin(f0 * harmonic as f32);
        if bin + 1 >= frame.len() {
            break;
        }
        sum += weight * frame[bin - 1].max(frame[bin]).max(frame[bin + 1]);
        weight *= 0.8;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    // a 110 hz sine is tracked as 110 hz with high clarity
    #[test]
    fn tracks_a_sine() {
        let rate = 11025;
        let mut samples = Vec::new();
        for i in 0..rate {
            samples.push(0.5 * (2.0 * PI * 110.0 * i as f32 / rate as f32).sin());
        }
        let signal = Signal {
            samples,
            sample_rate: rate as u32,
        };
        let frames = track(&signal, 128, 30.0, 400.0);
        let middle = frames[frames.len() / 2];
        assert!((middle.hz - 110.0).abs() < 1.0, "got {}", middle.hz);
        assert!(middle.clarity > 0.9);
    }
}
