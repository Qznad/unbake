use crate::audio::signal::Signal;
use crate::audio::wav;
use crate::listen;
use crate::music::{BAR, Note, Part, STEP};
use crate::rebuild::{self, Session};
use crate::separate;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

const ECHO_LAGS: [(u32, &str); 6] = [
    (1, "1/16"),
    (2, "1/8"),
    (3, "dotted 1/8 (3/16)"),
    (4, "1/4"),
    (6, "dotted 1/4 (3/8)"),
    (8, "1/2"),
];
const ECHO_SHARE: f64 = 0.15;
const ECHO_EXCESS: f64 = 0.08;
const ENOUGH_NOTES: usize = 16;
const PUMP_DB: f64 = 3.0;
const WET_DB: f64 = 12.0;
const DRY_DB: f64 = 25.0;
const WIDE: f64 = 0.15;
const STEPS: usize = 16;

#[derive(Serialize)]
pub struct Echo {
    pub lag: String,
    pub quieter: f64,
    pub louder: f64,
}

#[derive(Serialize)]
pub struct PartClues {
    pub part: String,
    pub echoes: Vec<Echo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pump_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    pub clues: Vec<String>,
}

#[derive(Serialize)]
pub struct Clues {
    pub song_bars: String,
    pub parts: Vec<PartClues>,
}

// the signs of effects in the song's stems over some bars: notes coming back quieter at a tempo synced gap (delay), sound filling the gaps between notes (reverb), parts dipping on each kick (sidechain) and stereo width
pub fn effect_clues(folder: &Path, from_bar: u32, bars: Option<u32>) -> Result<Clues, String> {
    let session = rebuild::load_session(folder)?;
    let first = from_bar.max(1) - 1;
    let last = bars
        .map_or(session.bars, |b| first + b.max(1))
        .min(session.bars);
    if first >= last {
        return Err(format!("the song has {} bars", session.bars));
    }
    let kicks = kick_steps(&session, first, last);
    let mut parts = Vec::new();
    for (name, notes) in [
        ("drums", &session.drums),
        ("bass", &session.bass),
        ("other", &session.other),
    ] {
        let inside: Vec<&Note> = notes
            .iter()
            .filter(|n| n.start / BAR >= first && n.start / BAR < last)
            .collect();
        let path = folder.join(separate::stem_file(name));
        let Ok(stem) = wav::read_wav(&path) else {
            continue;
        };
        let signal = Signal::from_wav(&stem);
        let level = listen::levels(&signal, &session.grid, last as usize);
        let range = first as usize..last as usize;
        let space = listen::mean(&listen::spaces(&signal, &session.grid, &level)[range.clone()]);
        let width = listen::mean(&listen::widths(&stem, &session.grid, &level)[range.clone()]);
        let pump = (name != "drums")
            .then(|| pump(&signal, &session, &level, &kicks, first, last))
            .flatten();
        let echoes = echoes(&inside, name == "drums");
        parts.push(part_clues(name, echoes, space, pump, width));
    }
    Ok(Clues {
        song_bars: format!("{}-{}", first + 1, last),
        parts,
    })
}

// how often a note comes back at each tempo synced gap, quieter as an echo would or as loud or louder as a repeated riff would, best echo first
fn echoes(notes: &[&Note], drums: bool) -> Vec<Echo> {
    if notes.len() < ENOUGH_NOTES {
        return Vec::new();
    }
    let identity = |n: &Note| -> i32 {
        if drums {
            match n.part {
                Part::Kick => -1,
                Part::Snare => -2,
                Part::Hat => -3,
                _ => -4,
            }
        } else {
            n.key as i32
        }
    };
    let mut at: HashMap<(u32, i32), u8> = HashMap::new();
    for note in notes {
        let place = (note.start / STEP, identity(note));
        let loudest = at.entry(place).or_insert(0);
        *loudest = (*loudest).max(note.velocity);
    }
    let total = notes.len() as f64;
    let mut found: Vec<Echo> = ECHO_LAGS
        .iter()
        .map(|&(lag, name)| {
            let (mut quieter, mut louder) = (0, 0);
            for note in notes {
                let later = (note.start / STEP + lag, identity(note));
                match at.get(&later) {
                    Some(&v) if v < note.velocity => quieter += 1,
                    Some(_) => louder += 1,
                    None => {}
                }
            }
            Echo {
                lag: name.to_string(),
                quieter: round(quieter as f64 / total),
                louder: round(louder as f64 / total),
            }
        })
        .collect();
    found.sort_by(|a, b| (b.quieter - b.louder).total_cmp(&(a.quieter - a.louder)));
    found.truncate(3);
    found
}

// the 16th steps of each bar where a kick hits, by 0 based bar
fn kick_steps(session: &Session, first: u32, last: u32) -> HashMap<u32, Vec<usize>> {
    let mut out: HashMap<u32, Vec<usize>> = HashMap::new();
    for hit in session.drums.iter().filter(|n| n.part == Part::Kick) {
        let bar = hit.start / BAR;
        if bar >= first && bar < last {
            out.entry(bar)
                .or_default()
                .push(((hit.start % BAR) / STEP) as usize);
        }
    }
    out
}

