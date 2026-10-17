use crate::analysis::drums;
use crate::analysis::spectrum::Spectrogram;
use crate::analysis::tempo;
use crate::audio::convert;
use crate::audio::signal::Signal;
use crate::midi;
use crate::music::{Note, Part};
use crate::report::{self, PartReport, Report};
use crate::separate;
use std::path::{Path, PathBuf};

pub struct Options {
    pub input: PathBuf,
    pub out: Option<PathBuf>,
    pub stems: Option<PathBuf>,
}

pub struct Stems {
    pub drums: Signal,
    pub bass: Signal,
    pub other: Signal,
    pub vocals: Signal,
}

// rebuilds a song into an output folder of stems, midi and a report
pub fn run(options: &Options) -> Result<Report, String> {
    let out = output_dir(options);
    let stems_dir = out.join("stems");
    let midi_dir = out.join("midi");
    create_dir(&out)?;
    create_dir(&midi_dir)?;

    progress("loading the song");
    let song = convert::load(&options.input, &out)?;
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
    let mix = Signal::from_wav(&song);

    progress("finding the tempo and bars");
    let rhythm = if stems.drums.peak() > 0.01 {
        &stems.drums
    } else {
        &mix
    };
    let mut harmony = stems.bass.clone();
    harmony.mix_in(&stems.other);
    let (_, grid) = tempo::analyze(rhythm, &harmony);
    let bars = grid.bar_count(mix.duration());

    progress("transcribing drums");
    let drum_spec = Spectrogram::standard(&stems.drums.normalized());
    let hits = drums::detect(&drum_spec);
    let drum_notes = drums::to_notes(&hits, &grid);

    let drum_track = midi::Track {
        name: "drums".to_string(),
        channel: 9,
        notes: drum_notes.clone(),
    };
    midi::write(&midi_dir.join("drums.mid"), grid.bpm, &[drum_track], &[])?;

    let mut parts = Vec::new();
    for part in [Part::Kick, Part::Snare, Part::Hat] {
        parts.push(PartReport {
            part: part.name().to_string(),
            notes: count(&drum_notes, part),
        });
    }

    let report = Report {
        source: options.input.display().to_string(),
        duration_seconds: mix.duration(),
        tempo_bpm: grid.bpm,
        first_bar_seconds: grid.origin,
        bars,
        parts,
        files: vec![
            "stems/drums.wav".to_string(),
            "stems/bass.wav".to_string(),
            "stems/other.wav".to_string(),
            "stems/vocals.wav".to_string(),
            "midi/drums.mid".to_string(),
        ],
        limitations: vec![
            "drums are sorted into kick, snare and hat only".to_string(),
            "vocals are left as audio".to_string(),
        ],
    };
    report::write_json(&report, &out.join("report.json"))?;
    progress(&format!(
        "done, {} bars of {} notes in {}",
        bars,
        drum_notes.len(),
        out.display()
    ));
    Ok(report)
}

// the folder to write into, next to where the command runs unless one was given
pub fn output_dir(options: &Options) -> PathBuf {
    if let Some(out) = &options.out {
        return out.clone();
    }
    let name = options
        .input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "song".to_string());
    PathBuf::from(format!("{}_rebuild", name))
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

// how many notes belong to a part
fn count(notes: &[Note], part: Part) -> usize {
    notes.iter().filter(|n| n.part == part).count()
}

// makes a folder and any missing parent folders
fn create_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {}", dir.display(), e))
}

// prints a progress line to stderr so stdout stays free for results
pub fn progress(message: &str) {
    eprintln!("unbake: {}", message);
}
