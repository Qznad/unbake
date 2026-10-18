use crate::analysis::chords::{self, ChordSpan};
use crate::analysis::drums::Hit;
use crate::analysis::sections;
use crate::analysis::spectrum::{HARMONIC_FACTOR, HOP, Spectrogram};
use crate::analysis::{chroma, drums, key, notes, onsets, pitch, tempo};
use crate::arrange::{self, Arrangement, Lane};
use crate::audio::signal::Signal;
use crate::audio::{convert, loudness, math, wav};
use crate::compare;
use crate::flp;
use crate::flp::build::{Extras, Sound, Stem};
use crate::matching;
use crate::midi::{self, Marker};
use crate::music::{self, BAR, Grid, Note, Part};
use crate::report::{self, PartReport, Report};
use crate::separate;
use crate::sounds::{self, SoundFile};
use crate::synth::{self, Voices};
use serde::Serialize;
use std::path::{Path, PathBuf};

const BUILT_IN_TEMPLATE: &[u8] = include_bytes!("../assets/template.flp");

pub struct Options {
    pub input: PathBuf,
    pub out: Option<PathBuf>,
    pub stems: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub sounds: Option<PathBuf>,
    pub fl_folder: Option<String>,
    pub match_sounds: bool,
}

pub struct Stems {
    pub drums: Signal,
    pub bass: Signal,
    pub other: Signal,
    pub vocals: Signal,
}

#[derive(Serialize)]
pub struct SongInfo {
    pub duration_seconds: f64,
    pub tempo_bpm: f64,
    pub first_bar_seconds: f64,
    pub bars: u32,
    pub key: String,
    pub relative_key: String,
}

