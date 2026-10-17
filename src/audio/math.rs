#![allow(dead_code)]

// converts a number of samples to seconds
pub fn samples_to_seconds(samples: u32, sample_rate: u32) -> f64 {
    samples as f64 / sample_rate as f64
}

// converts seconds to a number of samples
pub fn seconds_to_samples(seconds: f64, sample_rate: u32) -> u32 {
    (seconds * sample_rate as f64).round() as u32
}

// how long one beat lasts in milliseconds
pub fn bpm_to_ms_per_beat(bpm: f64) -> f64 {
    60_000.0 / bpm
}

// how long one step lasts in milliseconds, a step is a quarter of a beat
pub fn ms_per_step(bpm: f64) -> f64 {
    bpm_to_ms_per_beat(bpm) / 4.0
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

// changes the volume, 0.5 is half and 2.0 is double
pub fn apply_gain(samples: &[i16], gain: f32) -> Vec<i16> {
    let mut out = Vec::with_capacity(samples.len());
    for &s in samples {
        let scaled = (s as f32 * gain).round();
        out.push(scaled.clamp(-32768.0, 32767.0) as i16);
    }
    out
}

// converts samples to floats between -1 and 1
pub fn to_float(samples: &[i16]) -> Vec<f32> {
    let mut float_samples = Vec::new();
    for &n in samples {
        float_samples.push(n as f32 / 32768.0);
    }
    float_samples
}
