use crate::audio::wav::{self, Wav};
use std::path::{Path, PathBuf};
use std::process::Command;

// loads any audio file, converting it to wav with ffmpeg first when it isn't a 16 bit wav
pub fn load(path: &Path, work_dir: &Path) -> Result<Wav, String> {
    if let Ok(wav) = wav::read_wav(path) {
        return Ok(wav);
    }
    if !path.exists() {
        return Err(format!("can't read {}: file not found", path.display()));
    }
    std::fs::create_dir_all(work_dir)
        .map_err(|e| format!("can't create {}: {}", work_dir.display(), e))?;
    let converted = work_dir.join("source.wav");
    let input = inner_ogg(path, work_dir).unwrap_or_else(|| path.to_path_buf());
    to_wav(&input, &converted)?;
    wav::read_wav(&converted)
}

// the ogg stream image-line hides inside some of its wav files, saved on its own because ffmpeg can't read it in place
fn inner_ogg(path: &Path, work_dir: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = wav::read_u32_le(&bytes, at + 4).ok()? as usize;
        let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
        if &bytes[at..at + 4] == b"data" {
            if !body.starts_with(b"OggS") {
                return None;
            }
            let ogg = work_dir.join("source.ogg");
            std::fs::write(&ogg, body).ok()?;
            return Some(ogg);
        }
        at += 8 + size + (size & 1);
    }
    None
}

// converts any audio file ffmpeg understands to a 16 bit stereo wav
pub fn to_wav(input: &Path, output: &Path) -> Result<(), String> {
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-ac", "2", "-c:a", "pcm_s16le"])
        .arg(output)
        .status()
        .map_err(|_| {
            "ffmpeg was not found, run `unbake doctor` to see how to install it".to_string()
        })?;
    if !status.success() {
        return Err(format!("ffmpeg could not convert {}", input.display()));
    }
    Ok(())
}
