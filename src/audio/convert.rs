use crate::audio::wav::{self, Wav};
use std::path::Path;
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
    to_wav(path, &converted)?;
    wav::read_wav(&converted)
}

// converts any audio file ffmpeg understands to a 16 bit stereo wav
pub fn to_wav(input: &Path, output: &Path) -> Result<(), String> {
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-ac", "2", "-c:a", "pcm_s16le"])
        .arg(output)
        .status()
        .map_err(|_| "ffmpeg was not found, install it to open non-wav files".to_string())?;
    if !status.success() {
        return Err(format!("ffmpeg could not convert {}", input.display()));
    }
    Ok(())
}
