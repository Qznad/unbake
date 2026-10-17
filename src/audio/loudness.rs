use crate::audio::signal::Signal;
use std::f64::consts::PI;

struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    // runs the filter over a list of samples
    fn process(&self, input: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(input.len());
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for &x in input {
            let y = self.b0 * x + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y;
            out.push(y);
        }
        out
    }
}

// how loud a signal sounds in lufs, measured the way streaming services do it (ebu r128)
pub fn lufs(signal: &Signal) -> f64 {
    let filtered = k_weight(signal);
    let rate = signal.sample_rate as usize;
    let block = rate * 4 / 10;
    let step = rate / 10;

    let mut powers = Vec::new();
    let mut start = 0;
    while start + block <= filtered.len() {
        let mut sum = 0.0;
        for &s in &filtered[start..start + block] {
            sum += s * s;
        }
        powers.push(sum / block as f64);
        start += step;
    }

    let loudness = |power: f64| -0.691 + 10.0 * power.log10();
    let loud_enough: Vec<f64> = powers
        .into_iter()
        .filter(|&p| p > 0.0 && loudness(p) > -70.0)
        .collect();
    if loud_enough.is_empty() {
        return f64::NEG_INFINITY;
    }
    let relative_gate = loudness(average(&loud_enough)) - 10.0;
    let gated: Vec<f64> = loud_enough
        .into_iter()
        .filter(|&p| loudness(p) > relative_gate)
        .collect();
    loudness(average(&gated))
}

// the average of a list of numbers
fn average(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

// boosts the highs a little and cuts the deep lows, to match how loud our ears find each frequency
fn k_weight(signal: &Signal) -> Vec<f64> {
    let rate = signal.sample_rate as f64;

    let (f0, gain_db, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };

    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let low_cut = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };

    let input: Vec<f64> = signal.samples.iter().map(|&s| s as f64).collect();
    low_cut.process(&shelf.process(&input))
}

#[cfg(test)]
mod tests {
    use super::*;

    // a full scale 1 khz sine measures -3.01 lufs, the reference value from the standard
    #[test]
    fn reference_sine() {
        let rate = 48000;
        let mut samples = Vec::new();
        for i in 0..rate * 5 {
            samples.push((2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin());
        }
        let value = lufs(&Signal {
            samples,
            sample_rate: rate as u32,
        });
        assert!((value + 3.01).abs() < 0.05, "got {}", value);
    }
}
