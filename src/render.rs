use crate::audio::wav;
use crate::compose;
use crate::flp::events::{self, Project, Value};
use crate::flp::info::{self, NOTE_SIZE};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PATTERN_BASE: u16 = 20480;
const USER_DATA: &str = "%FLStudioUserData%";
const DRUM_WORDS: [&str; 15] = [
    "kick", "snare", "clap", "hat", "hh", "crash", "ride", "cymbal", "break", "perc", "tom",
    "drum", "shaker", "rim", "snap",
];
const BASS_WORDS: [&str; 3] = ["bass", "sub", "808"];
const VOCAL_WORDS: [&str; 5] = ["vocal", "vox", "voice", "acapella", "sing"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stem {
    Drums,
    Bass,
    Other,
    Vocals,
}

#[derive(Serialize)]
pub struct Rendered {
    pub wav: String,
    pub project: String,
    pub part: String,
    pub seconds: f64,
    pub channels: Vec<String>,
}

impl Stem {
    // the stem a name like drums or bass stands for
    pub fn parse(name: &str) -> Option<Stem> {
        match name.trim().to_lowercase().as_str() {
            "drums" => Some(Stem::Drums),
            "bass" => Some(Stem::Bass),
            "other" => Some(Stem::Other),
            "vocals" => Some(Stem::Vocals),
            _ => None,
        }
    }

    // the stem's name as unbake writes it, like drums
    pub fn name(self) -> &'static str {
        match self {
            Stem::Drums => "drums",
            Stem::Bass => "bass",
            Stem::Other => "other",
            Stem::Vocals => "vocals",
        }
    }
}

// the stem a channel belongs to, judged from its name: a stem's own audio, drums, bass, vocals or everything else
pub fn stem_of(name: &str) -> Stem {
    let low = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| low.contains(w));
    if low.contains("stem") {
        for stem in [Stem::Drums, Stem::Bass, Stem::Vocals, Stem::Other] {
            if low.contains(stem.name()) {
                return stem;
            }
        }
    }
    if has(&DRUM_WORDS) {
        Stem::Drums
    } else if has(&BASS_WORDS) {
        Stem::Bass
    } else if has(&VOCAL_WORDS) {
        Stem::Vocals
    } else {
        Stem::Other
    }
}

// a copy of a project in which only one stem's channels play: the other channels' notes are taken out of every pattern and their audio clips out of the playlist, with the channels that still play
pub fn solo(project: &Project, stem: Stem) -> (Project, Vec<String>) {
    let summary = info::summarize(project);
    let keep: HashSet<u16> = summary
        .channels
        .iter()
        .filter(|c| c.automation || stem_of(&c.name) == stem)
        .map(|c| c.id)
        .collect();
    let names = summary
        .channels
        .iter()
        .filter(|c| !c.automation && stem_of(&c.name) == stem)
        .map(|c| c.name.clone())
        .collect();
    let size = info::playlist_item_size(project);
    let mut copy = project.clone();
    for event in copy.events.iter_mut() {
        if let (events::PATTERN_NOTES, Value::Data(data)) = (event.id, &mut event.value) {
            *data = data
                .chunks_exact(NOTE_SIZE)
                .filter(|note| keep.contains(&u16::from_le_bytes([note[6], note[7]])))
                .flatten()
                .copied()
                .collect();
        }
    }
    let playlist = copy
        .events
        .iter_mut()
        .find(|e| e.id == events::PLAYLIST && matches!(e.value, Value::Data(_)));
    if let Some(event) = playlist {
        if let Value::Data(data) = &mut event.value {
            let mut items: Vec<Vec<u8>> = data
                .chunks_exact(size)
                .filter(|item| {
                    let index = u16::from_le_bytes([item[6], item[7]]);
                    index > PATTERN_BASE || keep.contains(&index)
                })
                .map(|item| item.to_vec())
                .collect();
            for (n, item) in items.iter_mut().enumerate() {
                if item.len() >= 36 {
                    item[32..36].copy_from_slice(&(n as u32 + 1).to_le_bytes());
                }
            }
            *data = items.concat();
        }
    }
    (copy, names)
}

// the samples a project points at that aren't on this computer, which would stop a render with a dialog nobody can answer
pub fn missing_samples(project: &Project) -> Vec<String> {
    let mut missing: Vec<String> = info::summarize(project)
        .channels
        .iter()
        .filter_map(|c| c.sample.clone())
        .filter(|sample| {
            let path = expand(sample);
            path.is_absolute() && !path.exists()
        })
        .collect();
    missing.dedup();
    missing
}

// a sample path with fl studio's factory and user data folders filled in
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix(USER_DATA) {
        Some(rest) => {
            let home = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join("Documents")
                .join("Image-Line")
                .join("FL Studio")
                .join(rest.trim_start_matches(['\\', '/']))
        }
        None => compose::local_path(path),
    }
}

