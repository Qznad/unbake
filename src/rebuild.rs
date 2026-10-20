use crate::analysis::chords::{self, ChordSpan};
use crate::analysis::drums::Hit;
use crate::analysis::key::Key;
use crate::analysis::sections;
use crate::analysis::sections::Section;
use crate::analysis::spectrum::{HARMONIC_FACTOR, HOP, Spectrogram};
use crate::analysis::{chroma, drums, key, notes, onsets, pitch, tempo};
use crate::arrange::{self, Arrangement, Lane};
use crate::audio::signal::Signal;
use crate::audio::{convert, loudness, math, wav};
use crate::basic_pitch;
use crate::compare;
use crate::evidence::Finding;
use crate::findings;
use crate::flp;
use crate::flp::build::{Extras, Sound, Stem};
use crate::matching;
use crate::midi::{self, Marker};
use crate::music::{self, BAR, Grid, Note, Part};
use crate::report::{self, PartReport, Report};
use crate::separate;
use crate::sounds::{self, SoundFile};
use crate::synth::{self, Voices};
use crate::theory;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const BUILT_IN_TEMPLATE: &[u8] = include_bytes!("../assets/template.flp");
const SESSION_FILE: &str = "extras/session.json";

pub struct Options {
    pub input: PathBuf,
    pub out: Option<PathBuf>,
    pub stems: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub sounds: Option<PathBuf>,
    pub fl_folder: Option<String>,
    pub match_sounds: bool,
    pub simple_chords: bool,
    pub loops: bool,
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
    pub findings: Vec<Finding>,
}

// finds the tempo, bars and key of a song file from the full mix, without separating it
pub fn analyze_song(file: &Path) -> Result<SongInfo, String> {
    let wav = convert::load(file, &std::env::temp_dir().join("unbake"))?;
    let mix = Signal::from_wav(&wav);
    let (_, grid, guesses) = tempo::analyze(&mix, &mix);
    let keys = key::ranked(&chroma::chromagram(&Spectrogram::harmonic(
        &mix.normalized(),
    )));
    let key = keys[0];
    Ok(SongInfo {
        duration_seconds: round(mix.duration(), 2),
        tempo_bpm: grid.bpm,
        first_bar_seconds: round(grid.origin, 3),
        bars: grid.bar_count(mix.duration()),
        key: key.name(),
        relative_key: key.relative().name(),
        findings: vec![findings::tempo(&guesses), findings::key(&keys)],
    })
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub description: String,
    pub from_vocals: bool,
    pub notes: Vec<Note>,
    pub finding: Finding,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub source: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub grid: Grid,
    pub key: Key,
    pub bars: u32,
    pub sections: Vec<Section>,
    pub findings: Vec<Finding>,
    pub warnings: Vec<String>,
    pub drum_hits: Vec<Hit>,
    pub drums: Vec<Note>,
    pub bass: Vec<Note>,
    pub other: Vec<Note>,
    pub chord_spans: Vec<ChordSpan>,
    pub candidates: Vec<Candidate>,
    pub melody: String,
}

impl Session {
    // the melody candidate chosen now, or the first one when that name is gone
    pub fn chosen(&self) -> Option<&Candidate> {
        self.candidates
            .iter()
            .find(|c| c.name == self.melody)
            .or(self.candidates.first())
    }

    // the other stem's notes that are not in the chosen melody, which the chords part plays
    pub fn chord_notes(&self) -> Vec<Note> {
        let taken: HashSet<(u32, u8)> = match self.chosen() {
            Some(c) if !c.from_vocals => c.notes.iter().map(|n| (n.start, n.key)).collect(),
            _ => HashSet::new(),
        };
        self.other
            .iter()
            .filter(|n| !taken.contains(&(n.start, n.key)))
            .map(|n| Note {
                part: Part::Chords,
                ..*n
            })
            .collect()
    }
}

// the session saved in a rebuild folder, everything heard so the project can be written again without listening again
pub fn load_session(folder: &Path) -> Result<Session, String> {
    let path = folder.join(SESSION_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "can't read {}, run transcribe on the song first ({})",
            path.display(),
            e
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("{} is broken: {}", path.display(), e))
}

