use crate::analysis::spectrum::Spectrogram;
use crate::audio::convert;
use crate::audio::signal::Signal;
use crate::audio::wav::{self, Wav};
use crate::combine;
use crate::flp::{events, info};
use crate::listen;
use crate::music::Grid;
use crate::rebuild::{self, Session};
use crate::render::{self, Stem};
use crate::separate;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const AUDIBLE: f64 = -50.0;
const BAND_EDGES: [f64; 4] = [60.0, 250.0, 1000.0, 4000.0];
const BAND_NAMES: [&str; 5] = [
    "sub (under 60 Hz)",
    "lows (60-250 Hz)",
    "low mids (250 Hz-1 kHz)",
    "high mids (1-4 kHz)",
    "highs (4 kHz and up)",
];
const RHYTHM_OFF: f64 = 0.5;
const NOTES_OFF: f64 = 0.7;
const BAND_FACTOR: f64 = 2.0;
const BAND_MATTERS: f64 = 0.05;
const DARKER: f64 = 0.6;
const BRIGHTER: f64 = 1.6;
const WIDE: f64 = 0.15;
const NARROW: f64 = 0.05;
const BALANCE_DB: f64 = 3.0;
const RENDER_TIMEOUT: u64 = 600;

type Levels = Vec<Option<f64>>;

#[derive(Serialize)]
pub struct Band {
    pub band: String,
    pub yours: u32,
    pub song: u32,
}

#[derive(Serialize)]
pub struct BarLine {
    pub song_bar: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yours_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rhythm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<f64>,
}

#[derive(Serialize)]
pub struct PartComparison {
    pub part: String,
    pub render: String,
    pub song_bars: String,
    pub issues: Vec<String>,
    pub rhythm: Option<f64>,
    pub notes: Option<f64>,
    pub yours_db: Option<f64>,
    pub song_db: Option<f64>,
    pub brightness_khz: [Option<f64>; 2],
    pub width: [Option<f64>; 2],
    pub bands: Vec<Band>,
    pub bars: Vec<BarLine>,
}

#[derive(Serialize)]
pub struct RenderComparison {
    pub song_bars: String,
    pub balance: Vec<String>,
    pub parts: Vec<PartComparison>,
}

struct Heard {
    level: Vec<Option<f64>>,
    tops: Vec<Option<f64>>,
    colours: Vec<Option<Vec<f64>>>,
    rhythms: Vec<Option<Vec<f64>>>,
    bands: Vec<Option<[f64; 5]>>,
    widths: Vec<Option<f64>>,
}

// renders a project, one stem of it or each stem and the whole mix, and compares how it sounds with the song's stems from a song bar on
pub fn compare_render(
    project: &Path,
    folder: &Path,
    from_bar: u32,
    part: Option<Stem>,
    bars: Option<u32>,
) -> Result<RenderComparison, String> {
    let session = rebuild::load_session(folder)?;
    let flp = events::read(project)?;
    let tempo = info::summarize(&flp).tempo.unwrap_or(session.grid.bpm);
    let length = bars
        .unwrap_or_else(|| combine::playlist(&flp).bars.round() as u32)
        .max(1);
    let out = render::default_folder(project);
    let timeout = Duration::from_secs(RENDER_TIMEOUT);
    let stems: Vec<Stem> = match part {
        Some(stem) => vec![stem],
        None => [Stem::Drums, Stem::Bass, Stem::Other, Stem::Vocals]
            .into_iter()
            .filter(|&stem| {
                render::solo(&flp, stem)
                    .1
                    .iter()
                    .any(|name| !name.to_lowercase().contains("stem"))
            })
            .collect(),
    };
    let first = from_bar.max(1) as usize - 1;
    let mut parts = Vec::new();
    let mut levels = Vec::new();
    for stem in stems {
        let rendered = render::render(project, Some(stem), &out, timeout)?;
        let song = folder.join(separate::stem_file(stem.name()));
        let (comparison, yours, theirs) = compare_wav(
            Path::new(&rendered.wav),
            &song,
            &session,
            stem.name(),
            first,
            length as usize,
            tempo,
        )?;
        levels.push((stem.name(), yours, theirs));
        parts.push(comparison);
    }
    let mut balance = Vec::new();
    if part.is_none() {
        let rendered = render::render(project, None, &out, timeout)?;
        let song_mix = folder.join("extras").join("mix.wav");
        if song_mix.exists() {
            let (comparison, mix_yours, mix_song) = compare_wav(
                Path::new(&rendered.wav),
                &song_mix,
                &session,
                "mix",
                first,
                length as usize,
                tempo,
            )?;
            balance = balance_notes(&levels, &mix_yours, &mix_song);
            parts.push(comparison);
        } else {
            balance.push(format!(
                "{} is missing, so the balance between the parts can't be compared",
                song_mix.display()
            ));
        }
    }
    Ok(RenderComparison {
        song_bars: format!("{}-{}", first + 1, first + length as usize),
        balance,
        parts,
    })
}

