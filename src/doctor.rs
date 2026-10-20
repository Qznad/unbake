use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warning,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    pub needed_for: String,
    pub fix: Option<String>,
}

// checks every outside program unbake runs, and says how to install or fix what is missing
pub fn run() -> Vec<Check> {
    let uv = find_program("uv");
    let gpu = gpu_name();
    vec![
        ffmpeg(),
        demucs(uv.as_deref(), gpu.as_deref()),
        basic_pitch(),
        gpu_check(gpu.as_deref()),
        uv_check(uv.as_deref()),
    ]
}

// ffmpeg, which opens every audio format other than wav
fn ffmpeg() -> Check {
    let needed_for = "opening mp3, flac and other non-wav songs".to_string();
    match first_line("ffmpeg", &["-version"]) {
        Some(line) => check("ffmpeg", Status::Ok, line, needed_for, None),
        None => check(
            "ffmpeg",
            Status::Missing,
            "not found on PATH".to_string(),
            needed_for,
            Some(by_system(
                "winget install Gyan.FFmpeg",
                "brew install ffmpeg",
                "install ffmpeg with your package manager, like sudo apt install ffmpeg",
            )),
        ),
    }
}

// demucs, which splits a song into stems, and whether its torch can use the gpu
fn demucs(uv: Option<&Path>, gpu: Option<&str>) -> Check {
    let needed_for =
        "splitting songs into drums, bass, other and vocals (skip it with --stems)".to_string();
    let Some(path) = find_program("demucs") else {
        return check(
            "demucs",
            Status::Missing,
            "not found on PATH".to_string(),
            needed_for,
            Some(
                "uv tool install --python 3.11 demucs --with numpy --with soundfile, then add uv's tool folder to PATH with uv tool update-shell"
                    .to_string(),
            ),
        );
    };
    let torch = uv
        .and_then(|uv| tool_python(uv, "demucs"))
        .and_then(|python| {
            first_line(
                &python.to_string_lossy(),
                &[
                    "-c",
                    "import torch; print(torch.__version__, torch.cuda.is_available())",
                ],
            )
        });
    match (torch, gpu) {
        (Some(line), Some(card)) if line.ends_with("False") => check(
            "demucs",
            Status::Warning,
            format!(
                "{}, torch {} runs on the CPU though a GPU ({}) is there",
                path.display(),
                line.trim_end_matches(" False"),
                card
            ),
            needed_for,
            Some(
                "for a GPU build, uv tool install --force --python 3.11 demucs --with numpy --with soundfile --with torch --index https://download.pytorch.org/whl/cu130 --index-strategy unsafe-best-match"
                    .to_string(),
            ),
        ),
        (Some(line), _) => {
            let device = if line.ends_with("True") { "GPU" } else { "CPU" };
            check(
                "demucs",
                Status::Ok,
                format!(
                    "{}, torch {} on the {}",
                    path.display(),
                    line.split(' ').next().unwrap_or(""),
                    device
                ),
                needed_for,
                None,
            )
        }
        (None, _) => check(
            "demucs",
            Status::Ok,
            format!(
                "{}, its torch could not be checked because it was not installed with uv",
                path.display()
            ),
            needed_for,
            None,
        ),
    }
}

// basic pitch, which hears the bass, melody and chord notes
fn basic_pitch() -> Check {
    let needed_for =
        "the bass, melody and chord notes (without it the weaker built-in trackers are used)"
            .to_string();
    match find_program("basic-pitch") {
        Some(path) => check(
            "basic-pitch",
            Status::Ok,
            path.display().to_string(),
            needed_for,
            None,
        ),
        None => check(
            "basic-pitch",
            Status::Missing,
            "not found on PATH".to_string(),
            needed_for,
            Some("uv tool install --python 3.10 basic-pitch --with \"setuptools<81\"".to_string()),
        ),
    }
}

// an nvidia graphics card, which makes demucs several times faster
fn gpu_check(gpu: Option<&str>) -> Check {
    let needed_for = "faster stem splitting, optional".to_string();
    match gpu {
        Some(card) => check("gpu", Status::Ok, card.to_string(), needed_for, None),
        None => check(
            "gpu",
            Status::Warning,
            "no NVIDIA GPU found, demucs will run on the CPU and take a few minutes per song"
                .to_string(),
            needed_for,
            None,
        ),
    }
}

// uv, the installer the fixes above use
fn uv_check(uv: Option<&Path>) -> Check {
    let needed_for = "installing demucs and basic-pitch".to_string();
    match uv {
        Some(path) => check(
            "uv",
            Status::Ok,
            path.display().to_string(),
            needed_for,
            None,
        ),
        None => check(
            "uv",
            Status::Missing,
            "not found on PATH".to_string(),
            needed_for,
            Some(by_system(
                "winget install astral-sh.uv",
                "brew install uv",
                "curl -LsSf https://astral.sh/uv/install.sh | sh",
            )),
        ),
    }
}

// a check with its fields filled in
fn check(
    name: &str,
    status: Status,
    detail: String,
    needed_for: String,
    fix: Option<String>,
) -> Check {
    Check {
        name: name.to_string(),
        status,
        detail,
        needed_for,
        fix,
    }
}

// the install command for this system
fn by_system(windows: &str, mac: &str, linux: &str) -> String {
    match std::env::consts::OS {
        "windows" => windows.to_string(),
        "macos" => mac.to_string(),
        _ => linux.to_string(),
    }
}

// the name of the first nvidia graphics card and its memory, if nvidia-smi finds one
fn gpu_name() -> Option<String> {
    first_line(
        "nvidia-smi",
        &["--query-gpu=name,memory.total", "--format=csv,noheader"],
    )
}

// the python inside a tool that uv installed, where its packages live
fn tool_python(uv: &Path, tool: &str) -> Option<PathBuf> {
    let dir = first_line(&uv.to_string_lossy(), &["tool", "dir"])?;
    let root = PathBuf::from(dir).join(tool);
    [
        root.join("Scripts").join("python.exe"),
        root.join("bin").join("python"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

// the first line a program prints, if it runs and succeeds
fn first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

// where a program is on PATH, trying the windows endings too
pub fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let endings: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_string())
            .split(';')
            .map(|e| e.to_lowercase())
            .chain([String::new()])
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ending in &endings {
            let candidate = dir.join(format!("{}{}", name, ending));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // a program that can't exist is not found, and the shell that runs the tests is
    #[test]
    fn finds_programs_on_path() {
        assert!(find_program("unbake-no-such-program").is_none());
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(find_program(shell).is_some());
    }
}