// saves the session into a rebuild folder
pub fn save_session(folder: &Path, session: &Session) -> Result<(), String> {
    let path = folder.join(SESSION_FILE);
    let text = serde_json::to_string(session).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("can't write {}: {}", path.display(), e))
}

// picks which melody candidate the project uses and saves that choice
pub fn choose_melody(folder: &Path, name: &str) -> Result<Session, String> {
    let mut session = load_session(folder)?;
    if !session.candidates.iter().any(|c| c.name == name) {
        let names: Vec<&str> = session.candidates.iter().map(|c| c.name.as_str()).collect();
        return Err(format!(
            "no melody candidate called {}, the candidates are {}",
            name,
            names.join(", ")
        ));
    }
    session.melody = name.to_string();
    save_session(folder, &session)?;
    Ok(session)
}

// rebuilds a song into a folder fl studio can open as it is, with midi, previews and the rest in extras
pub fn run(options: &Options) -> Result<Report, String> {
    let session = transcribe(options)?;
    build(options, &session)
}

// listens to a song and saves everything it hears as a session in the rebuild folder, ready to be written as a project
pub fn transcribe(options: &Options) -> Result<Session, String> {
    let out = output_dir(options);
    let extras = out.join("extras");
    for dir in [&out, &extras.join("midi"), &extras.join("render")] {
        create_dir(dir)?;
    }

    progress("loading the song");
    let song = convert::load(&options.input, &extras)?;
    let mix = Signal::from_wav(&song);
    let converted = extras.join("source.wav");
    let song_path = if converted.exists() {
        converted
    } else {
        options.input.clone()
    };

    progress("separating stems (this is the slow part)");
    match &options.stems {
        Some(dir) => separate::copy_stems(dir, &out)?,
        None => separate::split(&song_path, &out)?,
    }
    let stems = load_stems(&out)?;
    save(&mix, &extras.join("mix.wav"))?;

    progress("finding the tempo, bars, key and sections");
    let rhythm = if stems.drums.peak() > 0.01 {
        &stems.drums
    } else {
        &mix
    };
    let mut harmony = stems.bass.clone();
    harmony.mix_in(&stems.other);
    let (_, grid, tempo_guesses) = tempo::analyze(rhythm, &harmony);
    let vocal_level = loudness::lufs(&stems.vocals);
    let sung = vocal_level > -45.0 && vocal_level > loudness::lufs(&stems.other) - 20.0;
    let keys = key::ranked(&chroma::chromagram(&Spectrogram::harmonic(
        &harmony.normalized(),
    )));
    let key = keys[0];
    let bars = grid.bar_count(mix.duration());
    let sections = sections::detect(
        &stems.drums,
        &[&stems.bass, &stems.other, &stems.vocals],
        &grid,
        bars,
    );

    progress("transcribing drums, bass, melody and chords at the same time");
    let note_dir = extras.join("basic_pitch");
    let bass_path = out.join(separate::stem_file("bass"));
    let other_path = out.join(separate::stem_file("other"));
    let vocals_path = out.join(separate::stem_file("vocals"));
    let ((drum_hits, drum_notes), heard_bass, heard_other, heard_vocals, chord_spans) =
        std::thread::scope(|scope| {
            let drums =
                scope.spawn(|| transcribe_drums(&stems.drums, &grid, options.sounds.as_deref()));
            let bass = scope.spawn(|| basic_pitch::transcribe(&bass_path, &note_dir));
            let other = scope.spawn(|| basic_pitch::transcribe(&other_path, &note_dir));
            let vocals =
                scope.spawn(|| sung.then(|| basic_pitch::transcribe(&vocals_path, &note_dir)));
            let chords = scope.spawn(|| transcribe_chords(&stems.other, &stems.bass, &grid, bars));
            (
                drums.join().expect("drum transcription crashed"),
                bass.join().expect("bass transcription crashed"),
                other.join().expect("melody transcription crashed"),
                vocals.join().expect("vocal transcription crashed"),
                chords.join().expect("chord detection crashed"),
            )
        });
    let mut warnings = Vec::new();
    let bass_from_basic_pitch = heard_bass.is_ok();
    let (bass_notes, bass_heard) = match heard_bass {
        Ok(events) => {
            let heard = basic_pitch::to_notes(&events, &grid, Part::Bass);
            (basic_pitch::lowest_line(&heard), heard)
        }
        Err(e) => {
            warnings.push(format!("{}, so bass used the built-in pitch tracker", e));
            let line = track_pitch(&stems.bass, &grid, Part::Bass, 30.0, 400.0, 0.6);
            (line.clone(), line)
        }
    };
    let mut candidates = Vec::new();
    let (other, chord_spans) = match heard_other {
        Ok(events) => {
            let heard = basic_pitch::to_notes(&events, &grid, Part::Melody);
            let progression = theory::progression(&heard, &bass_notes, &key, bars);
            let (loudest, _) = basic_pitch::split_lead(&heard);
            candidates.push(candidate(
                "loudest",
                "the loudest note starting on each step of the other stem",
                false,
                loudest,
                &heard,
            ));
            candidates.push(candidate(
                "highest",
                "the highest note starting on each step of the other stem, leaving out held block chords",
                false,
                basic_pitch::highest_line(&heard),
                &heard,
            ));
            (heard, progression)
        }
        Err(e) => {
            warnings.push(format!(
                "{}, so melody and chords used the built-in trackers",
                e
            ));
            let melody = transcribe_melody(&stems.other, &grid);
            let mut heard = melody.clone();
            heard.extend_from_slice(&chords::to_notes(&chord_spans));
            let mut tracked = candidate(
                "tracker",
                "the built-in melody tracker's line, because Basic Pitch could not run",
                false,
                melody,
                &heard,
            );
            tracked.finding = findings::melody(&tracked.notes, &heard, false);
            tracked.finding.name = "melody (tracker)".to_string();
            candidates.push(tracked);
            (heard, chord_spans)
        }
    };
    match heard_vocals {
        Some(Ok(events)) => {
            let sung_notes = basic_pitch::to_notes(&events, &grid, Part::Melody);
            let (line, _) = basic_pitch::split_lead(&sung_notes);
            candidates.push(candidate(
                "vocals",
                "the loudest sung note on each step of the vocals stem, often the song's real melody",
                true,
                line,
                &sung_notes,
            ));
        }
        Some(Err(e)) => warnings.push(format!("{}, so the vocals give no melody candidate", e)),
        None => {}
    }
    compare_candidates(&mut candidates);

    let session = Session {
        source: options.input.display().to_string(),
        duration_seconds: mix.duration(),
        sample_rate: mix.sample_rate,
        grid,
        key,
        bars,
        sections,
        findings: vec![
            findings::tempo(&tempo_guesses),
            findings::key(&keys),
            findings::drums(&drum_hits),
            findings::bass(&bass_notes, &bass_heard, bass_from_basic_pitch),
        ],
        warnings,
        drum_hits,
        drums: drum_notes,
        bass: bass_notes,
        other,
        chord_spans,
        melody: candidates
            .first()
            .map(|c| c.name.clone())
            .unwrap_or_default(),
        candidates,
    };
    save_session(&out, &session)?;
    Ok(session)
}

