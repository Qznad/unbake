use crate::analysis::spectrum::Spectrogram;

#[derive(Clone, Copy, Debug)]
pub struct Onset {
    pub time: f64,
    pub frame: usize,
}

// how much new sound appears in each frame between two frequencies, on a log scale
pub fn novelty(spec: &Spectrogram, low_hz: f32, high_hz: f32) -> Vec<f32> {
    let low = spec.hz_bin(low_hz);
    let high = spec.hz_bin(high_hz).max(low);
    let bins = (high - low + 1) as f32;

    let mut out = vec![0.0; spec.frames.len()];
    for i in 1..spec.frames.len() {
        let mut sum = 0.0;
        for bin in low..=high {
            let now = compress(spec.frames[i][bin]);
            let before = compress(spec.frames[i - 1][bin]);
            if now > before {
                sum += now - before;
            }
        }
        out[i] = sum / bins;
    }
    out
}

// squashes loud and quiet magnitudes closer together, like our ears do
fn compress(magnitude: f32) -> f32 {
    (1.0 + 100.0 * magnitude).ln()
}

// finds the frames where the novelty jumps up clearly compared to its surroundings
pub fn pick_peaks(novelty: &[f32], frame_rate: f64, min_strength: f32) -> Vec<Onset> {
    let min_gap = (0.05 * frame_rate).round() as usize;
    let mut onsets: Vec<Onset> = Vec::new();

    for i in 0..novelty.len() {
        let value = novelty[i];
        if value < min_strength || !is_local_max(novelty, i, 3) {
            continue;
        }
        if value < 1.5 * local_mean(novelty, i, 10) {
            continue;
        }
        if let Some(last) = onsets.last() {
            if i - last.frame < min_gap {
                continue;
            }
        }
        onsets.push(Onset {
            time: i as f64 / frame_rate,
            frame: i,
        });
    }
    onsets
}

// true when no value within reach frames on either side is bigger
fn is_local_max(values: &[f32], i: usize, reach: usize) -> bool {
    let from = i.saturating_sub(reach);
    let to = (i + reach).min(values.len() - 1);
    for j in from..=to {
        if values[j] > values[i] || (j < i && values[j] == values[i]) {
            return false;
        }
    }
    true
}

// the average value within reach frames on either side
fn local_mean(values: &[f32], i: usize, reach: usize) -> f32 {
    let from = i.saturating_sub(reach);
    let to = (i + reach).min(values.len() - 1);
    crate::audio::math::mean(&values[from..=to])
}

// finds onsets across the whole spectrum, measured against the stem's own strongest attacks
pub fn detect(spec: &Spectrogram) -> Vec<Onset> {
    let curve = novelty(spec, 30.0, 16000.0);
    let top = crate::audio::math::percentile(&curve, 0.99);
    if top < 1e-4 {
        return Vec::new();
    }
    let scaled: Vec<f32> = curve.iter().map(|v| v / top).collect();
    pick_peaks(&scaled, spec.frame_rate(), 0.15)
}

// how many onsets in a match ones in b within 50 ms, as precision, recall and their balance
pub fn f_measure(a: &[Onset], b: &[Onset]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut used = vec![false; b.len()];
    let mut matched = 0;
    for onset in a {
        for (j, other) in b.iter().enumerate() {
            if !used[j] && (onset.time - other.time).abs() <= 0.05 {
                used[j] = true;
                matched += 1;
                break;
            }
        }
    }
    let precision = matched as f64 / a.len() as f64;
    let recall = matched as f64 / b.len() as f64;
    if precision + recall == 0.0 {
        return 0.0;
    }
    2.0 * precision * recall / (precision + recall)
}
