use std::process::Command;

// stamps the build with its git commit and whether the code had changes not yet committed
fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let changed = git(&[
        "status",
        "--porcelain",
        "--untracked-files=no",
        "--",
        "src",
        "Cargo.toml",
    ])
    .is_some_and(|status| !status.is_empty());
    let build = if changed {
        format!("{} with changes", commit)
    } else {
        commit
    };
    println!("cargo:rustc-env=UNBAKE_BUILD={}", build);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=src");
}
