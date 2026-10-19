use crate::audio::math;
use crate::flp::timeline;
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
    pub melody: Score,
    pub melody_channel: String,
    pub chords: f64,
    pub harmony: f64,
    pub patterns_truth: Option<usize>,
    pub patterns_found: Option<usize>,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy)]
struct Timed {
    time: f64,
    length: f64,
    key: u8,
}

struct Voice {
    name: String,
    notes: Vec<Timed>,
}

struct Truth {
    tempo: f64,
    drums: [Vec<Timed>; 3],
    voices: Vec<Voice>,
    bass: Option<usize>,
    patterns: Option<usize>,
}

// compares a rebuild folder with the song's real notes, from its fl studio project or a slakh track folder
pub fn evaluate(truth_path: &Path, rebuild_dir: &Path) -> Result<Evaluation, String> {
    let truth = if truth_path.is_dir() {
        slakh_truth(truth_path)?
    } else {
        flp_truth(truth_path)?
    };

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

    let our_drums = ours("drums.mid")?;
    let mut drums = Vec::new();
    for (i, (name, key)) in [("kick", 36), ("snare", 38), ("hat", 42)]
        .iter()
        .enumerate()
    {
        let found: Vec<Timed> = our_drums
            .iter()
            .filter(|n| n.key == *key)
            .copied()
            .collect();
        drums.push(score(name, &truth.drums[i], &found, false));
    }

    let none = Vec::new();
    let truth_bass = truth.bass.map_or(&none, |b| &truth.voices[b].notes);
    let our_bass = ours("bass.mid")?;
    let bass = score("bass", truth_bass, &our_bass, true);
    let bass_octave_shift = octave_shift(truth_bass, &our_bass);
    let our_melody = ours("melody.mid")?;
    let (melody, melody_voice) = best_melody(&truth.voices, truth.bass, &our_melody);

    let bar_seconds = 240.0 / truth.tempo;
    let truth_chords: Vec<Timed> = truth
        .voices
        .iter()
        .enumerate()
        .filter(|&(i, _)| Some(i) != truth.bass && Some(i) != melody_voice)
        .flat_map(|(_, v)| v.notes.iter().copied())
        .collect();
    let chords = harmony_similarity(&truth_chords, &ours("chords.mid")?, bar_seconds);

    let truth_harmony: Vec<Timed> = truth
        .voices
        .iter()
        .flat_map(|v| v.notes.iter().copied())
        .collect();
    let mut our_harmony = our_bass.clone();
    our_harmony.extend(our_melody.iter().copied());
    our_harmony.extend(ours("chords.mid")?);
    let harmony = harmony_similarity(&truth_harmony, &our_harmony, bar_seconds);

    let patterns_found = std::fs::read_dir(rebuild_dir).ok().and_then(|entries| {
        entries
            .flatten()
            .find(|e| e.path().extension().is_some_and(|x| x == "flp"))
            .and_then(|e| events::read(&e.path()).ok())
            .map(|p| timeline::patterns_used(&p))
    });

    let name_of = |voice: Option<usize>| {
        voice
            .map(|v| truth.voices[v].name.clone())
            .unwrap_or_else(|| "none found".to_string())
    };
    Ok(Evaluation {
        tempo_truth: truth.tempo,
        tempo_found,
        drums,
        bass,
        bass_channel: name_of(truth.bass),
        bass_octave_shift,
        melody,
        melody_channel: name_of(melody_voice),
        chords: round(chords),
        harmony: round(harmony),
        patterns_truth: truth.patterns,
        patterns_found,
        notes: vec![
            "hits and notes count as matched within 50 ms".to_string(),
            "bass notes also need the same note name, in any octave".to_string(),
            "melody is scored against the real part, other than the bass, that it matches best"
                .to_string(),
            "clips up to a bar long on drum channels count as hits, sampled breaks are not counted"
                .to_string(),
            "harmony compares which notes sound in each bar across all melodic parts, from 0 to 1"
                .to_string(),
            "chords compares the same way, our chords against every real part but the bass and the melody"
                .to_string(),
        ],
    })
}

