use crate::analysis::chroma;
use crate::analysis::spectrum::Spectrogram;
use crate::audio::signal::Signal;
use crate::audio::wav::{self, Wav};
use crate::music::Grid;
use crate::rebuild::{self, Session};
use crate::separate;
use serde::Serialize;
use std::path::Path;

const SILENT: f64 = -60.0;
const LOUD_SHARE: f64 = 0.9;
const TOP_SHARE: f64 = 0.9;
const MATCHES: usize = 5;
const STEPS: usize = 16;

pub struct Measures {
    pub drums_db: Vec<Option<f64>>,
    pub bass_db: Vec<Option<f64>>,
    pub other_db: Vec<Option<f64>>,
    pub drums_top: Vec<Option<f64>>,
    chords: Vec<Option<Vec<f64>>>,
    rhythm: Vec<Option<Vec<f64>>>,
}

#[derive(Serialize)]
pub struct Match {
    pub bar: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub likeness: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drums_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drums_top_khz: Option<f64>,
}

#[derive(Serialize)]
pub struct Likeness {
    pub from_bar: u32,
    pub bars: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drums_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drums_top_khz: Option<f64>,
    pub chords: Vec<Match>,
    pub drums: Vec<Match>,
}

// how loud each stem is in every bar against its own loud bars, how high the drums reach, and each bar's chord colour and drum rhythm for comparing bars
pub fn measure(folder: &Path, session: &Session) -> Measures {
    let bars = session.bars as usize;
    let grid = &session.grid;
    let drums = stem(folder, "drums");
    let bass = stem(folder, "bass");
    let other = stem(folder, "other");
    let drums_level = drums.as_ref().map(|s| levels(s, grid, bars));
    let other_level = other.as_ref().map(|s| levels(s, grid, bars));
    let none = || vec![None; bars];
    Measures {
        drums_db: drums_level.as_deref().map_or_else(none, relative),
        bass_db: bass
            .as_ref()
            .map_or_else(none, |s| relative(&levels(s, grid, bars))),
        other_db: other_level.as_deref().map_or_else(none, relative),
        drums_top: match (&drums, &drums_level) {
            (Some(signal), Some(level)) => tops(signal, grid, level),
            _ => none(),
        },
        chords: match (&other, &other_level) {
            (Some(signal), Some(level)) => centered(colours(signal, grid, level)),
            _ => vec![None; bars],
        },
        rhythm: match (&drums, &drums_level) {
            (Some(signal), Some(level)) => centered(rhythms(signal, grid, level)),
            _ => vec![None; bars],
        },
    }
}

// the bars elsewhere in the song that sound most like a stretch in their chords and in their drum rhythm, however loud or filtered each is
pub fn sounds_like(folder: &Path, from_bar: u32, bars: u32) -> Result<Likeness, String> {
    let session = rebuild::load_session(folder)?;
    let measures = measure(folder, &session);
    likeness(&measures, &session, from_bar, bars)
}

// what a stretch sounds like, from measures already taken, so many stretches can be compared without reading the stems again
pub fn likeness(
    measures: &Measures,
    session: &Session,
    from_bar: u32,
    bars: u32,
) -> Result<Likeness, String> {
    let first = from_bar.max(1) as usize - 1;
    let length = (bars.max(1) as usize).min(session.bars as usize);
    if first + length > session.bars as usize {
        return Err(format!(
            "the song has {} bars, so bars {} to {} are past its end",
            session.bars,
            first + 1,
            first + length
        ));
    }
    let window = |values: &[Option<f64>]| mean(&values[first..first + length]);
    Ok(Likeness {
        from_bar: first as u32 + 1,
        bars: length as u32,
        drums_db: window(&measures.drums_db).map(|v| v.round() as i32),
        other_db: window(&measures.other_db).map(|v| v.round() as i32),
        drums_top_khz: window(&measures.drums_top).map(tenth),
        chords: matches(&measures.chords, first, length, measures, session),
        drums: matches(&measures.rhythm, first, length, measures, session),
    })
}

// a stem of a transcribed folder as one channel, when it is there
fn stem(folder: &Path, name: &str) -> Option<Signal> {
    let path = folder.join(separate::stem_file(name));
    wav::read_wav(&path).ok().map(|w| Signal::from_wav(&w))
}