// finds the tempo, bars and key of a song file from the full mix, without separating it
pub fn analyze_song(file: &Path) -> Result<SongInfo, String> {
    let wav = convert::load(file, &std::env::temp_dir().join("unbake"))?;
    let mix = Signal::from_wav(&wav);
    let (_, grid) = tempo::analyze(&mix, &mix);
    let key = key::detect(&chroma::chromagram(&Spectrogram::harmonic(
        &mix.normalized(),
    )));
    Ok(SongInfo {
        duration_seconds: round(mix.duration(), 2),
        tempo_bpm: grid.bpm,
        first_bar_seconds: round(grid.origin, 3),
        bars: grid.bar_count(mix.duration()),
        key: key.name(),
        relative_key: key.relative().name(),
    })
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

    progress("finding the tempo, bars, key and sections");
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
    let sections = sections::detect(
        &stems.drums,
        &[&stems.bass, &stems.other, &stems.vocals],
        &grid,
        bars,
    );
    let markers: Vec<Marker> = sections
        .iter()
        .map(|s| Marker {
            tick: s.start_bar * BAR,
            name: s.name.clone(),
        })
        .collect();

    progress("transcribing drums, bass, melody and chords at the same time");
    let ((drum_hits, drum_notes), bass_notes, melody_notes, chord_spans) =
        std::thread::scope(|scope| {
            let drums = scope.spawn(|| transcribe_drums(&stems.drums, &grid));
            let bass = scope.spawn(|| transcribe(&stems.bass, &grid, Part::Bass, 30.0, 400.0, 0.6));
            let melody = scope.spawn(|| transcribe_melody(&stems.other, &grid));
            let chords = scope.spawn(|| transcribe_chords(&stems.other, &stems.bass, &grid, bars));
            (
                drums.join().expect("drum transcription crashed"),
                bass.join().expect("bass transcription crashed"),
                melody.join().expect("melody transcription crashed"),
                chords.join().expect("chord detection crashed"),
            )
        });
    let chord_notes = chords::to_notes(&chord_spans);

    progress("finding the loops each part repeats");
    let mut bass_notes = bass_notes;
    notes::fold_octaves(&mut bass_notes, 36);
    let names = ["drums", "bass", "melody", "chords"];
    let lanes: Vec<Lane> = names
        .iter()
        .zip([drum_notes, bass_notes, melody_notes, chord_notes])
        .map(|(name, notes)| Lane {
            name: capitalize(name),
            notes,
        })
        .collect();
    let arrangement = arrange::arrange(&lanes);
    let mut cleaned = arrange::expand(&arrangement, lanes.len());
    let chord_notes = cleaned.pop().unwrap_or_default();
    let melody_notes = cleaned.pop().unwrap_or_default();
    let bass_notes = cleaned.pop().unwrap_or_default();
    let drum_notes = cleaned.pop().unwrap_or_default();

    progress("writing midi");
    let parts = [
        ("drums", 9, &drum_notes),
        ("bass", 0, &bass_notes),
        ("melody", 1, &melody_notes),
        ("chords", 2, &chord_notes),
    ];
    let mut tracks = Vec::new();
    for (name, channel, notes) in parts {
        let track = midi::Track {
            name: name.to_string(),
            channel,
            notes: notes.clone(),
        };
        midi::write(
            &out.join("midi").join(format!("{}.mid", name)),
            grid.bpm,
            std::slice::from_ref(&track),
            &[],
        )?;
        tracks.push(track);
    }
    midi::write(&out.join("midi/song.mid"), grid.bpm, &tracks, &markers)?;

    let mut other_notes = melody_notes.clone();
    other_notes.extend_from_slice(&chord_notes);
    let mut voices = Voices::defaults();
    let mut sound_matching = Vec::new();
    if options.match_sounds {
        progress("matching synth sounds to the stems");
        let start = voices;
        let (bass_match, other_match) = std::thread::scope(|scope| {
            let bass = scope.spawn(|| {
                matching::match_sounds(
                    "bass",
                    &bass_notes,
                    &[Part::Bass],
                    &stems.bass,
                    &grid,
                    start,
                )
            });
            let other = scope.spawn(|| {
                matching::match_sounds(
                    "other",
                    &other_notes,
                    &[Part::Melody, Part::Chords],
                    &stems.other,
                    &grid,
                    start,
                )
            });
            (
                bass.join().expect("bass sound matching crashed"),
                other.join().expect("other sound matching crashed"),
            )
        });
        voices.bass = bass_match.voices.bass;
        voices.melody = other_match.voices.melody;
        voices.chords = other_match.voices.chords;
        sound_matching = vec![bass_match, other_match];
    }

    progress("preparing a sound for every part");
    let sounds = sounds::prepare(
        options.sounds.as_deref(),
        &out.join("samples"),
        &stems.drums,
        &stems.bass,
        &drum_hits,
        &bass_notes,
        &grid,
        &voices,
    )?;

    let mut warnings = Vec::new();
    let track_names: Vec<String> = lanes.iter().map(|l| l.name.clone()).collect();
    let flp_status = write_flp(
        options,
        &out,
        &arrangement,
        &track_names,
        &grid,
        &markers,
        &sounds,
        mix.duration(),
        &mut warnings,
    )?;

    progress("rendering and comparing each part");
    let measured_on = format!("bars 1 to {}", bars);
    let mut preview = Signal::silence(mix.duration(), mix.sample_rate);
    let part_reports = vec![
        rebuild_part(
            ("drums", "drums"),
            &drum_notes,
            &stems.drums,
            &voices,
            &grid,
            &out,
            &measured_on,
            &mut preview,
        )?,
        rebuild_part(
            ("bass", "bass"),
            &bass_notes,
            &stems.bass,
            &voices,
            &grid,
            &out,
            &measured_on,
            &mut preview,
        )?,
        rebuild_part(
            ("melody and chords", "other"),
            &other_notes,
            &stems.other,
            &voices,
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
        sections,
        chords: chord_spans.iter().map(|c| c.chord.name()).collect(),
        parts: part_reports,
        sound_matching,
        sounds: sounds
            .iter()
            .map(|s| format!("{}: {} ({})", s.part.name(), s.source, s.file))
            .collect(),
        voices,
        flp: flp_status,
        warnings,
        files: written_files(&out),
        limitations: vec![
            "drums are sorted into kick, snare and hat only, and a hat under a snare is not found"
                .to_string(),
            "bass and melody are transcribed one note at a time".to_string(),
            "chords are major and minor triads, one per half bar at most".to_string(),
            "section names are guesses from which stems play and how loud".to_string(),
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

// finds the drum hits in the drum stem, and the same hits snapped to the grid
fn transcribe_drums(stem: &Signal, grid: &Grid) -> (Vec<Hit>, Vec<Note>) {
    let spec = Spectrogram::standard(&stem.normalized());
    let hits = drums::detect(&spec);
    let notes = drums::to_notes(&hits, grid);
    (hits, notes)
}

// follows the pitch of a stem and turns it into notes on the grid
fn transcribe(
    stem: &Signal,
    grid: &Grid,
    part: Part,
    low_hz: f32,
    high_hz: f32,
    min_clarity: f32,
) -> Vec<Note> {
    let normalized = stem.normalized();
    let low = normalized.downsample(HARMONIC_FACTOR);
    let hop = HOP / HARMONIC_FACTOR;
    let frames = pitch::track(&low, hop, low_hz, high_hz);
    let frame_rate = low.sample_rate as f64 / hop as f64;
    let attacks = onsets::detect(&Spectrogram::standard(&normalized));
    notes::from_pitch(&frames, frame_rate, &attacks, grid, part, min_clarity)
}

// follows the loudest note in the melody range, ignoring frames much quieter than the lead's usual level
fn transcribe_melody(stem: &Signal, grid: &Grid) -> Vec<Note> {
    let normalized = stem.normalized();
    let spec = Spectrogram::harmonic(&normalized);
    let mut frames = pitch::salience_track(&spec, 60, 96);
    let levels: Vec<f32> = frames.iter().map(|f| f.level).collect();
    let floor = 0.5 * math::percentile(&levels, 0.95);
    for frame in frames.iter_mut() {
        if frame.level < floor {
            frame.clarity = 0.0;
        }
    }
    let attacks = onsets::detect(&Spectrogram::standard(&normalized));
    notes::from_pitch(
        &frames,
        spec.frame_rate(),
        &attacks,
        grid,
        Part::Melody,
        0.6,
    )
}

// finds the chords of the other stem, using the bass stem to help with the roots
fn transcribe_chords(other: &Signal, bass: &Signal, grid: &Grid, bars: u32) -> Vec<ChordSpan> {
    let upper = chroma::chromagram(&Spectrogram::harmonic(&other.normalized()));
    let low = chroma::chromagram(&Spectrogram::harmonic(&bass.normalized()));
    let frame_rate = other.sample_rate as f64 / HOP as f64;
    chords::detect(&upper, &low, frame_rate, grid, bars)
}

// writes the fl studio project from the built in template or the user's own, with sounds and stems
#[allow(clippy::too_many_arguments)]
fn write_flp(
    options: &Options,
    out: &Path,
    arrangement: &Arrangement,
    track_names: &[String],
    grid: &Grid,
    markers: &[Marker],
    sounds: &[SoundFile],
    seconds: f64,
    warnings: &mut Vec<String>,
) -> Result<String, String> {
    progress("building the fl studio project");
    let built_in = options.template.is_none();
    let template = match &options.template {
        Some(path) => flp::events::read(path)?,
        None => flp::events::parse(BUILT_IN_TEMPLATE)?,
    };

    let folder = fl_folder(options);
    let origin_ticks = (grid.origin / grid.tick_seconds()).round() as u32;
    let (shift, stem_start) = if origin_ticks >= 2 {
        (BAR, BAR - origin_ticks.min(BAR))
    } else {
        (0, 0)
    };
    let stems = ["drums", "bass", "other", "vocals"]
        .iter()
        .map(|name| Stem {
            name: format!("{} stem", capitalize(name)),
            path: format!("{}\\stems\\{}.wav", folder, name),
            seconds,
        })
        .collect();
    let extras = Extras {
        sounds: sounds
            .iter()
            .map(|s| Sound {
                part: s.part,
                path: format!("{}\\samples\\{}", folder, s.file),
                root: s.root,
            })
            .collect(),
        stems,
        data_folder: Some(folder.clone()),
        reuse_audio_channels: built_in,
        shift,
        stem_start,
    };

    let built = flp::build::build(
        &template,
        arrangement,
        track_names,
        grid.bpm,
        markers,
        &extras,
    )?;
    warnings.extend(built.warnings);
    let file = format!("{}.flp", song_name(options));
    flp::events::write(&out.join(&file), &built.project)?;
    Ok(format!(
        "{} with {} patterns, {} clips and {} markers, copy this folder to {} on windows",
        file,
        built.patterns,
        built.clips,
        markers.len(),
        folder
    ))
}

// the windows folder the rebuild should be copied to so fl studio finds the samples and stems
pub fn fl_folder(options: &Options) -> String {
    match &options.fl_folder {
        Some(folder) => folder.trim_end_matches('\\').to_string(),
        None => format!(
            "%FLStudioUserData%\\Projects\\unbake\\{}",
            song_name(options)
        ),
    }
}

// renders a part, turns it to the loudness of its stem, saves it, adds it to the preview and scores it
#[allow(clippy::too_many_arguments)]
fn rebuild_part(
    (name, stem_name): (&str, &str),
    notes: &[Note],
    stem: &Signal,
    voices: &Voices,
    grid: &Grid,
    out: &Path,
    measured_on: &str,
    preview: &mut Signal,
) -> Result<PartReport, String> {
    let raw = synth::render(notes, grid, voices, stem.duration(), stem.sample_rate);
    let original_lufs = loudness::lufs(stem);
    let raw_lufs = loudness::lufs(&raw);
    let gain_db = if original_lufs.is_finite() && raw_lufs.is_finite() {
        original_lufs - raw_lufs
    } else {
        0.0
    };
    let rendered = raw.scaled(math::db_to_gain(gain_db) as f32);
    save(
        &rendered,
        &out.join("render").join(format!("{}.wav", stem_name)),
    )?;
    preview.mix_in(&rendered);

    let scores = if notes.iter().all(|n| n.part.is_drum()) {
        compare::drum_part(stem, &rendered, measured_on)
    } else {
        compare::melodic_part(stem, &rendered, grid, measured_on)
    };

    Ok(PartReport {
        part: name.to_string(),
        notes: notes.len(),
        detail: describe(notes),
        original: format!("stems/{}.wav", stem_name),
        rebuild: format!("render/{}.wav", stem_name),
        original_lufs: round(original_lufs, 1),
        gain_db: round(gain_db, 1),
        scores,
    })
}

// a short description of what was found, like drum counts or the note range of each part
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
    let mut pieces = Vec::new();
    for part in [Part::Bass, Part::Melody, Part::Chords] {
        let keys: Vec<u8> = notes
            .iter()
            .filter(|n| n.part == part)
            .map(|n| n.key)
            .collect();
        if let (Some(low), Some(high)) = (keys.iter().min(), keys.iter().max()) {
            pieces.push(format!(
                "{} {} notes from {} to {}",
                part.name(),
                keys.len(),
                music::note_name(*low),
                music::note_name(*high)
            ));
        }
    }
    pieces.join(", ")
}

// the same word with a capital first letter, used for track and pattern names
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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

// prints a progress line to stderr so stdout stays free for results and the mcp server
pub fn progress(message: &str) {
    eprintln!("unbake: {}", message);
}