// the real notes of an fl studio project, with drums found by channel name
fn flp_truth(path: &Path) -> Result<Truth, String> {
    let project = events::read(path)?;
    let summary = info::summarize(&project);
    let tempo = summary.tempo.unwrap_or(120.0);
    let tick = 60.0 / tempo / project.ppq as f64;
    let notes = timeline::timeline(&project);
    let clips = timeline::audio_clips(&project);
    let channel_name = |channel: u16| {
        summary
            .channels
            .iter()
            .find(|c| c.id == channel)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("Channel {}", channel))
    };
    let timed = |start: u32, length: u32, key: u8| Timed {
        time: start as f64 * tick,
        length: length as f64 * tick,
        key,
    };

    let mut drums: [Vec<Timed>; 3] = Default::default();
    for (i, words) in [&["kick"][..], &["snare", "clap"][..], &["hat", "hh"][..]]
        .iter()
        .enumerate()
    {
        for n in notes
            .iter()
            .filter(|n| has_word(&channel_name(n.channel), words))
        {
            drums[i].push(timed(n.start, n.length, n.key));
        }
        for clip in &clips {
            if clip.length <= 4 * project.ppq as u32 && has_word(&channel_name(clip.channel), words)
            {
                drums[i].push(timed(clip.start, clip.length, 0));
            }
        }
        drums[i].sort_by(|a, b| a.time.total_cmp(&b.time));
    }

    let mut channels: Vec<u16> = notes
        .iter()
        .map(|n| n.channel)
        .filter(|&c| !is_drum(&channel_name(c)))
        .collect();
    channels.sort();
    channels.dedup();
    let voices: Vec<Voice> = channels
        .iter()
        .map(|&c| Voice {
            name: channel_name(c),
            notes: notes
                .iter()
                .filter(|n| n.channel == c)
                .map(|n| timed(n.start, n.length, n.key))
                .collect(),
        })
        .collect();
    let bass = pick_bass(&voices);
    Ok(Truth {
        tempo,
        drums,
        voices,
        bass,
        patterns: Some(timeline::patterns_used(&project)),
    })
}

// the real notes of a slakh track folder, one voice per rendered stem, all bass stems as one
fn slakh_truth(dir: &Path) -> Result<Truth, String> {
    let meta_path = dir.join("metadata.yaml");
    let meta = std::fs::read_to_string(&meta_path)
        .map_err(|e| format!("can't read {}: {}", meta_path.display(), e))?;

    let mut tempo = None;
    let mut drums: [Vec<Timed>; 3] = Default::default();
    let mut voices: Vec<Voice> = Vec::new();
    let mut bass_notes: Vec<Timed> = Vec::new();
    for stem in slakh_stems(&meta) {
        if !stem.rendered {
            continue;
        }
        let path = dir.join("MIDI").join(format!("{}.mid", stem.id));
        if !path.exists() {
            continue;
        }
        let (ppq, tracks) = midi::read(&path)?;
        let tempos: Vec<(u32, u32)> = tracks
            .iter()
            .flat_map(|t| t.tempos.iter().copied())
            .collect();
        if tempo.is_none() {
            tempo = tempos
                .first()
                .map(|&(_, micros)| 60_000_000.0 / micros as f64);
        }
        let mut notes = Vec::new();
        for track in &tracks {
            for &(start, length, key) in &track.notes {
                let time = midi::seconds_at(start, ppq, &tempos);
                notes.push(Timed {
                    time,
                    length: midi::seconds_at(start + length, ppq, &tempos) - time,
                    key,
                });
            }
        }
        notes.sort_by(|a, b| a.time.total_cmp(&b.time));

        if stem.drum {
            for note in notes {
                let slot = match note.key {
                    35 | 36 => Some(0),
                    37..=40 => Some(1),
                    42 | 44 | 46 => Some(2),
                    _ => None,
                };
                if let Some(slot) = slot {
                    drums[slot].push(note);
                }
            }
        } else if stem.class == "Bass" {
            bass_notes.extend(notes);
        } else {
            voices.push(Voice {
                name: format!("{} {} ({})", stem.id, stem.class, stem.program),
                notes,
            });
        }
    }
    let bass = if bass_notes.is_empty() {
        None
    } else {
        bass_notes.sort_by(|a, b| a.time.total_cmp(&b.time));
        voices.push(Voice {
            name: "Bass".to_string(),
            notes: bass_notes,
        });
        Some(voices.len() - 1)
    };
    Ok(Truth {
        tempo: tempo.unwrap_or(120.0),
        drums,
        voices,
        bass,
        patterns: None,
    })
}

