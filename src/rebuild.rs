use crate::analysis::spectrum::{HARMONIC_FACTOR, HOP, Spectrogram};
use crate::analysis::{chroma, drums, key, notes, onsets, pitch, tempo};
use crate::arrange::{self, Lane};
use crate::audio::signal::Signal;
use crate::audio::{convert, loudness, math, wav};
use crate::compare;
use crate::flp;
use crate::midi;
use crate::music::{self, Grid, Note, Part};
use crate::report::{self, PartReport, Report};
use crate::separate;
use crate::synth::{self, Sound};
use std::path::{Path, PathBuf};

pub struct Options {
    pub input: PathBuf,
    pub out: Option<PathBuf>,
    pub stems: Option<PathBuf>,
    pub template: Option<PathBuf>,
}

pub struct Stems {
    pub drums: Signal,
    pub bass: Signal,
    pub other: Signal,
    pub vocals: Signal,
}

// rebuilds a song into an output folder of stems, midi, an flp, a preview and a report
pub fn run(options: &Options) -> Result<Report, String> {
    let out = output_dir(options);
    let stems_dir = out.join("stems");
    for dir in [&out, &stems_dir, &out.join("midi"), &out.join("render")] {
        create_dir(dir)?;
    }

    progress("loading the song");
    let song = convert::load(&options.input, &out)?;
    let mix = Signal::from_wav(&song);
    let converted = out.join("source.wav");
    let song_path = if converted.exists() {
        converted
    } else {
        options.input.clone()
    };

    progress("separating stems (this is the slow part)");
    match &options.stems {
        Some(dir) => separate::copy_stems(dir, &stems_dir)?,
        None => separate::split(&song_path, &stems_dir)?,
    }
    let stems = load_stems(&stems_dir)?;
    save(&mix, &stems_dir.join("mix.wav"))?;

    progress("finding the tempo, bars and key");
    let rhythm = if stems.drums.peak() > 0.01 {
        &stems.drums
    } else {
        &mix
    };
    let mut harmony = stems.bass.clone();
    harmony.mix_in(&stems.other);
    let (_, grid) = tempo::analyze(rhythm, &harmony);
    let key = key::detect(&chroma::chromagram(&Spectrogram::harmonic(
        &harmony.normalized(),
    )));
    let bars = grid.bar_count(mix.duration());

    progress("transcribing drums and bass");
    let drum_spec = Spectrogram::standard(&stems.drums.normalized());
    let drum_notes = drums::to_notes(&drums::detect(&drum_spec), &grid);
    let bass_notes = transcribe(&stems.bass, &grid, Part::Bass, 30.0, 400.0);

    progress("writing midi");
    let tracks = vec![
        midi::Track {
            name: "drums".to_string(),
            channel: 9,
            notes: drum_notes.clone(),
        },
        midi::Track {
            name: "bass".to_string(),
            channel: 0,
            notes: bass_notes.clone(),
        },
    ];
    midi::write(&out.join("midi/drums.mid"), grid.bpm, &tracks[0..1], &[])?;
    midi::write(&out.join("midi/bass.mid"), grid.bpm, &tracks[1..2], &[])?;
    midi::write(&out.join("midi/song.mid"), grid.bpm, &tracks, &[])?;

    let lanes = vec![
        Lane {
            name: "Drums".to_string(),
            notes: drum_notes.clone(),
        },
        Lane {
            name: "Bass".to_string(),
            notes: bass_notes.clone(),
        },
    ];
    let mut warnings = Vec::new();
    let flp_status = write_flp(options, &out, &lanes, &grid, &mut warnings)?;

    progress("rendering and comparing each part");
    let measured_on = format!("bars 1 to {}", bars);
    let mut preview = Signal::silence(mix.duration(), mix.sample_rate);
    let parts = vec![
        rebuild_part(
            "drums",
            "drums",
            &drum_notes,
            &stems.drums,
            &grid,
            &out,
            &measured_on,
            &mut preview,
        )?,
        rebuild_part(
            "bass",
            "bass",
            &bass_notes,
            &stems.bass,
            &grid,
            &out,
            &measured_on,
            &mut preview,
        )?,
    ];
    if preview.peak() > 0.99 {
        preview = preview.scaled(0.99 / preview.peak());
    }
    save(&preview, &out.join("render/mix.wav"))?;

    let report = Report {
        source: options.input.display().to_string(),
        duration_seconds: round(mix.duration(), 2),
        tempo_bpm: grid.bpm,
        key: key.name(),
        relative_key: key.relative().name(),
        first_bar_seconds: round(grid.origin, 3),
        bars,
        parts,
        flp: flp_status,
        warnings,
        files: written_files(&out),
        limitations: vec![
            "drums are sorted into kick, snare and hat only, and a hat under a snare is not found"
                .to_string(),
            "bass is transcribed one note at a time".to_string(),
            "vocals are left as audio".to_string(),
            "the rebuild preview uses a small built in synth, not your FL Studio sounds"
                .to_string(),
            "the .flp writer follows the FL Studio 20.8 file layout and was not opened in FL Studio here"
                .to_string(),
        ],
    };
    report::write_json(&report, &out.join("report.json"))?;
    report::write_html(&report, &out.join("report.html"))?;
    progress(&format!("done, everything is in {}", out.display()));
    Ok(report)
}

