// some helpers are not used yet, this keeps the compiler quiet about them
#![allow(dead_code)]

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

// peak level in decibels below full scale, 0 is the loudest possible
pub fn peak_dbfs(peak: i32) -> f64 {
    if peak == 0 {
        return f64::NEG_INFINITY; // silence
    }
    20.0 * (peak as f64 / 32768.0).log10()
}

// averages the channels of each frame into one signal between -1 and 1
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

pub fn to_float(samples: &[i16]) -> Vec<f32> {
    let mut float_samples = Vec::new();
    for &n in samples {
        float_samples.push(n as f32 / 32768.0);
    }
    float_samples
}