// one melody candidate with its finding, named after how it was picked
fn candidate(
    name: &str,
    description: &str,
    from_vocals: bool,
    notes: Vec<Note>,
    heard: &[Note],
) -> Candidate {
    let mut finding = findings::melody(&notes, heard, true);
    finding.name = format!("melody ({})", name);
    if name == "highest" {
        finding.confidence = None;
    }
    if !from_vocals {
        finding.limitations.insert(
            0,
            "the other stem can hold several instruments, so one line can jump between them"
                .to_string(),
        );
    }
    Candidate {
        name: name.to_string(),
        description: description.to_string(),
        from_vocals,
        notes,
        finding,
    }
}

// adds to each candidate after the first how many of its notes the first one also has
fn compare_candidates(candidates: &mut [Candidate]) {
    let Some(first) = candidates.first() else {
        return;
    };
    let base: HashSet<(u32, u8)> = first.notes.iter().map(|n| (n.start, n.key)).collect();
    let first_name = first.name.clone();
    for c in candidates.iter_mut().skip(1) {
        let shared = c
            .notes
            .iter()
            .filter(|n| base.contains(&(n.start, n.key)))
            .count();
        let share = shared as f64 / c.notes.len().max(1) as f64;
        c.description = format!(
            "{}, sharing {:.0}% of its notes with {}",
            c.description,
            share * 100.0,
            first_name
        );
    }
}

