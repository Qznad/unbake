use crate::analysis::sections::Section;
use crate::compare::Score;
use crate::matching::Matched;
use crate::synth::Voices;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct Report {
    pub source: String,
    pub duration_seconds: f64,
    pub tempo_bpm: f64,
    pub key: String,
    pub relative_key: String,
    pub first_bar_seconds: f64,
    pub bars: u32,
    pub sections: Vec<Section>,
    pub chords: Vec<String>,
    pub parts: Vec<PartReport>,
    pub sound_matching: Vec<Matched>,
    pub voices: Voices,
    pub flp: String,
    pub warnings: Vec<String>,
    pub files: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Serialize)]
pub struct PartReport {
    pub part: String,
    pub notes: usize,
    pub detail: String,
    pub original: String,
    pub rebuild: String,
    pub original_lufs: f64,
    pub gain_db: f64,
    pub scores: Vec<Score>,
}

// saves the report as indented json
pub fn write_json(report: &Report, path: &Path) -> Result<(), String> {
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("can't write {}: {}", path.display(), e))
}

// saves a readable page of the report with players to compare each part by ear
pub fn write_html(report: &Report, path: &Path) -> Result<(), String> {
    let mut parts = String::new();
    for part in &report.parts {
        let mut rows = String::new();
        for s in &part.scores {
            rows.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{:.2}</td><td>{}</td><td>{}</td></tr>\n",
                escape(&s.dimension),
                s.value,
                escape(&s.measured_on),
                escape(&s.note)
            ));
        }
        parts.push_str(&format!(
            "<section>\n<h2>{}</h2>\n<p>{} notes, {}. Original stem at {:.1} LUFS, rebuild turned {:+.1} dB to match.</p>\n\
             <div class=\"ab\"><label>Original <audio controls preload=\"none\" src=\"{}\"></audio></label>\n\
             <label>Rebuild <audio controls preload=\"none\" src=\"{}\"></audio></label></div>\n\
             <table><tr><th>Dimension</th><th>Score</th><th>Measured on</th><th>What it means</th></tr>\n{}</table>\n</section>\n",
            escape(&part.part),
            part.notes,
            escape(&part.detail),
            part.original_lufs,
            part.gain_db,
            escape(&part.original),
            escape(&part.rebuild),
            rows
        ));
    }

    let list = |items: &[String]| -> String {
        items
            .iter()
            .map(|i| format!("<li>{}</li>", escape(i)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let sections: Vec<String> = report
        .sections
        .iter()
        .map(|s| {
            format!(
                "{}: bars {} to {}",
                s.name,
                s.start_bar + 1,
                s.start_bar + s.bars
            )
        })
        .collect();
    let mut matching: Vec<String> = Vec::new();
    for m in &report.sound_matching {
        matching.push(format!(
            "{} stem: similarity {:.2} before, {:.2} after {} renders",
            m.stem, m.score_before, m.score_after, m.renders
        ));
    }
    for (part, sound) in [
        ("bass", report.voices.bass),
        ("melody", report.voices.melody),
        ("chords", report.voices.chords),
    ] {
        matching.push(format!(
            "{} sound: {:?} wave, filter at {:.0} Hz, release {:.0} ms, volume {:.2}",
            part,
            sound.wave,
            sound.cutoff,
            sound.release * 1000.0,
            sound.gain
        ));
    }

    let page = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>unbake report</title>\n<style>\n{}\n</style>\n</head>\n<body>\n<main>\n\
         <h1>unbake report</h1>\n<p class=\"source\">{}</p>\n\
         <dl><dt>Tempo</dt><dd>{} BPM</dd><dt>Key</dt><dd>{} (same notes as {})</dd><dt>Bars</dt><dd>{}</dd>\
         <dt>Length</dt><dd>{:.1} s</dd><dt>FL project</dt><dd>{}</dd></dl>\n\
         <div class=\"ab\"><label>Original mix <audio controls preload=\"none\" src=\"stems/mix.wav\"></audio></label>\n\
         <label>Rebuild preview <audio controls preload=\"none\" src=\"render/mix.wav\"></audio></label></div>\n\
         <h2>Sections</h2>\n<ul>{}</ul>\n<p>Chords: {}</p>\n{}\n<h2>Sound matching</h2>\n<ul>{}</ul>\n\
         <h2>Warnings</h2>\n<ul>{}</ul>\n<h2>Limits</h2>\n<ul>{}</ul>\n\
         <p class=\"small\">Scores go from 0 to 1. Every score is measured on its own stem, never the full mix.</p>\n\
         </main>\n</body>\n</html>\n",
        STYLE,
        escape(&report.source),
        report.tempo_bpm,
        escape(&report.key),
        escape(&report.relative_key),
        report.bars,
        report.duration_seconds,
        escape(&report.flp),
        list(&sections),
        escape(&report.chords.join(" ")),
        parts,
        list(&matching),
        list(&report.warnings),
        list(&report.limitations)
    );
    std::fs::write(path, page).map_err(|e| format!("can't write {}: {}", path.display(), e))
}

// makes text safe to put inside html
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = ":root { --bg: #fafaf8; --fg: #1d1d1b; --muted: #6b6b66; --line: #e2e1dc; --card: #ffffff; }
@media (prefers-color-scheme: dark) { :root { --bg: #161615; --fg: #ecebe6; --muted: #9c9b95; --line: #33332f; --card: #1e1e1c; } }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.5 system-ui, sans-serif; }
main { max-width: 860px; margin: 0 auto; padding: 24px 16px 64px; }
h1 { margin: 0 0 4px; } h2 { margin: 32px 0 8px; }
.source, .small { color: var(--muted); }
dl { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; }
dt { color: var(--muted); } dd { margin: 0; }
section { background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 4px 16px 16px; margin-top: 16px; }
.ab { display: flex; flex-wrap: wrap; gap: 16px; margin: 12px 0; }
.ab label { display: flex; flex-direction: column; gap: 4px; color: var(--muted); font-size: 14px; }
audio { width: min(380px, 100%); }
table { width: 100%; border-collapse: collapse; font-size: 14px; display: block; overflow-x: auto; }
th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--line); vertical-align: top; }
.num { font-variant-numeric: tabular-nums; }";
