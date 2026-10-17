use crate::audio::math;
use crate::audio::wav::{Wav, WavInfo};
use std::f64::consts::PI;

#[derive(Clone)]
pub struct Signal {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Signal {
    // mixes a wav down to one channel between -1 and 1
    pub fn from_wav(wav: &Wav) -> Signal {
        Signal {
            samples: math::to_mono(&wav.samples, wav.info.channels),
            sample_rate: wav.info.sample_rate,
        }
    }

    // a silent signal of a given length
    pub fn silence(seconds: f64, sample_rate: u32) -> Signal {
        let count = (seconds * sample_rate as f64).round() as usize;
        Signal {
            samples: vec![0.0; count],
            sample_rate,
        }
    }

    // turns the signal into a mono 16 bit wav
    pub fn to_wav(&self) -> Wav {
        let mut samples = Vec::with_capacity(self.samples.len());
        for &s in &self.samples {
            samples.push((s * 32767.0).round().clamp(-32768.0, 32767.0) as i16);
        }
        Wav {
            info: WavInfo {
                sample_rate: self.sample_rate,
                channels: 1,
                bits_per_sample: 16,
                num_frames: samples.len() as u32,
            },
            samples,
        }
    }

    // how long the signal lasts in seconds
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }

    // the loudest absolute sample value
    pub fn peak(&self) -> f32 {
        let mut peak = 0.0;
        for &s in &self.samples {
            if s.abs() > peak {
                peak = s.abs();
            }
        }
        peak
    }

    // a copy with every sample multiplied by gain
    pub fn scaled(&self, gain: f32) -> Signal {
        let mut samples = Vec::with_capacity(self.samples.len());
        for &s in &self.samples {
            samples.push(s * gain);
        }
        Signal {
            samples,
            sample_rate: self.sample_rate,
        }
    }

    // a copy turned up or down so the loudest sample is at 0.9
    pub fn normalized(&self) -> Signal {
        let peak = self.peak();
        if peak == 0.0 {
            return self.clone();
        }
        self.scaled(0.9 / peak)
    }

    // adds another signal on top of this one, sample by sample
    pub fn mix_in(&mut self, other: &Signal) {
        if other.samples.len() > self.samples.len() {
            self.samples.resize(other.samples.len(), 0.0);
        }
        for i in 0..other.samples.len() {
            self.samples[i] += other.samples[i];
        }
    }

    // the part between two times in seconds
    pub fn slice(&self, start: f64, end: f64) -> Signal {
        let rate = self.sample_rate as f64;
        let from = ((start * rate) as usize).min(self.samples.len());
        let to = ((end * rate) as usize).clamp(from, self.samples.len());
        Signal {
            samples: self.samples[from..to].to_vec(),
            sample_rate: self.sample_rate,
        }
    }

    // a lower sample rate copy, filtering out highs first so they don't fold back as noise
    pub fn downsample(&self, factor: usize) -> Signal {
        let taps = 63;
        let half = taps / 2;
        let cutoff = 0.45 / factor as f64;

        let mut kernel = Vec::with_capacity(taps);
        let mut total = 0.0;
        for i in 0..taps {
            let x = i as f64 - half as f64;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * x).sin() / (PI * x)
            };
            let window = 0.54 - 0.46 * (2.0 * PI * i as f64 / (taps - 1) as f64).cos();
            kernel.push(sinc * window);
            total += sinc * window;
        }

        let mut samples = Vec::with_capacity(self.samples.len() / factor + 1);
        let mut i = 0;
        while i < self.samples.len() {
            let mut sum = 0.0;
            for (k, weight) in kernel.iter().enumerate() {
                let j = i as isize + k as isize - half as isize;
                if j >= 0 && (j as usize) < self.samples.len() {
                    sum += self.samples[j as usize] as f64 * weight;
                }
            }
            samples.push((sum / total) as f32);
            i += factor;
        }

        Signal {
            samples,
            sample_rate: self.sample_rate / factor as u32,
        }
    }
}