// writes the fl project, midi, sounds, previews and report from a session, using the melody chosen in it
pub fn build(options: &Options, session: &Session) -> Result<Report, String> {
    let out = output_dir(options);
    let extras = out.join("extras");
    for dir in [&out, &extras.join("midi"), &extras.join("render")] {
        create_dir(dir)?;
    }
    let stems = load_stems(&out)?;
    let grid = session.grid;
    let key = session.key;
    let bars = session.bars;
    let duration = session.duration_seconds;
    let markers: Vec<Marker> = session
        .sections
        .iter()
        .map(|s| Marker {
            tick: s.start_bar * BAR,
            name: s.name.clone(),
        })
        .collect();
    let mut warnings = session.warnings.clone();
    let drum_hits = &session.drum_hits;
    let chord_spans = &session.chord_spans;
    let chosen = session.chosen();
    let melody_notes: Vec<Note> = chosen
        .map(|c| {
            c.notes
                .iter()
                .map(|n| Note {
                    part: Part::Melody,
                    ..*n
                })
                .collect()
        })
        .unwrap_or_default();
    let heard_chords = session.chord_notes();
    let heard_melody = match chosen {
        Some(c) if !c.from_vocals => melody_notes.clone(),
        _ => Vec::new(),
    };
    let chord_notes = if options.simple_chords {
        theory::voice(chord_spans)
    } else {
        heard_chords.clone()
    };
    let mut findings = session.findings.clone();
    if let Some(c) = chosen {
        findings.push(c.finding.clone());
    }
    findings.push(findings::chords(chord_spans, options.simple_chords));

    progress("arranging the parts into patterns");
    let drum_notes = session.drums.clone();
    let mut bass_notes = session.bass.clone();
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
    let arrangement = if options.loops {
        arrange::arrange(&lanes)
    } else {
        arrange::whole(&lanes)
    };
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
            &extras.join("midi").join(format!("{}.mid", name)),
            grid.bpm,
            std::slice::from_ref(&track),
            &[],
        )?;
        tracks.push(track);
    }
    midi::write(&extras.join("midi/song.mid"), grid.bpm, &tracks, &markers)?;

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
        &out,
        &stems.drums,
        &stems.bass,
        &stems.other,
        drum_hits,
        &bass_notes,
        &heard_melody,
        &heard_chords,
        &grid,
        &voices,
    )?;

    let track_names: Vec<String> = lanes.iter().map(|l| l.name.clone()).collect();
    let flp_status = write_flp(
        options,
        &out,
        &arrangement,
        &track_names,
        &grid,
        &markers,
        &sounds,
        duration,
        &mut warnings,
    )?;

    progress("rendering and comparing each part");
    let measured_on = format!("bars 1 to {}", bars);
    let mut preview = Signal::silence(duration, session.sample_rate);
    let part_reports = vec![
        rebuild_part(
            ("drums", "drums"),
            &drum_notes,
            &stems.drums,
            &voices,
            &grid,
            &extras,
            &measured_on,
            &mut preview,
        )?,
        rebuild_part(
            ("bass", "bass"),
            &bass_notes,
            &stems.bass,
            &voices,
            &grid,
            &extras,
            &measured_on,
            &mut preview,
        )?,
        rebuild_part(
            ("melody and chords", "other"),
            &other_notes,
            &stems.other,
            &voices,
            &grid,
            &extras,
            &measured_on,
            &mut preview,
        )?,
    ];
    if preview.peak() > 0.99 {
        preview = preview.scaled(0.99 / preview.peak());
    }
    save(&preview, &extras.join("render/mix.wav"))?;

    let report = Report {
        source: session.source.clone(),
        duration_seconds: round(duration, 2),
        tempo_bpm: grid.bpm,
        key: key.name(),
        relative_key: key.relative().name(),
        first_bar_seconds: round(grid.origin, 3),
        bars,
        sections: session.sections.clone(),
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
        findings,
        limitations: vec![
            "section names are guesses from which stems play and how loud".to_string(),
            "vocals are left as audio".to_string(),
            "the rebuild preview uses a small built in synth, not your FL Studio sounds"
                .to_string(),
        ],
    };
    report::write_json(&report, &extras.join("report.json"))?;
    report::write_html(&report, &out.join("report.html"))?;
    progress(&format!("done, everything is in {}", out.display()));
    Ok(report)
}