// compares a render of one part with the song's audio for the same part, bar by bar, and returns the comparison with both sides' bar levels
fn compare_wav(
    render_path: &Path,
    song_path: &Path,
    session: &Session,
    part: &str,
    first: usize,
    length: usize,
    tempo: f64,
) -> Result<(PartComparison, Levels, Levels), String> {
    let render_wav = load(render_path)?;
    let song_wav = wav::read_wav(song_path)?;
    let render_grid = Grid {
        bpm: tempo,
        origin: 0.0,
    };
    let in_render =
        (render_wav.info.duration_seconds() / render_grid.bar_seconds()).floor() as usize;
    let n = length
        .min(in_render.max(1))
        .min((session.bars as usize).saturating_sub(first));
    let yours = hear(&render_wav, &render_grid, 0, n);
    let theirs = hear(&song_wav, &session.grid, first, n);
    let comparison = compare(part, render_path, first, n, &yours, &theirs);
    Ok((comparison, yours.level, theirs.level))
}

// a wav file, through ffmpeg into a converted folder next to it when it isn't the plain 16 bit kind unbake reads itself
fn load(path: &Path) -> Result<Wav, String> {
    let work = path.parent().unwrap_or(Path::new(".")).join("converted");
    convert::load(path, &work)
}

// every measure of some bars of a recording, starting at a 0 based bar
fn hear(wav: &Wav, grid: &Grid, first: usize, bars: usize) -> Heard {
    let signal = Signal::from_wav(wav);
    let total = first + bars;
    let level = listen::levels(&signal, grid, total);
    let slice = |values: Vec<Option<f64>>| values[first..].to_vec();
    let tops = listen::tops(&signal, grid, &level);
    let colours = listen::colours(&signal, grid, &level)[first..].to_vec();
    let rhythms = listen::rhythms(&signal, grid, &level)[first..].to_vec();
    let bands = band_shares(&signal, grid, &level)[first..].to_vec();
    let widths = widths(wav, grid, &level)[first..].to_vec();
    Heard {
        level: slice(level),
        tops: slice(tops),
        colours,
        rhythms,
        bands,
        widths,
    }
}

// the share of each bar's sound in five bands, from the sub to the highs
fn band_shares(signal: &Signal, grid: &Grid, level: &[Option<f64>]) -> Vec<Option<[f64; 5]>> {
    let spectrum = Spectrogram::new(signal, 4096, 2048);
    let rate = spectrum.frame_rate();
    let band_of: Vec<usize> = (0..spectrum.frame_size / 2 + 1)
        .map(|bin| {
            let hz = spectrum.bin_hz(bin) as f64;
            BAND_EDGES.iter().filter(|&&edge| hz >= edge).count()
        })
        .collect();
    (0..level.len())
        .map(|bar| {
            level[bar]?;
            let start = grid.origin + bar as f64 * grid.bar_seconds();
            let from = (start * rate).max(0.0) as usize;
            let to = (((start + grid.bar_seconds()) * rate) as usize).min(spectrum.frames.len());
            let mut energy = [0.0f64; 5];
            for frame in spectrum.frames.get(from..to)? {
                for (bin, m) in frame.iter().enumerate() {
                    energy[band_of[bin]] += (*m as f64) * (*m as f64);
                }
            }
            let total: f64 = energy.iter().sum();
            (total > 0.0).then(|| energy.map(|e| e / total))
        })
        .collect()
}

