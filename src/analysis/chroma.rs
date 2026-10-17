use crate::analysis::spectrum::Spectrogram;
use crate::audio::math;

pub type Chroma = [f32; 12];

// how strongly each of the 12 pitch classes sounds in every frame, c is index 0
pub fn chromagram(spec: &Spectrogram) -> Vec<Chroma> {
    let low = spec.hz_bin(55.0).max(1);
    let high = spec.hz_bin(2000.0);

    let mut classes = Vec::with_capacity(high + 1);
    for bin in 0..=high {
        let key = math::hz_to_midi(spec.bin_hz(bin.max(1))).round() as i32;
        classes.push(key.rem_euclid(12) as usize);
    }

    let mut out = Vec::with_capacity(spec.frames.len());
    for frame in &spec.frames {
        let mut chroma = [0.0; 12];
        for bin in low..=high {
            chroma[classes[bin]] += frame[bin];
        }
        out.push(chroma);
    }
    out
}

// adds up the chroma of the frames between two times
pub fn sum_between(chroma: &[Chroma], frame_rate: f64, start: f64, end: f64) -> Chroma {
    let mut total = [0.0; 12];
    let from = (start * frame_rate).max(0.0) as usize;
    let to = ((end * frame_rate).max(0.0) as usize).min(chroma.len());
    for frame in chroma.iter().take(to).skip(from) {
        for pc in 0..12 {
            total[pc] += frame[pc];
        }
    }
    total
}