// the first and last sample of a 0 based bar
pub(crate) fn bar_samples(signal: &Signal, grid: &Grid, bar: usize) -> (usize, usize) {
    let rate = signal.sample_rate as f64;
    let start = grid.origin + bar as f64 * grid.bar_seconds();
    let from = ((start * rate).max(0.0) as usize).min(signal.samples.len());
    let to = (((start + grid.bar_seconds()) * rate).max(0.0) as usize).min(signal.samples.len());
    (from, to)
}

// the loudness of every bar in decibels, missing when the bar is silent
pub(crate) fn levels(signal: &Signal, grid: &Grid, bars: usize) -> Vec<Option<f64>> {
    (0..bars)
        .map(|bar| {
            let (from, to) = bar_samples(signal, grid, bar);
            if to <= from {
                return None;
            }
            let power = signal.samples[from..to]
                .iter()
                .map(|&s| (s as f64) * (s as f64))
                .sum::<f64>()
                / (to - from) as f64;
            let db = 10.0 * (power + 1e-12).log10();
            (db > SILENT).then_some(db)
        })
        .collect()
}

// each bar's loudness against the stem's loud bars, so 0 is as loud as it gets and -10 is 10 decibels quieter
fn relative(levels: &[Option<f64>]) -> Vec<Option<f64>> {
    let mut heard: Vec<f64> = levels.iter().flatten().copied().collect();
    if heard.is_empty() {
        return vec![None; levels.len()];
    }
    heard.sort_by(f64::total_cmp);
    let loud = heard[((heard.len() - 1) as f64 * LOUD_SHARE).round() as usize];
    levels.iter().map(|l| l.map(|db| db - loud)).collect()
}

// the frequency in kilohertz below which most of each bar's energy sits, low when a filter cuts the highs
pub(crate) fn tops(signal: &Signal, grid: &Grid, level: &[Option<f64>]) -> Vec<Option<f64>> {
    let spectrum = Spectrogram::new(signal, 2048, 2048);
    let rate = spectrum.frame_rate();
    (0..level.len())
        .map(|bar| {
            level[bar]?;
            let start = grid.origin + bar as f64 * grid.bar_seconds();
            let from = (start * rate).max(0.0) as usize;
            let to = (((start + grid.bar_seconds()) * rate) as usize).min(spectrum.frames.len());
            let mut power = vec![0.0f64; spectrum.frame_size / 2 + 1];
            for frame in spectrum.frames.get(from..to)? {
                for (bin, m) in frame.iter().enumerate() {
                    power[bin] += (*m as f64) * (*m as f64);
                }
            }
            let total: f64 = power.iter().sum();
            if total <= 0.0 {
                return None;
            }
            let mut sum = 0.0;
            let bin = power
                .iter()
                .position(|p| {
                    sum += p;
                    sum >= TOP_SHARE * total
                })
                .unwrap_or(power.len() - 1);
            Some(spectrum.bin_hz(bin) as f64 / 1000.0)
        })
        .collect()
}

// how strongly each note name sounds in every bar that is not silent, from the frames that lie wholly inside the bar so a loud neighbour doesn't leak in
pub(crate) fn colours(
    signal: &Signal,
    grid: &Grid,
    level: &[Option<f64>],
) -> Vec<Option<Vec<f64>>> {
    let low = signal.downsample(4);
    let spectrum = Spectrogram::new(&low, 2048, 512);
    let frames = chroma::chromagram(&spectrum);
    let rate = spectrum.frame_rate();
    let half = spectrum.frame_size as f64 / 2.0 / spectrum.sample_rate as f64;
    (0..level.len())
        .map(|bar| {
            level[bar]?;
            let start = grid.origin + bar as f64 * grid.bar_seconds();
            let from = ((start + half) * rate).ceil().max(0.0) as usize;
            let to = (((start + grid.bar_seconds() - half) * rate).floor() as usize + 1)
                .min(frames.len());
            let mut sum = vec![0.0; 12];
            for frame in frames.get(from..to)? {
                for (total, v) in sum.iter_mut().zip(frame) {
                    *total += *v as f64;
                }
            }
            unit(sum)
        })
        .collect()
}