// follows the pitch of a stem and turns it into notes on the grid
fn transcribe(stem: &Signal, grid: &Grid, part: Part, low_hz: f32, high_hz: f32) -> Vec<Note> {
    let normalized = stem.normalized();
    let low = normalized.downsample(HARMONIC_FACTOR);
    let hop = HOP / HARMONIC_FACTOR;
    let frames = pitch::track(&low, hop, low_hz, high_hz);
    let frame_rate = low.sample_rate as f64 / hop as f64;
    let attacks = onsets::detect(&Spectrogram::standard(&normalized));
    notes::from_pitch(&frames, frame_rate, &attacks, grid, part, 0.6)
}

// writes the fl studio project when a template was given, and says what happened
fn write_flp(
    options: &Options,
    out: &Path,
    lanes: &[Lane],
    grid: &Grid,
    warnings: &mut Vec<String>,
) -> Result<String, String> {
    let Some(template_path) = &options.template else {
        return Ok(
            "not written, pass --template with an FL Studio project that has channels \
                   named Kick, Snare, Hat and Bass"
                .to_string(),
        );
    };
    progress("building the fl studio project");
    let template = flp::events::read(template_path)?;
    let arrangement = arrange::arrange(lanes);
    let names: Vec<String> = lanes.iter().map(|l| l.name.clone()).collect();
    let built = flp::build::build(&template, &arrangement, &names, grid.bpm, &[])?;
    warnings.extend(built.warnings);

    let file = format!("{}.flp", song_name(options));
    flp::events::write(&out.join(&file), &built.project)?;
    Ok(format!(
        "{} with {} patterns and {} clips",
        file,
        arrangement.patterns.len(),
        arrangement.clips.len()
    ))
}

// renders a part, turns it to the loudness of its stem, saves it, adds it to the preview and scores it
#[allow(clippy::too_many_arguments)]
fn rebuild_part(
    name: &str,
    stem_name: &str,
    notes: &[Note],
    stem: &Signal,
    grid: &Grid,
    out: &Path,
    measured_on: &str,
    preview: &mut Signal,
) -> Result<PartReport, String> {
    let drums = notes.iter().all(|n| n.part.is_drum());
    let sound = match notes.first() {
        Some(note) => Sound::default_for(note.part),
        None => Sound::default_for(Part::Bass),
    };
    let raw = synth::render(notes, grid, &sound, stem.duration(), stem.sample_rate);

    let original_lufs = loudness::lufs(stem);
    let raw_lufs = loudness::lufs(&raw);
    let gain_db = if original_lufs.is_finite() && raw_lufs.is_finite() {
        original_lufs - raw_lufs
    } else {
        0.0
    };
    let rendered = raw.scaled(math::db_to_gain(gain_db) as f32);
    save(&rendered, &out.join("render").join(format!("{}.wav", name)))?;
    preview.mix_in(&rendered);

    let scores = if drums {
        compare::drum_part(stem, &rendered, measured_on)
    } else {
        compare::melodic_part(stem, &rendered, grid, measured_on)
    };

    Ok(PartReport {
        part: name.to_string(),
        notes: notes.len(),
        detail: describe(notes),
        original: format!("stems/{}.wav", stem_name),
        rebuild: format!("render/{}.wav", name),
        original_lufs: round(original_lufs, 1),
        gain_db: round(gain_db, 1),
        scores,
    })
}

// a short description of what was found, like drum counts or the note range
fn describe(notes: &[Note]) -> String {
    if notes.is_empty() {
        return "nothing was found".to_string();
    }
    if notes[0].part.is_drum() {
        return format!(
            "kick {}, snare {}, hat {}",
            count(notes, Part::Kick),
            count(notes, Part::Snare),
            count(notes, Part::Hat)
        );
    }
    let low = notes.iter().map(|n| n.key).min().unwrap_or(0);
    let high = notes.iter().map(|n| n.key).max().unwrap_or(0);
    format!(
        "from {} to {}",
        music::note_name(low),
        music::note_name(high)
    )
}

// the song file name without its extension
pub fn song_name(options: &Options) -> String {
    options
        .input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "song".to_string())
}

// the folder to write into, next to where the command runs unless one was given
pub fn output_dir(options: &Options) -> PathBuf {
    match &options.out {
        Some(out) => out.clone(),
        None => PathBuf::from(format!("{}_rebuild", song_name(options))),
    }
}

// loads the four stems from a folder as mono signals
pub fn load_stems(dir: &Path) -> Result<Stems, String> {
    let load = |name: &str| -> Result<Signal, String> {
        let path = dir.join(format!("{}.wav", name));
        let wav = convert::load(&path, &dir.join("converted").join(name))?;
        Ok(Signal::from_wav(&wav))
    };
    Ok(Stems {
        drums: load("drums")?,
        bass: load("bass")?,
        other: load("other")?,
        vocals: load("vocals")?,
    })
}

// the files in the output folder, as paths relative to it
fn written_files(out: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in ["", "stems", "midi", "render"] {
        let Ok(entries) = std::fs::read_dir(out.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                files.push(if dir.is_empty() {
                    name
                } else {
                    format!("{}/{}", dir, name)
                });
            }
        }
    }
    files.sort();
    files
}

// how many notes belong to a part
fn count(notes: &[Note], part: Part) -> usize {
    notes.iter().filter(|n| n.part == part).count()
}

// rounds a number to a number of decimals
fn round(value: f64, decimals: i32) -> f64 {
    let factor = 10f64.powi(decimals);
    (value * factor).round() / factor
}

// makes a folder and any missing parent folders
fn create_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {}", dir.display(), e))
}

// saves a signal as a wav file
fn save(signal: &Signal, path: &Path) -> Result<(), String> {
    wav::write_wav(path, &signal.to_wav())
}

// prints a progress line to stderr so stdout stays free for results
pub fn progress(message: &str) {
    eprintln!("unbake: {}", message);
}
