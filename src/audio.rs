pub fn samples_to_seconds(samples: u32, sample_rate: u32) -> f64 {
    samples as f64 / sample_rate as f64
}

pub fn seconds_to_samples(seconds: f64, sample_rate: u32) -> u32 {
    (seconds * sample_rate as f64).round() as u32
}

pub fn bpm_to_ms_per_beat(bpm: f64) -> f64 {
    60_000.0 / bpm
}

pub fn ms_per_step(bpm: f64) -> f64 {
    bpm_to_ms_per_beat(bpm) / 4.0
}

pub fn peak(samples: &[i16]) -> i32 {
    samples.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0)
}

pub fn to_float(samples: &[i16]) -> Vec<f32> {
    let mut float_samples = Vec::new();
    for &n in samples {
        float_samples.push(n as f32 / 32768.0);
    }
    float_samples
}