// how sharply the stem hits on each 16th step of every bar that is not silent
pub(crate) fn rhythms(
    signal: &Signal,
    grid: &Grid,
    level: &[Option<f64>],
) -> Vec<Option<Vec<f64>>> {
    let window = 1024;
    let hop = 256;
    let samples = &signal.samples;
    let count = samples.len().saturating_sub(window) / hop;
    let envelope: Vec<f64> = (0..count)
        .map(|i| {
            let part = &samples[i * hop..i * hop + window];
            let power = part.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / window as f64;
            (power.sqrt() + 1e-6).ln()
        })
        .collect();
    let rise: Vec<f64> = envelope
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).max(0.0))
        .collect();
    let rate = signal.sample_rate as f64 / hop as f64;
    let step = grid.bar_seconds() / STEPS as f64;
    (0..level.len())
        .map(|bar| {
            level[bar]?;
            let start = grid.origin + bar as f64 * grid.bar_seconds();
            let hits = (0..STEPS)
                .map(|s| {
                    let from = ((start + s as f64 * step) * rate).max(0.0) as usize;
                    let to = (((start + (s + 1) as f64 * step) * rate) as usize).min(rise.len());
                    rise.get(from..to)
                        .map_or(0.0, |r| r.iter().copied().fold(0.0, f64::max))
                })
                .collect();
            unit(hits)
        })
        .collect()
}

// how wide each bar sounds in stereo, the side's share of the sound, 0 for mono
pub(crate) fn widths(wav: &Wav, grid: &Grid, level: &[Option<f64>]) -> Vec<Option<f64>> {
    let channels = wav.info.channels as usize;
    let rate = wav.info.sample_rate as f64;
    let frames = wav.samples.len() / channels.max(1);
    (0..level.len())
        .map(|bar| {
            level[bar]?;
            if channels < 2 {
                return Some(0.0);
            }
            let start = grid.origin + bar as f64 * grid.bar_seconds();
            let from = ((start * rate).max(0.0) as usize).min(frames);
            let to = (((start + grid.bar_seconds()) * rate) as usize).min(frames);
            let (mut mid, mut side) = (0.0f64, 0.0f64);
            for frame in from..to {
                let left = wav.samples[frame * channels] as f64;
                let right = wav.samples[frame * channels + 1] as f64;
                mid += ((left + right) / 2.0).powi(2);
                side += ((left - right) / 2.0).powi(2);
            }
            (mid + side > 0.0).then(|| side / (mid + side))
        })
        .collect()
}

// how far apart each bar's loud and quiet moments are in decibels, small when reverb, long releases or a held sound fill the gaps between notes
pub(crate) fn spaces(signal: &Signal, grid: &Grid, level: &[Option<f64>]) -> Vec<Option<f64>> {
    let frame = 1024;
    (0..level.len())
        .map(|bar| {
            level[bar]?;
            let (from, to) = bar_samples(signal, grid, bar);
            let mut levels: Vec<f64> = signal.samples[from..to]
                .chunks_exact(frame)
                .map(|chunk| {
                    let power =
                        chunk.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / frame as f64;
                    10.0 * (power + 1e-12).log10()
                })
                .collect();
            if levels.len() < 8 {
                return None;
            }
            levels.sort_by(f64::total_cmp);
            let at = |share: f64| levels[((levels.len() - 1) as f64 * share).round() as usize];
            Some(at(0.9) - at(0.2))
        })
        .collect()
}

// a list of numbers scaled to length 1, or nothing when they are all zero
pub(crate) fn unit(values: Vec<f64>) -> Option<Vec<f64>> {
    let norm = values.iter().map(|v| v * v).sum::<f64>().sqrt();
    (norm > 1e-9).then(|| values.iter().map(|v| v / norm).collect())
}

// each bar's values minus the song's average, so what every bar shares, like the key, stops making bars look alike
fn centered(rows: Vec<Option<Vec<f64>>>) -> Vec<Option<Vec<f64>>> {
    let heard: Vec<&Vec<f64>> = rows.iter().flatten().collect();
    let Some(size) = heard.first().map(|r| r.len()) else {
        return rows;
    };
    let mut average = vec![0.0; size];
    for row in &heard {
        for (a, v) in average.iter_mut().zip(row.iter()) {
            *a += v / heard.len() as f64;
        }
    }
    rows.into_iter()
        .map(|row| unit(row?.iter().zip(&average).map(|(v, a)| v - a).collect()))
        .collect()
}

