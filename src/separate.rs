use std::path::Path;
use std::process::Command;

pub const STEMS: [&str; 4] = ["drums", "bass", "other", "vocals"];

// the file name a stem is saved under, like "Drums stem.wav"
pub fn stem_file(stem: &str) -> String {
    let (first, rest) = stem.split_at(1);
    format!("{}{} stem.wav", first.to_uppercase(), rest)
}

// true when the folder already has all four stems, so the slow separation can be skipped
pub fn has_stems(dir: &Path) -> bool {
    STEMS.iter().all(|stem| dir.join(stem_file(stem)).exists())
}

// splits a song into four stems with demucs and saves them in a folder
pub fn split(song: &Path, dir: &Path) -> Result<(), String> {
    if has_stems(dir) {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {}", dir.display(), e))?;

    let work = dir.join("demucs");
    let status = Command::new("demucs")
        .args(["-n", "htdemucs", "-o"])
        .arg(&work)
        .arg(song)
        .status()
        .map_err(|_| {
            "demucs was not found, install it with `uv tool install --python 3.11 demucs` \
             or pass --stems with a folder holding drums.wav, bass.wav, other.wav and vocals.wav"
                .to_string()
        })?;
    if !status.success() {
        return Err("demucs failed to separate the song".to_string());
    }

    let name = song
        .file_stem()
        .ok_or("the song path has no file name".to_string())?;
    let result = work.join("htdemucs").join(name);
    for stem in STEMS {
        let from = result.join(format!("{}.wav", stem));
        let to = dir.join(stem_file(stem));
        std::fs::rename(&from, &to).map_err(|e| format!("can't move {}: {}", from.display(), e))?;
    }
    std::fs::remove_dir_all(&work).map_err(|e| format!("can't clean up demucs files: {}", e))
}

// copies stems from a folder that was separated some other way, named like drums.wav or Drums stem.wav
pub fn copy_stems(from: &Path, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {}", dir.display(), e))?;
    for stem in STEMS {
        let named = from.join(stem_file(stem));
        let source = if named.exists() {
            named
        } else {
            from.join(format!("{}.wav", stem))
        };
        let target = dir.join(stem_file(stem));
        if source == target {
            continue;
        }
        std::fs::copy(&source, &target)
            .map_err(|e| format!("can't copy {}: {}", source.display(), e))?;
    }
    Ok(())
}
