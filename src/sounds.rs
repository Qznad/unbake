use crate::analysis::drums::Hit;
use crate::analysis::pitch;
use crate::analysis::spectrum::{HARMONIC_FACTOR, HOP};
use crate::audio::signal::Signal;
use crate::audio::{convert, math, wav};
use crate::music::{Grid, Note, PPQ, Part, STEP};
use crate::synth::{self, Voices};
use std::path::{Path, PathBuf};

pub const ROOT: u8 = 60;
const MOST_COMPETITION: f32 = 0.35;
const PARTS: [Part; 6] = [
    Part::Kick,
    Part::Snare,
    Part::Hat,
    Part::Bass,
    Part::Melody,
    Part::Chords,
];

pub struct SoundFile {
    pub part: Part,
    pub file: String,
    pub source: String,
    pub root: u8,
}

// one sound per part for the fl project, the user's own when given, otherwise cut from the stems or synthesized
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    user_dir: Option<&Path>,
    dir: &Path,
    drums: &Signal,
    bass: &Signal,
    other: &Signal,
    hits: &[Hit],
    bass_notes: &[Note],
    heard_melody: &[Note],
    heard_chords: &[Note],
    grid: &Grid,
    voices: &Voices,
) -> Result<Vec<SoundFile>, String> {
    let mut heard_other = heard_melody.to_vec();
    heard_other.extend_from_slice(heard_chords);
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {}", dir.display(), e))?;
    let mut out = Vec::new();

    for part in PARTS {
        let user_file = user_dir.and_then(|dir| find_user_file(dir, part));
        let (sound, root, source) = match user_file {
            Some(path) => {
                let (sound, root) = user_sound(&path, part)?;
                (sound, root, "your sound".to_string())
            }
            None if part.is_drum() => match cut_drum(drums, hits, part) {
                Some(cut) => (cut, ROOT, "cut from the drum stem".to_string()),
                None => synth_sound(part, voices, drums.sample_rate),
            },
            None if part == Part::Bass => match cut_note(bass, bass_notes, grid) {
                Some((cut, root)) => (cut, root, "cut from the bass stem".to_string()),
                None => synth_sound(part, voices, bass.sample_rate),
            },
            None if part == Part::Melody => {
                match cut_alone(other, heard_melody, &heard_other, grid) {
                    Some((cut, root)) => (cut, root, "cut from the other stem".to_string()),
                    None => synth_sound(part, voices, other.sample_rate),
                }
            }
            None => synth_sound(part, voices, other.sample_rate),
        };

        let file = format!("{}.wav", part.channel_name());
        wav::write_wav(&dir.join(&file), &sound.normalized().to_wav())?;
        out.push(SoundFile {
            part,
            file,
            source,
            root,
        });
    }
    Ok(out)
}

// the sound of a drum, the user's own when given, otherwise the cleanest hit cut from the stem
pub fn drum_sound(
    user_dir: Option<&Path>,
    stem: &Signal,
    hits: &[Hit],
    part: Part,
) -> Option<Signal> {
    if let Some(path) = user_dir.and_then(|dir| find_user_file(dir, part)) {
        if let Ok((sound, _)) = user_sound(&path, part) {
            return Some(sound);
        }
    }
    cut_drum(stem, hits, part)
}

// the user's file for a part, named after it like kick.wav, lead.mp3 or pad.flac
fn find_user_file(dir: &Path, part: Part) -> Option<PathBuf> {
    let names: &[&str] = match part {
        Part::Melody => &["melody", "lead"],
        Part::Chords => &["chords", "pad"],
        other => &[other.name()],
    };
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        if names.contains(&stem.as_str()) && path.is_file() {
            return Some(path);
        }
    }
    None
}

// loads a sound the user gave, trims the silence in front and finds the note it plays
fn user_sound(path: &Path, part: Part) -> Result<(Signal, u8), String> {
    let work_dir = std::env::temp_dir().join("unbake").join(part.name());
    let wav = convert::load(path, &work_dir)?;
    let sound = Signal::from_wav(&wav).trim_start();
    if part.is_drum() {
        return Ok((sound, ROOT));
    }
    let key = played_key(&sound).unwrap_or(ROOT as f32);
    Ok((sound, key.round().clamp(0.0, 127.0) as u8))
}