struct SlakhStem {
    id: String,
    class: String,
    program: String,
    drum: bool,
    rendered: bool,
}

// the stems listed in a slakh metadata.yaml, read line by line
fn slakh_stems(meta: &str) -> Vec<SlakhStem> {
    let mut stems: Vec<SlakhStem> = Vec::new();
    let mut in_stems = false;
    for line in meta.lines() {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if indent == 0 {
            in_stems = text == "stems:";
            continue;
        }
        if !in_stems {
            continue;
        }
        if indent == 2 && text.ends_with(':') {
            stems.push(SlakhStem {
                id: text.trim_end_matches(':').to_string(),
                class: String::new(),
                program: String::new(),
                drum: false,
                rendered: true,
            });
            continue;
        }
        let (Some(stem), Some((field, value))) = (stems.last_mut(), text.split_once(':')) else {
            continue;
        };
        let value = value
            .trim()
            .trim_matches('\'')
            .trim_matches('"')
            .to_string();
        match field.trim() {
            "inst_class" => stem.class = value,
            "midi_program_name" => stem.program = value,
            "is_drum" => stem.drum = value == "true",
            "audio_rendered" => stem.rendered = value == "true",
            _ => {}
        }
    }
    stems
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

// the bass voice, one named bass with notes, otherwise the lowest single note line below a flat 3, if any
fn pick_bass(voices: &[Voice]) -> Option<usize> {
    if let Some(i) = voices
        .iter()
        .position(|v| has_word(&v.name, &["bass", "808", "sub"]))
    {
        return Some(i);
    }

    let mut best: Option<(usize, f32)> = None;
    for (i, voice) in voices.iter().enumerate() {
        let notes = &voice.notes;
        if notes.len() < 8 {
            continue;
        }
        let overlapping = notes
            .windows(2)
            .filter(|pair| pair[1].time < pair[0].time + pair[0].length - 1e-6)
            .count();
        if overlapping as f32 > 0.2 * notes.len() as f32 {
            continue;
        }
        let keys: Vec<f32> = notes.iter().map(|n| n.key as f32).collect();
        let median = math::percentile(&keys, 0.5);
        if median > 56.0 {
            continue;
        }
        if best.is_none_or(|(_, m)| median < m) {
            best = Some((i, median));
        }
    }
    best.map(|(i, _)| i)
}

// scores the found melody against every real voice except the bass and keeps the best match
fn best_melody(voices: &[Voice], bass: Option<usize>, found: &[Timed]) -> (Score, Option<usize>) {
    let mut best = (score("melody", &[], found, true), None);
    for (i, voice) in voices.iter().enumerate() {
        if Some(i) == bass {
            continue;
        }
        let s = score("melody", &voice.notes, found, true);
        if s.f > best.0.f {
            best = (s, Some(i));
        }
    }
    best
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

#[cfg(test)]
mod tests {
    use super::*;

    // the stems of a slakh metadata file come out with their class, program and drum flag
    #[test]
    fn reads_slakh_stems() {
        let meta = "UUID: abc\nstems:\n  S00:\n    audio_rendered: true\n    inst_class: Guitar\n    is_drum: false\n    midi_program_name: Distortion Guitar\n  S01:\n    audio_rendered: false\n    inst_class: Drums\n    is_drum: true\n    midi_program_name: Drums\ntarget_peak: -1.0\n";
        let stems = slakh_stems(meta);
        assert_eq!(stems.len(), 2);
        assert_eq!(stems[0].id, "S00");
        assert_eq!(stems[0].class, "Guitar");
        assert_eq!(stems[0].program, "Distortion Guitar");
        assert!(!stems[0].drum && stems[0].rendered);
        assert!(stems[1].drum && !stems[1].rendered);
    }
}
