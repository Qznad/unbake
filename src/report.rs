use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct Report {
    pub source: String,
    pub duration_seconds: f64,
    pub tempo_bpm: f64,
    pub first_bar_seconds: f64,
    pub bars: u32,
    pub parts: Vec<PartReport>,
    pub files: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Serialize)]
pub struct PartReport {
    pub part: String,
    pub notes: usize,
}

// saves the report as indented json
pub fn write_json(report: &Report, path: &Path) -> Result<(), String> {
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("can't write {}: {}", path.display(), e))
}
