use crate::audio::math;
use crate::flp::timeline::{self, TimedNote};
use crate::flp::{events, info};
use crate::midi;
use serde::Serialize;
use std::path::Path;

const TOLERANCE: f64 = 0.05;

#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub name: String,
    pub truth: usize,
    pub found: usize,
    pub matched: usize,
    pub precision: f64,
    pub recall: f64,
    pub f: f64,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub tempo_truth: f64,
    pub tempo_found: f64,
    pub drums: Vec<Score>,
    pub bass: Score,
    pub bass_channel: String,
    pub bass_octave_shift: i32,
    pub harmony: f64,
    pub patterns_truth: usize,
    pub patterns_found: Option<usize>,
    pub notes: Vec<String>,
}

struct Timed {
    time: f64,
    length: f64,
    key: u8,
}

// compares a rebuild folder with the fl studio project the song was made from
pub fn evaluate(project_path: &Path, rebuild_dir: &Path) -> Result<Evaluation, String> {
    let project = events::read(project_path)?;
    let summary = info::summarize(&project);
    let tempo_truth = summary.tempo.unwrap_or(120.0);
    let truth_tick = 60.0 / tempo_truth / project.ppq as f64;
    let truth_notes = timeline::timeline(&project);
    let clips = timeline::audio_clips(&project);

    let report_path = rebuild_dir.join("extras/report.json");
    let report_text = std::fs::read_to_string(&report_path)
        .map_err(|e| format!("can't read {}: {}", report_path.display(), e))?;
    let report: serde_json::Value =
        serde_json::from_str(&report_text).map_err(|e| e.to_string())?;
    let tempo_found = report["tempo_bpm"].as_f64().unwrap_or(0.0);
    let origin = report["first_bar_seconds"].as_f64().unwrap_or(0.0);

    let midi_dir = rebuild_dir.join("extras/midi");
    let ours = |file: &str| -> Result<Vec<Timed>, String> {
        let path = midi_dir.join(file);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let (ppq, tracks) = midi::read(&path)?;
        let tick = 60.0 / tempo_found / ppq as f64;
        let mut out = Vec::new();
        for track in tracks {
            for (start, length, key) in track.notes {
                out.push(Timed {
                    time: origin + start as f64 * tick,
                    length: length as f64 * tick,
                    key,
                });
            }
        }
        Ok(out)
    };

    let mut drums = Vec::new();
    let our_drums = ours("drums.mid")?;
    for (name, words, key) in [
        ("kick", &["kick"][..], 36),
        ("snare", &["snare", "clap"][..], 38),
        ("hat", &["hat", "hh"][..], 42),
    ] {
        let mut truth: Vec<Timed> = truth_notes
            .iter()
            .filter(|n| has_word(&channel_name(&summary, n.channel), words))
            .map(|n| to_timed(n, truth_tick))
            .collect();
        for clip in &clips {
            if clip.length <= 4 * project.ppq as u32
                && has_word(&channel_name(&summary, clip.channel), words)
            {
                truth.push(Timed {
                    time: clip.start as f64 * truth_tick,
                    length: clip.length as f64 * truth_tick,
                    key: 0,
                });
            }
        }
        truth.sort_by(|a, b| a.time.total_cmp(&b.time));
        let found: Vec<Timed> = our_drums
            .iter()
            .filter(|n| n.key == key)
            .map(|n| Timed { ..*n })
            .collect();
        drums.push(score(name, &truth, &found, false));
    }

    let melodic: Vec<&TimedNote> = truth_notes
        .iter()
        .filter(|n| !is_drum(&channel_name(&summary, n.channel)))
        .collect();
    let bass_channel = pick_bass(&summary, &melodic);
    let truth_bass: Vec<Timed> = melodic
        .iter()
        .filter(|n| Some(n.channel) == bass_channel)
        .map(|n| to_timed(n, truth_tick))
        .collect();
    let our_bass = ours("bass.mid")?;
    let bass = score("bass", &truth_bass, &our_bass, true);
    let bass_octave_shift = octave_shift(&truth_bass, &our_bass);

    let truth_harmony: Vec<Timed> = melodic.iter().map(|n| to_timed(n, truth_tick)).collect();
    let mut our_harmony = ours("bass.mid")?;
    our_harmony.extend(ours("melody.mid")?);
    our_harmony.extend(ours("chords.mid")?);
    let harmony = harmony_similarity(&truth_harmony, &our_harmony, 240.0 / tempo_truth);

    let patterns_found = std::fs::read_dir(rebuild_dir).ok().and_then(|entries| {
        entries
            .flatten()
            .find(|e| e.path().extension().is_some_and(|x| x == "flp"))
            .and_then(|e| events::read(&e.path()).ok())
            .map(|p| timeline::patterns_used(&p))
    });

    Ok(Evaluation {
        tempo_truth,
        tempo_found,
        drums,
        bass,
        bass_channel: bass_channel
            .map(|c| channel_name(&summary, c))
            .unwrap_or_else(|| "none found".to_string()),
        bass_octave_shift,
        harmony: round(harmony),
        patterns_truth: timeline::patterns_used(&project),
        patterns_found,
        notes: vec![
            "hits and notes count as matched within 50 ms".to_string(),
            "bass notes also need the same note name, in any octave".to_string(),
            "clips up to a bar long on drum channels count as hits, sampled breaks are not counted"
                .to_string(),
            "harmony compares which notes sound in each bar across all melodic parts, from 0 to 1"
                .to_string(),
        ],
    })
}