// the note a single note recording plays, measured over its clearest frames
fn played_key(sound: &Signal) -> Option<f32> {
    let low = sound.normalized().downsample(HARMONIC_FACTOR);
    let frames = pitch::track(&low, HOP / HARMONIC_FACTOR, 30.0, 1500.0);
    let keys: Vec<f32> = frames
        .iter()
        .take(200)
        .filter(|f| f.clarity > 0.8 && f.level > 0.05)
        .map(|f| math::hz_to_midi(f.hz))
        .collect();
    if keys.is_empty() {
        return None;
    }
    Some(math::percentile(&keys, 0.5))
}

// the cleanest hit of one drum in the stem, one where no other drum plays at the same time
fn cut_drum(stem: &Signal, hits: &[Hit], part: Part) -> Option<Signal> {
    let longest: f64 = match part {
        Part::Kick => 0.4,
        Part::Snare => 0.3,
        _ => 0.15,
    };
    let mut candidates: Vec<&Hit> = hits.iter().filter(|h| h.part == part).collect();
    candidates.sort_by(|a, b| b.strength.total_cmp(&a.strength));

    let alone = |hit: &Hit| {
        hits.iter().all(|other| {
            let same = other.part == hit.part && other.time == hit.time;
            same || other.time < hit.time - 0.03 || other.time > hit.time + longest.min(0.12)
        })
    };
    let chosen = candidates
        .iter()
        .find(|h| alone(h))
        .or(candidates.first())?;

    let next = hits
        .iter()
        .map(|h| h.time)
        .filter(|&t| t > chosen.time + 0.03)
        .fold(f64::MAX, f64::min);
    let end = (chosen.time + longest)
        .min(next - 0.005)
        .max(chosen.time + 0.06);
    let cut = stem.slice((chosen.time - 0.01).max(0.0), end);
    Some(cut.fade_out(0.015))
}

// the longest clear note of the bass line cut from the stem, with the note it plays
fn cut_note(stem: &Signal, notes: &[Note], grid: &Grid) -> Option<(Signal, u8)> {
    let note = notes.iter().max_by_key(|n| n.length)?;
    let start = grid.tick_time(note.start);
    let length = (note.length as f64 * grid.tick_seconds() + 0.08).min(1.0);
    if length < 0.12 {
        return None;
    }
    Some((stem.slice(start, start + length).fade_out(0.03), note.key))
}

// the note with the least else sounding around it, cut from the stem, when it is clean enough
fn cut_alone(
    stem: &Signal,
    candidates: &[Note],
    everything: &[Note],
    grid: &Grid,
) -> Option<(Signal, u8)> {
    let competition = |note: &Note| {
        let from = note.start;
        let to = note.start + note.length + STEP;
        let mut sum = 0.0f32;
        for other in everything {
            if other.start == note.start && other.key == note.key {
                continue;
            }
            let overlap = (other.start + other.length)
                .min(to)
                .saturating_sub(other.start.max(from));
            let fresh = if other.start >= from { 1.0 } else { 0.5 };
            sum += fresh * overlap as f32 * other.velocity as f32;
        }
        sum / ((to - from) as f32 * note.velocity as f32)
    };
    let (least, note) = candidates
        .iter()
        .filter(|n| n.length as f64 * grid.tick_seconds() >= 0.12)
        .map(|n| (competition(n), n))
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    if least > MOST_COMPETITION {
        return None;
    }
    let start = grid.tick_time(note.start);
    let length = (note.length as f64 * grid.tick_seconds() + 0.08).min(1.0);
    Some((stem.slice(start, start + length).fade_out(0.03), note.key))
}

// one c5 note played with our synth sound for the part, used when there is nothing better
fn synth_sound(part: Part, voices: &Voices, rate: u32) -> (Signal, u8, String) {
    let length = match part {
        Part::Chords => 4 * PPQ,
        _ => PPQ,
    };
    let note = Note {
        part,
        key: ROOT,
        velocity: 110,
        start: 0,
        length,
    };
    let grid = Grid {
        bpm: 120.0,
        origin: 0.0,
    };
    let seconds = length as f64 * grid.tick_seconds() + 0.5;
    let sound = synth::render(&[note], &grid, voices, seconds, rate).trim_start();
    (sound, ROOT, "synth".to_string())
}