// the stretches elsewhere that look most like one stretch, best first, without two picks covering mostly the same bars
fn matches(
    feature: &[Option<Vec<f64>>],
    first: usize,
    length: usize,
    measures: &Measures,
    session: &Session,
) -> Vec<Match> {
    let mut scored: Vec<(usize, f64)> = (0..=feature.len().saturating_sub(length))
        .filter(|&start| start + length <= first || start >= first + length)
        .filter_map(|start| {
            let pairs: Vec<f64> = (0..length)
                .filter_map(|i| {
                    let a = feature[first + i].as_ref()?;
                    let b = feature[start + i].as_ref()?;
                    Some(a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>())
                })
                .collect();
            (pairs.len() * 2 >= length && !pairs.is_empty())
                .then(|| (start, pairs.iter().sum::<f64>() / pairs.len() as f64))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let apart = (length / 2).max(1);
    let mut picked: Vec<(usize, f64)> = Vec::new();
    for (start, score) in scored {
        if picked.iter().all(|&(p, _)| start.abs_diff(p) >= apart) {
            picked.push((start, score));
        }
        if picked.len() == MATCHES {
            break;
        }
    }
    picked
        .into_iter()
        .map(|(start, score)| {
            let window = |values: &[Option<f64>]| mean(&values[start..start + length]);
            Match {
                bar: start as u32 + 1,
                section: session
                    .sections
                    .iter()
                    .rev()
                    .find(|s| s.start_bar as usize <= start)
                    .map(|s| s.name.clone()),
                likeness: (score * 100.0).round() / 100.0,
                drums_db: window(&measures.drums_db).map(|v| v.round() as i32),
                other_db: window(&measures.other_db).map(|v| v.round() as i32),
                drums_top_khz: window(&measures.drums_top).map(tenth),
            }
        })
        .collect()
}

// the average of the values that are there
pub(crate) fn mean(values: &[Option<f64>]) -> Option<f64> {
    let heard: Vec<f64> = values.iter().flatten().copied().collect();
    (!heard.is_empty()).then(|| heard.iter().sum::<f64>() / heard.len() as f64)
}

// a number rounded to one decimal
fn tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16000;

    // a signal of whole bars at 120 bpm, each bar made by a function of its number and the time
    fn bars(count: usize, mut sound: impl FnMut(usize, f64) -> f32) -> (Signal, Grid) {
        let grid = Grid {
            bpm: 120.0,
            origin: 0.0,
        };
        let per_bar = (grid.bar_seconds() * RATE as f64) as usize;
        let samples = (0..count * per_bar)
            .map(|i| sound(i / per_bar, i as f64 / RATE as f64))
            .collect();
        (
            Signal {
                samples,
                sample_rate: RATE,
            },
            grid,
        )
    }

    // a chord of sine waves at some midi notes
    fn chord(keys: &[f64], t: f64) -> f32 {
        keys.iter()
            .map(|k| (2.0 * std::f64::consts::PI * 440.0 * 2f64.powf((k - 69.0) / 12.0) * t).sin())
            .sum::<f64>() as f32
            * 0.2
    }

    // quiet low bars read below the loud bright ones, and a low sound reaches far less high than noise
    #[test]
    fn measures_levels_and_tops() {
        let mut seed = 1u32;
        let (signal, grid) = bars(4, |bar, t| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            if bar < 2 {
                0.05 * (2.0 * std::f64::consts::PI * 200.0 * t).sin() as f32
            } else {
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.8
            }
        });
        let level = levels(&signal, &grid, 4);
        let loud = relative(&level);
        assert!(loud[3].unwrap().abs() < 1.0);
        assert!(loud[0].unwrap() < -10.0);
        let top = tops(&signal, &grid, &level);
        assert!(top[0].unwrap() < 0.5);
        assert!(top[3].unwrap() > 2.0);
    }

    // a quiet bar of a chord looks like the loud bars of the same chord, not like another chord
    #[test]
    fn quiet_chords_match_loud_ones() {
        let c_major = [60.0, 64.0, 67.0];
        let d_minor = [62.0, 65.0, 69.0];
        let (signal, grid) = bars(6, |bar, t| match bar {
            0 | 2 => chord(&c_major, t),
            4 => chord(&c_major, t) * 0.05,
            _ => chord(&d_minor, t),
        });
        let level = levels(&signal, &grid, 6);
        let colour = centered(colours(&signal, &grid, &level));
        let dot = |a: usize, b: usize| {
            let (x, y) = (colour[a].as_ref().unwrap(), colour[b].as_ref().unwrap());
            x.iter().zip(y).map(|(p, q)| p * q).sum::<f64>()
        };
        assert!(dot(4, 0) > 0.8);
        assert!(dot(4, 1) < 0.0);
    }
}
