use crate::doctor::{self, Status};
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::Command;

const GPU_TORCH: [&str; 6] = [
    "--with",
    "torch",
    "--index",
    "https://download.pytorch.org/whl/cu130",
    "--index-strategy",
    "unsafe-best-match",
];

pub struct Step {
    pub what: String,
    pub program: String,
    pub args: Vec<String>,
    pub may_fail: bool,
}

pub struct Plan {
    pub steps: Vec<Step>,
    pub ready: Vec<String>,
    pub notes: Vec<String>,
}

// what setup would install and connect on this computer, worked out from doctor's checks
pub fn plan() -> Plan {
    let checks = doctor::run();
    let status = |name: &str| {
        checks
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.status.clone())
    };
    let gpu = status("gpu") == Some(Status::Ok);
    let mut steps = Vec::new();
    let mut notes = Vec::new();
    if cfg!(windows) {
        if status("ffmpeg") == Some(Status::Missing) {
            steps.push(winget(
                "install ffmpeg, which opens mp3, flac and other non-wav songs",
                "Gyan.FFmpeg",
            ));
        }
        if status("uv") == Some(Status::Missing) {
            steps.push(winget(
                "install uv, which installs Demucs and Basic Pitch",
                "astral-sh.uv",
            ));
        }
        let mut tools = false;
        match status("demucs") {
            Some(Status::Missing) => {
                steps.push(demucs(gpu, false));
                tools = true;
            }
            Some(Status::Warning) if gpu => {
                steps.push(demucs(true, true));
                tools = true;
            }
            _ => {}
        }
        if status("basic-pitch") == Some(Status::Missing) {
            steps.push(uv_tool(
                "install Basic Pitch, which turns the stems into notes",
                &["--python", "3.10", "basic-pitch", "--with", "setuptools<81"],
            ));
            tools = true;
        }
        if tools {
            steps.push(Step {
                what: "put uv's tool folder on PATH, so new terminals find Demucs and Basic Pitch"
                    .to_string(),
                program: "uv".to_string(),
                args: vec!["tool".to_string(), "update-shell".to_string()],
                may_fail: false,
            });
        }
    } else {
        for c in &checks {
            if let (Status::Missing | Status::Warning, Some(fix)) = (&c.status, &c.fix) {
                notes.push(format!("{}: {}", c.name, fix));
            }
        }
        if !notes.is_empty() {
            notes.insert(
                0,
                "setup only installs tools on Windows so far, so run these yourself:".to_string(),
            );
        }
    }
    match claude_program() {
        Some(claude) => {
            let (connecting, note) = connect(&claude);
            steps.extend(connecting);
            notes.extend(note);
        }
        None => notes.push(format!(
            "Claude Code wasn't found, so connect it yourself: claude mcp add -s user unbake -- {} mcp, or a .mcp.json as the install page shows",
            this_exe()
        )),
    }
    let ready = checks
        .iter()
        .filter(|c| c.status == Status::Ok && c.name != "unbake")
        .map(|c| c.name.clone())
        .collect();
    Plan {
        steps,
        ready,
        notes,
    }
}

// a step that installs a program with winget, the windows package manager
fn winget(what: &str, id: &str) -> Step {
    Step {
        what: what.to_string(),
        program: "winget".to_string(),
        args: [
            "install",
            "--id",
            id,
            "-e",
            "--source",
            "winget",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect(),
        may_fail: false,
    }
}

// a step that installs demucs with uv, with torch's gpu build when an nvidia card is there
fn demucs(gpu: bool, again: bool) -> Step {
    let mut args = vec![
        "--python",
        "3.11",
        "demucs",
        "--with",
        "numpy",
        "--with",
        "soundfile",
    ];
    if gpu {
        args.extend(GPU_TORCH);
    }
    if again {
        args.insert(0, "--force");
    }
    let what = match (gpu, again) {
        (true, true) => {
            "reinstall Demucs with its GPU build, since it runs on the CPU though an NVIDIA card is there (a big download)"
        }
        (true, false) => {
            "install Demucs, which splits a song into stems, with its GPU build for your NVIDIA card (a big download)"
        }
        _ => "install Demucs, which splits a song into stems",
    };
    uv_tool(what, &args)
}

// a step that installs a python tool with uv
fn uv_tool(what: &str, args: &[&str]) -> Step {
    Step {
        what: what.to_string(),
        program: "uv".to_string(),
        args: ["tool", "install"]
            .iter()
            .chain(args)
            .map(|a| a.to_string())
            .collect(),
        may_fail: false,
    }
}

// the steps that register unbake with claude code for every folder, none when it's already there, and a note when this folder has its own entry
fn connect(claude: &str) -> (Vec<Step>, Option<String>) {
    let exe = this_exe();
    let current = Command::new(claude)
        .args(["mcp", "get", "unbake"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string());
    if current
        .as_deref()
        .is_some_and(|text| same_path(text).contains(&same_path(&exe)))
    {
        return (Vec::new(), None);
    }
    let user_wide = current
        .as_deref()
        .is_some_and(|text| text.contains("Scope: User"));
    let note = current.filter(|_| !user_wide).map(|_| {
        "This folder has its own unbake entry (in .mcp.json or this folder's settings), which wins over the one setup adds while you work here.".to_string()
    });
    let mut steps = Vec::new();
    if user_wide {
        steps.push(Step {
            what: "remove the old unbake entry from Claude Code".to_string(),
            program: claude.to_string(),
            args: ["mcp", "remove", "unbake", "-s", "user"]
                .iter()
                .map(|a| a.to_string())
                .collect(),
            may_fail: true,
        });
    }
    steps.push(Step {
        what: "connect unbake to Claude Code, in every folder you open".to_string(),
        program: claude.to_string(),
        args: ["mcp", "add", "-s", "user", "unbake", "--", &exe, "mcp"]
            .iter()
            .map(|a| a.to_string())
            .collect(),
        may_fail: false,
    });
    (steps, note)
}

// this unbake.exe's path with forward slashes, the way an mcp config wants it
fn this_exe() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| "unbake".to_string())
}

