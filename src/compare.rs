use crate::analysis::chroma::{self, Chroma};
use crate::analysis::spectrum::Spectrogram;
use crate::analysis::{drums, onsets};
use crate::audio::math;
use crate::audio::signal::Signal;
use crate::music::{Grid, Part};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub dimension: String,
    pub value: f64,
    pub measured_on: String,
    pub note: String,
}

// compares a rebuilt drum part with the drum stem in timing, rhythm per drum, timbre and envelope
pub fn drum_part(original: &Signal, rebuilt: &Signal, measured_on: &str) -> Vec<Score> {
    let a = Spectrogram::standard(&original.normalized());
    let b = Spectrogram::standard(&rebuilt.normalized());

    let hits_a = drums::detect(&a);
    let hits_b = drums::detect(&b);
    let mut per_drum = Vec::new();
    let mut notes = Vec::new();
    for part in [Part::Kick, Part::Snare, Part::Hat] {
        let in_a = hits_as_onsets(&hits_a, part);
        let in_b = hits_as_onsets(&hits_b, part);
        if in_a.is_empty() && in_b.is_empty() {
            continue;
        }
        let f = onsets::f_measure(&in_a, &in_b);
        per_drum.push(f);
        notes.push(format!("{} {:.2}", part.name(), f));
    }

    vec![
        timing(&a, &b, measured_on),
        score(
            "rhythm",
            math::mean(&per_drum.iter().map(|&f| f as f32).collect::<Vec<_>>()) as f64,
            measured_on,
            &format!(
                "hits found again in the rebuild with the same detector, {}",
                notes.join(", ")
            ),
        ),
        timbre(&a, &b, measured_on),
        envelope(&a, &b, measured_on),
    ]
}

// compares a rebuilt melodic part with its stem in timing, pitch, timbre and envelope
pub fn melodic_part(
    original: &Signal,
    rebuilt: &Signal,
    grid: &Grid,
    measured_on: &str,
) -> Vec<Score> {
    let a = Spectrogram::standard(&original.normalized());
    let b = Spectrogram::standard(&rebuilt.normalized());
    vec![
        timing(&a, &b, measured_on),
        pitch(original, rebuilt, grid, measured_on),
        timbre(&a, &b, measured_on),
        envelope(&a, &b, measured_on),
    ]
}

// how many attacks line up within 50 ms, as an f-measure from 0 to 1
fn timing(a: &Spectrogram, b: &Spectrogram, measured_on: &str) -> Score {
    let f = onsets::f_measure(&onsets::detect(a), &onsets::detect(b));
    score(
        "timing",
        f,
        measured_on,
        "attacks that line up within 50 ms",
    )
}

// how alike the notes are bar by bar, comparing which of the 12 pitch classes sound
fn pitch(original: &Signal, rebuilt: &Signal, grid: &Grid, measured_on: &str) -> Score {
    let a = chroma::chromagram(&Spectrogram::harmonic(&original.normalized()));
    let b = chroma::chromagram(&Spectrogram::harmonic(&rebuilt.normalized()));
    let frame_rate = original.sample_rate as f64 / crate::analysis::spectrum::HOP as f64;

    let bars = grid.bar_count(original.duration());
    let mut bar_chroma: Vec<(Chroma, Chroma)> = Vec::new();
    for bar in 0..bars {
        let start = grid.origin + bar as f64 * grid.bar_seconds();
        let end = start + grid.bar_seconds();
        bar_chroma.push((
            chroma::sum_between(&a, frame_rate, start, end),
            chroma::sum_between(&b, frame_rate, start, end),
        ));
    }

    let loudest = bar_chroma
        .iter()
        .map(|(x, _)| x.iter().sum::<f32>())
        .fold(0.0f32, f32::max);
    let mut similarities = Vec::new();
    for (x, y) in &bar_chroma {
        if x.iter().sum::<f32>() > 0.05 * loudest {
            similarities.push(math::cosine(x, y));
        }
    }
    score(
        "pitch",
        math::mean(&similarities) as f64,
        measured_on,
        "chroma per bar, octaves and chords inside one bar are not told apart",
    )
}

// how alike the overall tone colour is, from the average loudness in 40 hearing-spaced bands
fn timbre(a: &Spectrogram, b: &Spectrogram, measured_on: &str) -> Score {
    let active = active_frames(a);
    let r = math::correlation(&band_profile(a, &active), &band_profile(b, &active));
    score(
        "timbre",
        r.max(0.0) as f64,
        measured_on,
        "average spectrum shape where the original plays",
    )
}

// how alike the rise and fall of loudness is over time
fn envelope(a: &Spectrogram, b: &Spectrogram, measured_on: &str) -> Score {
    let r = math::correlation(&frame_levels(a), &frame_levels(b));
    score(
        "envelope",
        r.max(0.0) as f64,
        measured_on,
        "frame by frame loudness, on a log scale",
    )
}

// the average loudness in 40 bands from 40 hz to 16 khz, spaced the way we hear pitch
pub fn band_profile(spec: &Spectrogram, active: &[bool]) -> Vec<f32> {
    let mel = |hz: f32| 2595.0 * (1.0 + hz / 700.0).log10();
    let hz = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);
    let (low, high) = (mel(40.0), mel(16000.0));

    let mut edges = Vec::with_capacity(41);
    for i in 0..=40 {
        edges.push(spec.hz_bin(hz(low + (high - low) * i as f32 / 40.0)));
    }

    let mut profile = vec![0.0f32; 40];
    let mut count = 0;
    for (frame, &on) in spec.frames.iter().zip(active) {
        if !on {
            continue;
        }
        count += 1;
        for band in 0..40 {
            let mut sum = 0.0;
            for bin in edges[band]..=edges[band + 1].max(edges[band]) {
                sum += frame[bin] * frame[bin];
            }
            profile[band] += (1.0 + 1000.0 * sum).ln();
        }
    }
    for value in profile.iter_mut() {
        *value /= count.max(1) as f32;
    }
    profile
}

// which frames are within 30 db of the loudest frames
pub fn active_frames(spec: &Spectrogram) -> Vec<bool> {
    let levels = frame_levels(spec);
    let top = math::percentile(&levels, 0.99);
    levels.iter().map(|&l| l > top - 3.0).collect()
}

// the loudness of every frame on a log scale, where 1 step is 10 db
pub fn frame_levels(spec: &Spectrogram) -> Vec<f32> {
    let mut levels = Vec::with_capacity(spec.frames.len());
    for frame in &spec.frames {
        let energy: f32 = frame.iter().map(|m| m * m).sum();
        levels.push((energy + 1e-10).log10());
    }
    levels
}

// turns the hits of one drum into onsets so they can be matched by time
fn hits_as_onsets(hits: &[drums::Hit], part: Part) -> Vec<onsets::Onset> {
    hits.iter()
        .filter(|h| h.part == part)
        .map(|h| onsets::Onset {
            time: h.time,
            frame: 0,
        })
        .collect()
}

// builds a score rounded to 3 decimals
fn score(dimension: &str, value: f64, measured_on: &str, note: &str) -> Score {
    Score {
        dimension: dimension.to_string(),
        value: (value * 1000.0).round() / 1000.0,
        measured_on: measured_on.to_string(),
        note: note.to_string(),
    }
}