// finds the drum hits in two passes, first by frequency bands, then by matching each drum's own sound
fn transcribe_drums(stem: &Signal, grid: &Grid, user_dir: Option<&Path>) -> (Vec<Hit>, Vec<Note>) {
    let spec = Spectrogram::standard(&stem.normalized());
    let first = drums::detect(&spec);

    let mut templates = Vec::new();
    for part in [Part::Kick, Part::Snare, Part::Hat] {
        if let Some(sound) = sounds::drum_sound(user_dir, stem, &first, part) {
            templates.push((part, drums::template(&sound)));
        }
    }
    let matched = drums::detect_with_templates(&spec, &templates);
    let hits = drums::merge_passes(&first, &matched);

    let notes = drums::to_notes(&hits, grid);
    (hits, notes)
}

// follows the pitch of a stem and turns it into notes on the grid
fn track_pitch(
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

// writes the fl studio project next to every sound and stem it plays
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
    let stems = separate::STEMS
        .iter()
        .map(|stem| {
            let file = separate::stem_file(stem);
            Stem {
                name: file.trim_end_matches(".wav").to_string(),
                path: format!("{}\\{}", folder, file),
                seconds,
            }
        })
        .collect();
    let extras = Extras {
        sounds: sounds
            .iter()
            .map(|s| Sound {
                part: s.part,
                path: format!("{}\\{}", folder, s.file),
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
        "{} with {} patterns, {} clips and {} markers, copy this whole folder to {} on windows",
        file,
        built.patterns,
        built.clips,
        markers.len(),
        folder
    ))
}

// where the song folder will sit on windows, fl studio looks for its sounds there first
pub fn fl_folder(options: &Options) -> String {
    match &options.fl_folder {
        Some(folder) => folder.trim_end_matches('\\').to_string(),
        None => format!(
            "%FLStudioUserData%\\Projects\\unbake\\{}",
            folder_name(options)
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
    extras: &Path,
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
        &extras.join("render").join(format!("{}.wav", stem_name)),
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
        original: separate::stem_file(stem_name),
        rebuild: format!("extras/render/{}.wav", stem_name),
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
        None => PathBuf::from(song_name(options)),
    }
}

// the name of the output folder, which is also the project folder's name on windows
fn folder_name(options: &Options) -> String {
    let out = output_dir(options);
    std::path::absolute(&out)
        .unwrap_or(out)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| song_name(options))
}

// loads the four stems from a folder as mono signals
pub fn load_stems(dir: &Path) -> Result<Stems, String> {
    let load = |name: &str| -> Result<Signal, String> {
        let path = dir.join(separate::stem_file(name));
        let work_dir = std::env::temp_dir().join("unbake").join(name);
        let wav = convert::load(&path, &work_dir)?;
        Ok(Signal::from_wav(&wav))
    };
    Ok(Stems {
        drums: load("drums")?,
        bass: load("bass")?,
        other: load("other")?,
        vocals: load("vocals")?,
    })
}

// the files in the output folder and every folder inside it, as paths relative to it
fn written_files(out: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut folders = vec![PathBuf::new()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(out.join(&folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let relative = folder.join(entry.file_name());
            if entry.path().is_dir() {
                folders.push(relative);
            } else {
                files.push(relative.to_string_lossy().replace('\\', "/"));
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