// a project note converted to seconds
fn to_timed(note: &TimedNote, tick: f64) -> Timed {
    Timed {
        time: note.start as f64 * tick,
        length: note.length as f64 * tick,
        key: note.key,
    }
}

// the display name of a channel by its number
fn channel_name(summary: &info::Summary, channel: u16) -> String {
    summary
        .channels
        .iter()
        .find(|c| c.id == channel)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("Channel {}", channel))
}

// true when a channel name contains one of the words, ignoring case
fn has_word(name: &str, words: &[&str]) -> bool {
    let name = name.to_lowercase();
    words.iter().any(|w| name.contains(w))
}

// true for channels that are drums by their name
fn is_drum(name: &str) -> bool {
    has_word(
        name,
        &[
            "kick", "snare", "clap", "hat", "hh", "cymbal", "crash", "ride", "perc", "tom",
        ],
    )
}

// the bass channel, one named bass with notes, otherwise the lowest single note line below a flat 3, if any
fn pick_bass(summary: &info::Summary, melodic: &[&TimedNote]) -> Option<u16> {
    let mut channels: Vec<u16> = melodic.iter().map(|n| n.channel).collect();
    channels.sort();
    channels.dedup();

    for &c in &channels {
        if has_word(&channel_name(summary, c), &["bass", "808", "sub"]) {
            return Some(c);
        }
    }

    let mut best: Option<(u16, f32)> = None;
    for &c in &channels {
        let notes: Vec<&&TimedNote> = melodic.iter().filter(|n| n.channel == c).collect();
        if notes.len() < 8 {
            continue;
        }
        let mut overlapping = 0;
        for pair in notes.windows(2) {
            if pair[1].start < pair[0].start + pair[0].length {
                overlapping += 1;
            }
        }
        if overlapping as f32 > 0.2 * notes.len() as f32 {
            continue;
        }
        let keys: Vec<f32> = notes.iter().map(|n| n.key as f32).collect();
        let median = math::percentile(&keys, 0.5);
        if median > 56.0 {
            continue;
        }
        if best.is_none_or(|(_, m)| median < m) {
            best = Some((c, median));
        }
    }
    best.map(|(c, _)| c)
}

// how many found notes line up with real ones, as precision, recall and their balance
fn score(name: &str, truth: &[Timed], found: &[Timed], same_note: bool) -> Score {
    let mut used = vec![false; found.len()];
    let mut matched = 0;
    for t in truth {
        for (j, f) in found.iter().enumerate() {
            let same = !same_note || t.key % 12 == f.key % 12;
            if !used[j] && same && (t.time - f.time).abs() <= TOLERANCE {
                used[j] = true;
                matched += 1;
                break;
            }
        }
    }
    let precision = matched as f64 / found.len().max(1) as f64;
    let recall = matched as f64 / truth.len().max(1) as f64;
    let f = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    Score {
        name: name.to_string(),
        truth: truth.len(),
        found: found.len(),
        matched,
        precision: round(precision),
        recall: round(recall),
        f: round(f),
    }
}

// the most common octave difference between found bass notes and the real ones they line up with
fn octave_shift(truth: &[Timed], found: &[Timed]) -> i32 {
    let mut counts = std::collections::HashMap::new();
    for t in truth {
        if let Some(f) = found
            .iter()
            .find(|f| (t.time - f.time).abs() <= TOLERANCE && t.key % 12 == f.key % 12)
        {
            let shift = (f.key as i32 - t.key as i32).div_euclid(12);
            *counts.entry(shift).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map(|(s, _)| s)
        .unwrap_or(0)
}

// how alike the notes sounding in each bar are, averaged over the bars where the real project plays
fn harmony_similarity(truth: &[Timed], found: &[Timed], bar_seconds: f64) -> f64 {
    let end = truth.iter().map(|n| n.time + n.length).fold(0.0, f64::max);
    let bars = (end / bar_seconds).ceil() as usize;
    let fill = |notes: &[Timed]| {
        let mut chroma = vec![[0.0f32; 12]; bars + 1];
        for n in notes {
            let bar = (n.time / bar_seconds).max(0.0) as usize;
            if bar < chroma.len() {
                chroma[bar][n.key as usize % 12] += n.length.min(bar_seconds) as f32;
            }
        }
        chroma
    };
    let a = fill(truth);
    let b = fill(found);
    let mut total = 0.0;
    let mut count = 0;
    for bar in 0..bars {
        if a[bar].iter().sum::<f32>() > 0.0 {
            total += math::cosine(&a[bar], &b[bar]) as f64;
            count += 1;
        }
    }
    total / count.max(1) as f64
}

// rounds to 2 decimals
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