// this unbake.exe's path as a command to type, in quotes only when it has spaces
pub fn command_path() -> String {
    let exe = this_exe();
    if exe.contains(' ') {
        format!("\"{}\"", exe)
    } else {
        exe
    }
}

// a path written the same way whatever its slashes and case
fn same_path(text: &str) -> String {
    text.replace('\\', "/").to_lowercase()
}

// the claude code program, on PATH or inside the vs code extension
fn claude_program() -> Option<String> {
    doctor::find_program("claude")
        .or_else(vscode_claude)
        .map(|p| p.display().to_string())
}

// the newest claude program that the vs code extension ships
fn vscode_claude() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    std::fs::read_dir(PathBuf::from(home).join(".vscode").join("extensions"))
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("anthropic.claude-code-")
        })
        .map(|entry| {
            entry
                .path()
                .join("resources")
                .join("native-binary")
                .join(name)
        })
        .filter(|path| path.is_file())
        .max_by_key(|path| path.metadata().and_then(|m| m.modified()).ok())
}

// a step's command as you would type it
pub fn shown(step: &Step) -> String {
    std::iter::once(step.program.as_str())
        .chain(step.args.iter().map(String::as_str))
        .map(|part| {
            if part.contains([' ', '<', '>', '&', '|']) {
                format!("\"{}\"", part)
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// prints what setup found and what it would do
pub fn print(plan: &Plan) {
    println!("unbake setup ({})", doctor::VERSION);
    if !plan.ready.is_empty() {
        println!("\nAlready in place: {}", plan.ready.join(", "));
    }
    if plan.steps.is_empty() {
        println!("\nNothing to install or connect.");
    } else {
        println!("\nThis will:");
        for (i, step) in plan.steps.iter().enumerate() {
            println!("  {}. {}", i + 1, step.what);
            println!("     {}", shown(step));
        }
    }
    for note in &plan.notes {
        println!("\n{}", note);
    }
}

// asks whether to go ahead, and says no when nobody can answer, as when an agent runs setup
pub fn ask() -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("\nGo ahead? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

// runs the plan's steps one by one and names the ones that failed
pub fn run(plan: &Plan) -> Vec<String> {
    let mut failed = Vec::new();
    let count = plan.steps.len();
    for (i, step) in plan.steps.iter().enumerate() {
        println!("\n[{}/{}] {}", i + 1, count, step.what);
        refresh_path();
        let program = doctor::find_program(&step.program)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| step.program.clone());
        let ok = Command::new(&program)
            .args(&step.args)
            .status()
            .is_ok_and(|s| s.success());
        if !ok && !step.may_fail {
            failed.push(format!("{} ({})", step.what, shown(step)));
        }
    }
    refresh_path();
    failed
}

// reloads PATH the way a new terminal would see it, so tools installed a moment ago are found
fn refresh_path() {
    if !cfg!(windows) {
        return;
    }
    let Ok(out) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')",
        ])
        .output()
    else {
        return;
    };
    let saved = String::from_utf8_lossy(&out.stdout);
    let mut parts: Vec<String> = saved
        .trim()
        .split(';')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    if parts.is_empty() {
        return;
    }
    for part in std::env::var("PATH").unwrap_or_default().split(';') {
        if !part.is_empty() && !parts.iter().any(|p| p.eq_ignore_ascii_case(part)) {
            parts.push(part.to_string());
        }
    }
    unsafe { std::env::set_var("PATH", parts.join(";")) };
}

#[cfg(test)]
mod tests {
    use super::*;

    // a command shows quotes only around the parts that need them
    #[test]
    fn shows_commands_as_typed() {
        let step = uv_tool(
            "install Basic Pitch",
            &["--python", "3.10", "basic-pitch", "--with", "setuptools<81"],
        );
        assert_eq!(
            shown(&step),
            "uv tool install --python 3.10 basic-pitch --with \"setuptools<81\""
        );
    }

    // the gpu build of demucs brings torch from pytorch's cuda index, and a reinstall forces it
    #[test]
    fn picks_the_demucs_build() {
        let cpu = demucs(false, false);
        assert!(!cpu.args.contains(&"torch".to_string()));
        let again = demucs(true, true);
        assert_eq!(again.args[2], "--force");
        assert!(shown(&again).contains("--index https://download.pytorch.org/whl/cu130"));
    }

    // paths match whatever their slashes and case
    #[test]
    fn compares_paths_loosely() {
        assert_eq!(
            same_path("C:\\Users\\Me\\unbake.exe"),
            same_path("c:/users/me/UNBAKE.exe")
        );
    }
}
