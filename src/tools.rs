use crate::evidence::Finding;
use crate::music::{BAR, Note, STEP, note_name};
use crate::piano_roll::{self, COLORS, Layer};
use crate::rebuild::{self, Options, Session};
use crate::report::Report;
use serde::Serialize;
use std::path::{Path, PathBuf};

const PARTS: [&str; 5] = ["drums", "bass", "melody", "chords", "leftover"];

#[derive(Serialize)]
pub struct CandidateView {
    pub name: String,
    pub description: String,
    pub chosen: bool,
    pub notes: usize,
    pub finding: Finding,
}

#[derive(Serialize)]
pub struct NoteView {
    pub bar: u32,
    pub step: u32,
    pub note: String,
    pub key: u8,
    pub steps: u32,
    pub velocity: u8,
}

#[derive(Serialize)]
pub struct PartView {
    pub name: String,
    pub from_bar: u32,
    pub bars: u32,
    pub notes_in_song: usize,
    pub finding: Option<Finding>,
    pub notes: Vec<NoteView>,
}

#[derive(Serialize)]
pub struct PianoRoll {
    pub path: String,
    pub legend: Vec<String>,
    pub scale: String,
    #[serde(skip)]
    pub png: Vec<u8>,
}

// the melody candidates saved in a rebuild folder with their evidence, saying which one is chosen
pub fn candidates(folder: &Path) -> Result<Vec<CandidateView>, String> {
    let session = rebuild::load_session(folder)?;
    let chosen = session.chosen().map(|c| c.name.clone());
    Ok(session
        .candidates
        .iter()
        .map(|c| CandidateView {
            name: c.name.clone(),
            description: c.description.clone(),
            chosen: chosen.as_deref() == Some(c.name.as_str()),
            notes: c.notes.len(),
            finding: c.finding.clone(),
        })
        .collect())
}

// the notes of a part or candidate in some bars, with bars and steps counted from 1 like in fl studio
pub fn inspect(folder: &Path, name: &str, from_bar: u32, bars: u32) -> Result<PartView, String> {
    let session = rebuild::load_session(folder)?;
    let notes = notes_named(&session, name)?;
    let start = from_bar.saturating_sub(1) * BAR;
    let end = start + bars.max(1) * BAR;
    let mut shown: Vec<&Note> = notes
        .iter()
        .filter(|n| n.start >= start && n.start < end)
        .collect();
    shown.sort_by_key(|n| (n.start, n.key));
    Ok(PartView {
        name: name.to_string(),
        from_bar: from_bar.max(1),
        bars: bars.max(1),
        notes_in_song: notes.len(),
        finding: finding_named(&session, name),
        notes: shown
            .iter()
            .map(|n| NoteView {
                bar: n.start / BAR + 1,
                step: n.start % BAR / STEP + 1,
                note: note_name(n.key),
                key: n.key,
                steps: n.length.div_ceil(STEP),
                velocity: n.velocity,
            })
            .collect(),
    })
}

// draws one to four parts or candidates over each other in different colours, saves the png in extras and returns it
pub fn piano_roll(
    folder: &Path,
    names: &[String],
    from_bar: u32,
    bars: u32,
) -> Result<PianoRoll, String> {
    if names.is_empty() || names.len() > COLORS.len() {
        return Err(format!(
            "give one to {} parts or candidates to draw",
            COLORS.len()
        ));
    }
    let session = rebuild::load_session(folder)?;
    let lists: Vec<Vec<Note>> = names
        .iter()
        .map(|n| notes_named(&session, n))
        .collect::<Result<_, _>>()?;
    let layers: Vec<Layer> = lists
        .iter()
        .zip(COLORS)
        .map(|(notes, (_, color))| Layer { notes, color })
        .collect();
    let first = from_bar.max(1);
    let image = piano_roll::render(&layers, first - 1, bars.max(1))?;

    let dir = folder.join("extras/piano_roll");
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {}", dir.display(), e))?;
    let path = dir.join(format!(
        "{}_bars_{}_to_{}.png",
        names.join("_"),
        first,
        first + bars.max(1) - 1
    ));
    std::fs::write(&path, &image.png)
        .map_err(|e| format!("can't write {}: {}", path.display(), e))?;
    Ok(PianoRoll {
        path: path.display().to_string(),
        legend: names
            .iter()
            .zip(COLORS)
            .map(|(n, (color, _))| format!("{} in {}", n, color))
            .chain(
                (names.len() > 1)
                    .then(|| format!("notes in more than one of them in {}", piano_roll::SHARED.0)),
            )
            .collect(),
        scale: piano_roll::scale(&image),
        png: image.png,
    })
}

// writes the fl project from a rebuild folder's session, with the melody chosen in it and the given project options
pub fn write(folder: &Path, mut options: Options) -> Result<Report, String> {
    let session = rebuild::load_session(folder)?;
    options.input = PathBuf::from(&session.source);
    options.out = Some(folder.to_path_buf());
    rebuild::build(&options, &session)
}

// the notes of a part, the chosen melody, or a melody candidate, by name
pub fn notes_named(session: &Session, name: &str) -> Result<Vec<Note>, String> {
    match name {
        "drums" => Ok(session.drums.clone()),
        "bass" => Ok(session.bass.clone()),
        "melody" => Ok(session
            .chosen()
            .map(|c| c.notes.clone())
            .unwrap_or_default()),
        "chords" => Ok(session.held_chords()),
        "leftover" => Ok(session.leftover_notes()),
        other => session
            .candidates
            .iter()
            .find(|c| c.name == other)
            .map(|c| c.notes.clone())
            .ok_or_else(|| {
                let mut names: Vec<String> = PARTS.iter().map(|s| s.to_string()).collect();
                names.extend(session.candidates.iter().map(|c| c.name.clone()));
                format!("nothing called {}, try one of {}", other, names.join(", "))
            }),
    }
}

// the finding saved for a part or candidate, if there is one
fn finding_named(session: &Session, name: &str) -> Option<Finding> {
    let candidate = match name {
        "melody" => session.chosen(),
        other => session.candidates.iter().find(|c| c.name == other),
    };
    candidate
        .map(|c| c.finding.clone())
        .or_else(|| session.findings.iter().find(|f| f.name == name).cloned())
}
