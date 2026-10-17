use crate::audio::signal::Signal;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use std::f32::consts::PI;

pub const FRAME: usize = 2048;
pub const HOP: usize = 512;
pub const HARMONIC_FACTOR: usize = 4;

pub struct Spectrogram {
    pub frames: Vec<Vec<f32>>,
    pub frame_size: usize,
    pub hop: usize,
    pub sample_rate: u32,
}

impl Spectrogram {
    // the magnitude of every frequency in overlapping frames, frame i is centered on sample i * hop
    pub fn new(signal: &Signal, frame_size: usize, hop: usize) -> Spectrogram {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(frame_size);
        let window = hann(frame_size);
        let scale = 4.0 / frame_size as f32;

        let count = signal.samples.len() / hop + 1;
        let mut frames = Vec::with_capacity(count);
        let mut buffer = vec![Complex::new(0.0f32, 0.0f32); frame_size];

        for i in 0..count {
            let start = (i * hop) as isize - (frame_size / 2) as isize;
            for k in 0..frame_size {
                let pos = start + k as isize;
                let sample = if pos >= 0 && (pos as usize) < signal.samples.len() {
                    signal.samples[pos as usize]
                } else {
                    0.0
                };
                buffer[k] = Complex::new(sample * window[k], 0.0);
            }
            fft.process(&mut buffer);

            let mut magnitudes = Vec::with_capacity(frame_size / 2 + 1);
            for value in buffer.iter().take(frame_size / 2 + 1) {
                magnitudes.push(value.norm() * scale);
            }
            frames.push(magnitudes);
        }

        Spectrogram {
            frames,
            frame_size,
            hop,
            sample_rate: signal.sample_rate,
        }
    }

    // the usual spectrogram for drums, onsets and loudness
    pub fn standard(signal: &Signal) -> Spectrogram {
        Spectrogram::new(signal, FRAME, HOP)
    }

    // a spectrogram with finer frequency steps for notes and chords, at the same frame rate
    pub fn harmonic(signal: &Signal) -> Spectrogram {
        let low = signal.downsample(HARMONIC_FACTOR);
        Spectrogram::new(&low, FRAME, HOP / HARMONIC_FACTOR)
    }

    // the frequency in hz of a bin
    pub fn bin_hz(&self, bin: usize) -> f32 {
        bin as f32 * self.sample_rate as f32 / self.frame_size as f32
    }

    // the bin closest to a frequency
    pub fn hz_bin(&self, hz: f32) -> usize {
        let bin = (hz * self.frame_size as f32 / self.sample_rate as f32).round() as usize;
        bin.min(self.frame_size / 2)
    }

    // how many frames there are per second
    pub fn frame_rate(&self) -> f64 {
        self.sample_rate as f64 / self.hop as f64
    }

    // the frame closest to a time in seconds
    pub fn time_frame(&self, time: f64) -> usize {
        let frame = (time * self.frame_rate()).round().max(0.0) as usize;
        frame.min(self.frames.len().saturating_sub(1))
    }

    // the total energy between two frequencies in one frame
    pub fn band_energy(&self, frame: usize, low_hz: f32, high_hz: f32) -> f32 {
        let mut sum = 0.0;
        for bin in self.hz_bin(low_hz)..=self.hz_bin(high_hz) {
            let m = self.frames[frame][bin];
            sum += m * m;
        }
        sum
    }
}

// a window that fades each frame in and out so the edges don't click
fn hann(size: usize) -> Vec<f32> {
    let mut window = Vec::with_capacity(size);
    for i in 0..size {
        window.push(0.5 - 0.5 * (2.0 * PI * i as f32 / size as f32).cos());
    }
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    // a sine that sits exactly on bin 46 shows up there with a magnitude of about 1
    #[test]
    fn finds_a_sine() {
        let rate = 44100;
        let hz = 46.0 * rate as f32 / FRAME as f32;
        let mut samples = Vec::new();
        for i in 0..rate {
            samples.push((2.0 * PI * hz * i as f32 / rate as f32).sin());
        }
        let spec = Spectrogram::standard(&Signal {
            samples,
            sample_rate: rate as u32,
        });
        let frame = &spec.frames[40];
        let mut best = 0;
        for bin in 0..frame.len() {
            if frame[bin] > frame[best] {
                best = bin;
            }
        }
        assert_eq!(best, 46);
        assert!((frame[best] - 1.0).abs() < 0.05);
    }
}