// renders a project, or only one stem's channels of it, to a wav through fl studio's command line, and waits until fl studio has written it and closed
pub fn render(
    project_path: &Path,
    stem: Option<Stem>,
    out_dir: &Path,
    timeout: Duration,
) -> Result<Rendered, String> {
    let project = events::read(project_path)?;
    let missing = missing_samples(&project);
    if !missing.is_empty() {
        return Err(format!(
            "these samples are missing, and FL Studio would stop to ask for them: {}",
            missing.join(", ")
        ));
    }
    let program = fl_program()?;
    if fl_running() {
        return Err(
            "FL Studio is open: ask the user to save their work and close it, since it can't render from the command line while it runs"
                .to_string(),
        );
    }
    let (copy, channels) = match stem {
        Some(stem) => solo(&project, stem),
        None => {
            let names = info::summarize(&project)
                .channels
                .into_iter()
                .filter(|c| !c.automation)
                .map(|c| c.name)
                .collect();
            (project.clone(), names)
        }
    };
    if stem.is_some() && channels.is_empty() {
        return Err(format!(
            "no channel of this project belongs to the {} (judged by the channel names)",
            stem.map_or("", |s| s.name())
        ));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("can't create {}: {}", out_dir.display(), e))?;
    let base = project_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());
    let part = stem.map_or("all", |s| s.name());
    let name = format!("{} - {}", base, part);
    let flp = out_dir.join(format!("{}.flp", name));
    let wav_path = out_dir.join(format!("{}.wav", name));
    if wav_path.exists() {
        std::fs::remove_file(&wav_path)
            .map_err(|e| format!("can't replace {}: {}", wav_path.display(), e))?;
    }
    events::write(&flp, &copy)?;

    let mut child = Command::new(&program)
        .arg("/Ewav")
        .arg("/R")
        .arg(&flp)
        .spawn()
        .map_err(|e| format!("can't start {}: {}", program.display(), e))?;
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return Err(format!(
                "FL Studio didn't finish rendering within {} s, so it was closed; a dialog may have been waiting in its window",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if !wav_path.exists() {
        return Err(format!(
            "FL Studio closed without writing {}",
            wav_path.display()
        ));
    }
    let seconds = wav::read_wav(&wav_path)
        .map(|w| w.info.duration_seconds())
        .unwrap_or(0.0);
    Ok(Rendered {
        wav: wav_path.display().to_string(),
        project: flp.display().to_string(),
        part: part.to_string(),
        seconds: (seconds * 100.0).round() / 100.0,
        channels,
    })
}

// where renders of a project go by default, a renders folder next to it
pub fn default_folder(project: &Path) -> PathBuf {
    project
        .parent()
        .map_or_else(|| PathBuf::from("renders"), |dir| dir.join("renders"))
}

// fl studio's program file, in the install folder that holds its factory data
fn fl_program() -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err(
            "rendering through FL Studio's command line only works on Windows for now".to_string(),
        );
    }
    let root = compose::factory_folder().ok_or("FL Studio's install folder was not found")?;
    let program = root.join("FL64.exe");
    if program.exists() {
        Ok(program)
    } else {
        Err(format!("{} was not found", program.display()))
    }
}

// whether fl studio is running, since it can't render from the command line while it's open
fn fl_running() -> bool {
    Command::new("tasklist")
        .args(["/NH", "/FO", "CSV"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.trim_start_matches('"').starts_with("FL64"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // channel names point to their stem, with drum words winning over bass words
    #[test]
    fn stems_by_name() {
        assert_eq!(stem_of("Kick"), Stem::Drums);
        assert_eq!(stem_of("Hats"), Stem::Drums);
        assert_eq!(stem_of("Break"), Stem::Drums);
        assert_eq!(stem_of("Bass drum"), Stem::Drums);
        assert_eq!(stem_of("Bass Layer"), Stem::Bass);
        assert_eq!(stem_of("Sub"), Stem::Bass);
        assert_eq!(stem_of("Pad"), Stem::Other);
        assert_eq!(stem_of("Lead (Intro)"), Stem::Other);
        assert_eq!(stem_of("Drums stem (Intro)"), Stem::Drums);
        assert_eq!(stem_of("Other stem"), Stem::Other);
        assert_eq!(stem_of("Vocal chop"), Stem::Vocals);
        assert_eq!(Stem::parse("Drums"), Some(Stem::Drums));
        assert_eq!(Stem::parse("lead"), None);
    }

    // a solo copy keeps only its stem's notes, and drops the other stems' notes from a shared pattern
    #[test]
    fn solo_keeps_one_stem() {
        let spec: compose::Loop = serde_json::from_str(
            r#"{
                "name": "solo test", "tempo": 140, "bars": 1,
                "channels": [
                    { "name": "Kick", "sample": "C:/kick.wav" },
                    { "name": "Bass", "sample": "C:/bass.wav" },
                    { "name": "Lead", "sample": "C:/lead.wav" }
                ],
                "patterns": [
                    { "name": "All", "grids": [ { "channel": "Kick", "hits": "x...x...x...x..." } ],
                      "notes": [ { "channel": "Bass", "key": "C2", "steps": 4 }, { "channel": "Lead", "key": "C5", "steps": 2 } ] }
                ]
            }"#,
        )
        .unwrap();
        let dir = std::env::temp_dir().join("unbake_solo_test");
        let written = compose::write(&spec, &dir, None).unwrap();
        let project = events::read(Path::new(&written.path)).unwrap();
        let (drums, names) = solo(&project, Stem::Drums);
        assert_eq!(names, vec!["Kick"]);
        let notes: usize = info::summarize(&drums)
            .patterns
            .iter()
            .map(|p| p.notes)
            .sum();
        assert_eq!(notes, 4);
        let (bass, _) = solo(&project, Stem::Bass);
        let notes: usize = info::summarize(&bass)
            .patterns
            .iter()
            .map(|p| p.notes)
            .sum();
        assert_eq!(notes, 1);
        assert_eq!(missing_samples(&project).len(), 3);
    }
}