// how wide each bar sounds in stereo, the side's share of the sound, 0 for mono
fn widths(wav: &Wav, grid: &Grid, level: &[Option<f64>]) -> Vec<Option<f64>> {
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

// the comparison of one part's render with the song, bar by bar, with what stands out written as issues
fn compare(
    part: &str,
    render_path: &Path,
    first: usize,
    n: usize,
    yours: &Heard,
    theirs: &Heard,
) -> PartComparison {
    let plays = |level: Option<f64>| level.is_some_and(|db| db > AUDIBLE);
    let cosine = |a: &Option<Vec<f64>>, b: &Option<Vec<f64>>| match (a, b) {
        (Some(a), Some(b)) => Some(a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>()),
        _ => None,
    };
    let pitched = part != "drums";
    let mut lines = Vec::new();
    let mut silent = Vec::new();
    let mut extra = Vec::new();
    let mut off_rhythm = Vec::new();
    let mut off_notes = Vec::new();
    let mut both = Vec::new();
    for i in 0..n {
        let bar = (first + i + 1) as u32;
        let (y, s) = (yours.level[i], theirs.level[i]);
        let rhythm = cosine(&yours.rhythms[i], &theirs.rhythms[i]).map(round);
        let notes = pitched
            .then(|| cosine(&yours.colours[i], &theirs.colours[i]))
            .flatten()
            .map(round);
        match (plays(y), plays(s)) {
            (false, true) => silent.push(bar),
            (true, false) => extra.push(bar),
            (true, true) => {
                both.push(i);
                if rhythm.is_some_and(|r| r < RHYTHM_OFF) {
                    off_rhythm.push(bar);
                }
                if notes.is_some_and(|c| c < NOTES_OFF) {
                    off_notes.push(bar);
                }
            }
            (false, false) => {}
        }
        lines.push(BarLine {
            song_bar: bar,
            yours_db: y.map(|v| v.round() as i32),
            song_db: s.map(|v| v.round() as i32),
            rhythm,
            notes,
        });
    }
    let average = |values: Vec<Option<f64>>| listen::mean(&values).map(round);
    let over_both =
        |f: &dyn Fn(usize) -> Option<f64>| average(both.iter().map(|&i| f(i)).collect());
    let rhythm = over_both(&|i| cosine(&yours.rhythms[i], &theirs.rhythms[i]));
    let notes = if pitched {
        over_both(&|i| cosine(&yours.colours[i], &theirs.colours[i]))
    } else {
        None
    };
    let yours_db = over_both(&|i| yours.level[i]);
    let song_db = over_both(&|i| theirs.level[i]);
    let brightness = [
        over_both(&|i| yours.tops[i]),
        over_both(&|i| theirs.tops[i]),
    ];
    let width = [
        over_both(&|i| yours.widths[i]),
        over_both(&|i| theirs.widths[i]),
    ];
    let band_means = |heard: &Heard| -> [f64; 5] {
        let mut sum = [0.0; 5];
        let mut count = 0.0;
        for &i in &both {
            if let Some(shares) = heard.bands[i] {
                for (total, share) in sum.iter_mut().zip(shares) {
                    *total += share;
                }
                count += 1.0;
            }
        }
        sum.map(|total| if count > 0.0 { total / count } else { 0.0 })
    };
    let (mine, song) = (band_means(yours), band_means(theirs));
    let bands: Vec<Band> = BAND_NAMES
        .iter()
        .enumerate()
        .map(|(b, name)| Band {
            band: name.to_string(),
            yours: (mine[b] * 100.0).round() as u32,
            song: (song[b] * 100.0).round() as u32,
        })
        .collect();

    let mut issues = Vec::new();
    if !silent.is_empty() {
        issues.push(format!(
            "silent in song bars {} where the song's {} plays: check its channels are unmuted, audible and have notes there",
            ranges(&silent),
            part
        ));
    }
    if !extra.is_empty() {
        issues.push(format!(
            "plays in song bars {} where the song's {} is silent",
            ranges(&extra),
            part
        ));
    }
    if !off_rhythm.is_empty() {
        issues.push(format!(
            "{} start at other times than the song's in bars {}",
            if pitched { "notes" } else { "hits" },
            ranges(&off_rhythm)
        ));
    }
    if !off_notes.is_empty() {
        issues.push(format!(
            "plays other notes than the song in bars {}",
            ranges(&off_notes)
        ));
    }
    if !both.is_empty() {
        for (b, name) in BAND_NAMES.iter().enumerate() {
            let (y, s) = (mine[b], song[b]);
            if y.max(s) < BAND_MATTERS {
                continue;
            }
            if y >= s * BAND_FACTOR {
                issues.push(format!(
                    "too much {}: {:.0}% of the sound against {:.0}% in the song",
                    name,
                    y * 100.0,
                    s * 100.0
                ));
            } else if s >= y * BAND_FACTOR {
                issues.push(format!(
                    "too little {}: {:.0}% of the sound against {:.0}% in the song",
                    name,
                    y * 100.0,
                    s * 100.0
                ));
            }
        }
    }
    if let [Some(y), Some(s)] = brightness {
        if s > 0.0 && y / s < DARKER {
            issues.push(format!(
                "darker than the song: most of its sound stays under {:.1} kHz against {:.1} kHz",
                y, s
            ));
        } else if s > 0.0 && y / s > BRIGHTER {
            issues.push(format!(
                "brighter than the song: most of its sound reaches {:.1} kHz against {:.1} kHz",
                y, s
            ));
        }
    }
    if let [Some(y), Some(s)] = width {
        if s >= WIDE && y < NARROW {
            issues.push(format!(
                "nearly mono ({:.2}) where the song's {} is wide in stereo ({:.2})",
                y, part, s
            ));
        } else if y >= WIDE && s < NARROW {
            issues.push(format!(
                "wide in stereo ({:.2}) where the song's {} is nearly mono ({:.2})",
                y, part, s
            ));
        }
    }
    PartComparison {
        part: part.to_string(),
        render: render_path.display().to_string(),
        song_bars: format!("{}-{}", first + 1, first + n),
        issues,
        rhythm,
        notes,
        yours_db,
        song_db,
        brightness_khz: brightness,
        width,
        bands,
        bars: lines,
    }
}

// how loud each part sits against the whole mix in the render compared with the song, where it differs enough to hear
fn balance_notes(
    parts: &[(&str, Levels, Levels)],
    mix_yours: &[Option<f64>],
    mix_song: &[Option<f64>],
) -> Vec<String> {
    let mut notes = Vec::new();
    for (name, yours, theirs) in parts {
        let offsets = |part: &[Option<f64>], mix: &[Option<f64>]| {
            let gaps: Vec<Option<f64>> = part
                .iter()
                .zip(mix)
                .map(|(p, m)| match (p, m) {
                    (Some(p), Some(m)) if *p > AUDIBLE => Some(p - m),
                    _ => None,
                })
                .collect();
            listen::mean(&gaps)
        };
        let (Some(y), Some(s)) = (offsets(yours, mix_yours), offsets(theirs, mix_song)) else {
            continue;
        };
        let difference = y - s;
        if difference.abs() >= BALANCE_DB {
            notes.push(format!(
                "your {} part sits {:.0} dB {} in the mix than the song's",
                name,
                difference.abs(),
                if difference > 0.0 {
                    "louder"
                } else {
                    "quieter"
                }
            ));
        }
    }
    notes
}

// a list of bars written as runs, like 33-36, 39
fn ranges(bars: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bars.len() {
        let mut j = i;
        while j + 1 < bars.len() && bars[j + 1] == bars[j] + 1 {
            j += 1;
        }
        out.push(if i == j {
            bars[i].to_string()
        } else {
            format!("{}-{}", bars[i], bars[j])
        });
        i = j + 1;
    }
    out.join(", ")
}

// a number rounded to two decimals
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // what one bar of a part sounds like, for building comparisons by hand
    fn heard(levels: &[Option<f64>], highs: f64) -> Heard {
        let n = levels.len();
        let shares = [0.1, 0.5, 0.2, 0.2 - highs, highs];
        Heard {
            level: levels.to_vec(),
            tops: vec![Some(if highs > 0.1 { 6.0 } else { 1.0 }); n],
            colours: vec![Some([1.0; 12].iter().map(|v| v / 12f64.sqrt()).collect()); n],
            rhythms: vec![Some(vec![0.25; 16]); n],
            bands: vec![Some(shares); n],
            widths: vec![Some(0.0); n],
        }
    }

    // a part that stays silent where the song plays, and has far fewer highs, gets both written up
    #[test]
    fn finds_silence_and_missing_highs() {
        let yours = heard(&[Some(-10.0), None, Some(-10.0)], 0.01);
        let song = heard(&[Some(-20.0), Some(-20.0), Some(-20.0)], 0.15);
        let found = compare("drums", Path::new("x.wav"), 32, 3, &yours, &song);
        assert_eq!(found.song_bars, "33-35");
        assert!(found.issues[0].starts_with("silent in song bars 34 "));
        assert!(
            found
                .issues
                .iter()
                .any(|i| i.starts_with("too little highs"))
        );
        assert!(
            found
                .issues
                .iter()
                .any(|i| i.starts_with("darker than the song"))
        );
        assert_eq!(found.rhythm, Some(1.0));
        assert_eq!(found.notes, None);
    }

    // bar lists read as runs
    #[test]
    fn bars_as_runs() {
        assert_eq!(ranges(&[33, 34, 35, 36, 39]), "33-36, 39");
        assert_eq!(ranges(&[5]), "5");
        assert_eq!(ranges(&[]), "");
    }
}
