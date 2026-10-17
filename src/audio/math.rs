// converts a number of samples to seconds
pub fn samples_to_seconds(samples: u32, sample_rate: u32) -> f64 {
    samples as f64 / sample_rate as f64
}

// finds the loudest sample, ignoring the sign
pub fn peak(samples: &[i16]) -> i32 {
    samples.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0)
}

// converts a peak to decibels, 0 is the loudest possible
pub fn peak_dbfs(peak: i32) -> f64 {
    if peak == 0 {
        return f64::NEG_INFINITY;
    }
    20.0 * (peak as f64 / 32768.0).log10()
}

// mixes all channels into one signal between -1 and 1
pub fn to_mono(samples: &[i16], channels: u16) -> Vec<f32> {
    let channels = channels as usize;
    let mut mono = Vec::with_capacity(samples.len() / channels);
    for frame in samples.chunks_exact(channels) {
        let mut sum: f32 = 0.0;
        for &s in frame {
            sum += s as f32;
        }
        mono.push(sum / channels as f32 / 32768.0);
    }
    mono
}

// converts decibels to a volume multiplier
pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

// the average of a list of numbers
pub fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

// the value below which a given fraction of the numbers fall, 0.5 is the median
pub fn percentile(values: &[f32], fraction: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let index = ((sorted.len() - 1) as f32 * fraction).round() as usize;
    sorted[index]
}

// how similar two lists are in direction, 1 is identical and 0 is unrelated
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

// how well two lists rise and fall together, from -1 to 1
pub fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mean_a = mean(&a[..n]);
    let mean_b = mean(&b[..n]);
    let mut centered_a = Vec::with_capacity(n);
    let mut centered_b = Vec::with_capacity(n);
    for i in 0..n {
        centered_a.push(a[i] - mean_a);
        centered_b.push(b[i] - mean_b);
    }
    cosine(&centered_a, &centered_b)
}

// the frequency in hz of a midi note, 69 is the a at 440 hz
pub fn midi_to_hz(key: f32) -> f32 {
    440.0 * 2f32.powf((key - 69.0) / 12.0)
}

// the midi note of a frequency, with decimals for notes in between
pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}