// how many decibels a part drops on the step of each kick and the one after it, against the steps well away from kicks, as sidechain pumping does
fn pump(
    signal: &Signal,
    session: &Session,
    level: &[Option<f64>],
    kicks: &HashMap<u32, Vec<usize>>,
    first: u32,
    last: u32,
) -> Option<f64> {
    let grid = &session.grid;
    let rate = signal.sample_rate as f64;
    let step = grid.bar_seconds() / STEPS as f64;
    let mut dips = Vec::new();
    for bar in first..last {
        if level.get(bar as usize).copied().flatten().is_none() {
            continue;
        }
        let Some(hits) = kicks.get(&bar) else {
            continue;
        };
        let start = grid.origin + bar as f64 * grid.bar_seconds();
        let steps: Vec<f64> = (0..STEPS)
            .map(|s| {
                let from = (((start + s as f64 * step) * rate) as usize).min(signal.samples.len());
                let to =
                    (((start + (s + 1) as f64 * step) * rate) as usize).min(signal.samples.len());
                let part = &signal.samples[from..to];
                let power = part.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>()
                    / part.len().max(1) as f64;
                10.0 * (power + 1e-12).log10()
            })
            .collect();
        let near = |s: usize| hits.iter().any(|&k| s == k || s == (k + 1) % STEPS);
        let away = |s: usize| {
            hits.iter()
                .all(|&k| (s + STEPS - k) % STEPS >= 3 && (k + STEPS - s) % STEPS >= 2)
        };
        let on: Vec<f64> = (0..STEPS).filter(|&s| near(s)).map(|s| steps[s]).collect();
        let off: Vec<f64> = (0..STEPS).filter(|&s| away(s)).map(|s| steps[s]).collect();
        if on.is_empty() || off.is_empty() {
            continue;
        }
        dips.push(
            on.iter().sum::<f64>() / on.len() as f64 - off.iter().sum::<f64>() / off.len() as f64,
        );
    }
    (!dips.is_empty()).then(|| round(dips.iter().sum::<f64>() / dips.len() as f64))
}

// what one stem's measures suggest about its effects, in plain words with an fl studio way to get there
fn part_clues(
    name: &str,
    echoes: Vec<Echo>,
    space: Option<f64>,
    pump: Option<f64>,
    width: Option<f64>,
) -> PartClues {
    let mut clues = Vec::new();
    if let Some(best) = echoes
        .first()
        .filter(|e| e.quieter >= ECHO_SHARE && round(e.quieter - e.louder) >= ECHO_EXCESS)
    {
        clues.push(format!(
            "{:.0}% of its notes come back {} later and quieter: likely a delay set to {} (Fruity Delay 3 or Fruity Delay Bank); a riff that really repeats can look the same, so check by ear",
            best.quieter * 100.0,
            best.lag,
            best.lag
        ));
    }
    if let Some(space) = space {
        if space < WET_DB {
            clues.push(format!(
                "it rarely goes quiet between notes ({:.0} dB between its loud and quiet moments): a long reverb, long releases or a held layer (Fruity Reeverb 2 or LuxeVerb)",
                space
            ));
        } else if space > DRY_DB {
            clues.push(format!(
                "dry: the sound stops between notes ({:.0} dB between its loud and quiet moments), little or no reverb",
                space
            ));
        }
    }
    if let Some(pump) = pump.filter(|&p| p <= -PUMP_DB) {
        clues.push(format!(
            "it dips {:.0} dB on each kick and comes back: sidechain compression or a volume shape (Fruity Limiter sidechained to the kick, or Gross Beat)",
            -pump
        ));
    }
    if let Some(width) = width.filter(|&w| w >= WIDE) {
        clues.push(format!(
            "wide in stereo ({:.2}): a chorus, stereo widener or stereo reverb (Fruity Chorus, Hyper Chorus)",
            width
        ));
    }
    PartClues {
        part: name.to_string(),
        echoes,
        space_db: space.map(round),
        pump_db: pump,
        width: width.map(round),
        clues,
    }
}

// a number rounded to two decimals
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // a line whose every note returns a dotted 8th later and quieter reads as a dotted 8th delay
    #[test]
    fn finds_a_dotted_eighth_echo() {
        let mut notes = Vec::new();
        for i in 0..24u32 {
            let start = i * 8 * STEP;
            let key = 60 + (i % 5) as u8 * 2;
            notes.push(Note {
                part: Part::Melody,
                key,
                velocity: 100,
                start,
                length: STEP,
            });
            notes.push(Note {
                part: Part::Melody,
                key,
                velocity: 60,
                start: start + 3 * STEP,
                length: STEP,
            });
        }
        let refs: Vec<&Note> = notes.iter().collect();
        let found = echoes(&refs, false);
        assert_eq!(found[0].lag, "dotted 1/8 (3/16)");
        let clues = part_clues("other", found, None, None, None);
        assert!(clues.clues[0].contains("dotted 1/8"));
    }

    // a part that dips on the kicks reads as pumping, and wide, wet parts get their own clues
    #[test]
    fn describes_space_pump_and_width() {
        let clues = part_clues("other", Vec::new(), Some(8.0), Some(-5.0), Some(0.4));
        assert_eq!(clues.clues.len(), 3);
        assert!(clues.clues[0].contains("reverb"));
        assert!(clues.clues[1].contains("dips 5 dB"));
        assert!(clues.clues[2].contains("wide"));
    }
}
