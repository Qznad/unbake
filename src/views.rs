use crate::music::{BAR, Note, Part, STEP};
use crate::rebuild::{self, Session};
use crate::tools;
use serde::Serialize;
use std::path::Path;

const NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
const PRESENT: f64 = 0.08;
const THIRD: f64 = 0.04;

#[derive(Serialize)]
pub struct BarLine {
    pub bar: u32,
    pub section: Option<String>,
    pub kick: usize,
    pub snare: usize,
    pub hat: usize,
    pub bass: usize,
    pub other: usize,
    pub chord: Option<String>,
}

#[derive(Serialize)]
pub struct GridRow {
    pub part: String,
    pub hits: Option<String>,
    pub notes: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct GridBar {
    pub bar: u32,
    pub rows: Vec<GridRow>,
}

#[derive(Serialize)]
pub struct Tone {
    pub note: String,
    pub percent: u32,
}

#[derive(Serialize)]
pub struct HarmonySpan {
    pub from_bar: u32,
    pub bars: u32,
    pub bass: Option<String>,
    pub chord: Option<String>,
    pub tones: Vec<Tone>,
}

// one line per bar with its section, how many drum, bass and other notes it holds and the chord heard
pub fn overview(folder: &Path) -> Result<Vec<BarLine>, String> {
    let session = rebuild::load_session(folder)?;
    let count = |notes: &[Note], bar: u32, part: Option<Part>| {
        notes
            .iter()
            .filter(|n| n.start / BAR == bar && part.is_none_or(|p| n.part == p))
            .count()
    };
    let spans = bass_spans(&session, 0, session.bars);
    Ok((0..session.bars)
        .map(|bar| BarLine {
            bar: bar + 1,
            section: session
                .sections
                .iter()
                .find(|s| s.start_bar == bar)
                .map(|s| s.name.clone()),
            kick: count(&session.drums, bar, Some(Part::Kick)),
            snare: count(&session.drums, bar, Some(Part::Snare)),
            hat: count(&session.drums, bar, Some(Part::Hat)),
            bass: count(&session.bass, bar, None),
            other: count(&session.other, bar, None),
            chord: spans
                .iter()
                .find(|s| bar + 1 >= s.from_bar && bar + 1 < s.from_bar + s.bars)
                .and_then(|s| s.chord.clone()),
        })
        .collect())
}

// a part as 16 steps per bar, drums as hit strings compose can take and pitched parts as the note starting on each step
pub fn grid(folder: &Path, name: &str, from_bar: u32, bars: u32) -> Result<Vec<GridBar>, String> {
    let session = rebuild::load_session(folder)?;
    let notes = tools::notes_named(&session, name)?;
    let first = from_bar.max(1) - 1;
    Ok((first..first + bars.max(1))
        .map(|bar| {
            let in_bar: Vec<&Note> = notes.iter().filter(|n| n.start / BAR == bar).collect();
            let rows = if name == "drums" {
                [Part::Kick, Part::Snare, Part::Hat]
                    .iter()
                    .map(|&part| {
                        let mut hits = ['.'; 16];
                        for note in in_bar.iter().filter(|n| n.part == part) {
                            let step = ((note.start % BAR) / STEP) as usize;
                            hits[step.min(15)] = symbol(note.velocity);
                        }
                        GridRow {
                            part: part.name().to_string(),
                            hits: Some(hits.iter().collect()),
                            notes: None,
                        }
                    })
                    .collect()
            } else {
                let mut cells = vec![".".to_string(); 16];
                let mut loudest = [0u8; 16];
                for note in &in_bar {
                    let step = (((note.start % BAR) / STEP) as usize).min(15);
                    if note.velocity >= loudest[step] {
                        loudest[step] = note.velocity;
                        cells[step] = crate::music::note_name(note.key);
                    }
                }
                vec![GridRow {
                    part: name.to_string(),
                    hits: None,
                    notes: Some(cells),
                }]
            };
            GridBar { bar: bar + 1, rows }
        })
        .collect())
}

// the note names sounding in the other and bass stems for each span of bars, weighted by length and loudness, with a chord name, spans following the bass when per is 0
pub fn harmony(
    folder: &Path,
    from_bar: u32,
    bars: u32,
    per: u32,
) -> Result<Vec<HarmonySpan>, String> {
    let session = rebuild::load_session(folder)?;
    let first = from_bar.max(1) - 1;
    let last = (first + bars.max(1)).min(session.bars.max(first + 1));
    if per == 0 {
        return Ok(bass_spans(&session, first, last));
    }
    Ok((first..last)
        .step_by(per as usize)
        .map(|bar| span(&session, bar, per.min(last - bar)))
        .collect())
}

// one span for each run of bars whose loudest bass note stays the same
fn bass_spans(session: &Session, first: u32, last: u32) -> Vec<HarmonySpan> {
    let roots: Vec<Option<usize>> = (first..last)
        .map(|bar| {
            let low = weights(&session.bass, bar * BAR, (bar + 1) * BAR);
            (0..12)
                .filter(|&pc| low[pc] > 0.0)
                .max_by(|&a, &b| low[a].total_cmp(&low[b]))
        })
        .collect();
    let mut spans = Vec::new();
    let mut start = 0;
    for i in 1..=roots.len() {
        if i == roots.len() || roots[i] != roots[start] {
            spans.push(span(session, first + start as u32, (i - start) as u32));
            start = i;
        }
    }
    spans
}

// the harmony of some bars starting at a 0 based bar
fn span(session: &Session, bar: u32, bars: u32) -> HarmonySpan {
    let start = bar * BAR;
    let end = start + bars * BAR;
    let other = weights(&session.other, start, end);
    let low = weights(&session.bass, start, end);
    let total: f64 = other.iter().sum();
    let mut order: Vec<usize> = (0..12).filter(|&pc| other[pc] > 0.0).collect();
    order.sort_by(|&a, &b| other[b].total_cmp(&other[a]));
    let tones = order
        .iter()
        .take(8)
        .map(|&pc| Tone {
            note: NAMES[pc].to_string(),
            percent: (100.0 * other[pc] / total.max(1e-9)).round() as u32,
        })
        .collect();
    let root = (0..12)
        .filter(|&pc| low[pc] > 0.0)
        .max_by(|&a, &b| low[a].total_cmp(&low[b]));
    let chord = match root {
        Some(root) if total > 0.0 => {
            let shares: Vec<f64> = other.iter().map(|w| w / total).collect();
            name_over_bass(root, &shares)
        }
        _ => None,
    };
    HarmonySpan {
        from_bar: bar + 1,
        bars,
        bass: root.map(|pc| NAMES[pc].to_string()),
        chord,
        tones,
    }
}

// how long and loud each note name sounds between two ticks
fn weights(notes: &[Note], start: u32, end: u32) -> [f64; 12] {
    let mut out = [0.0; 12];
    for note in notes {
        let from = note.start.max(start);
        let to = (note.start + note.length).min(end);
        if to > from {
            out[note.key as usize % 12] += (to - from) as f64 * note.velocity as f64;
        }
    }
    out
}

// the chord over a bass note, named from the bass when the notes above fit it, otherwise from the notes as a slash chord, or none
pub fn name_over_bass(bass: usize, shares: &[f64]) -> Option<String> {
    let at = |interval: usize| shares[(bass + interval) % 12];
    let third = at(3).max(at(4)) >= PRESENT;
    let colour = [2, 10, 11].iter().any(|&i| at(i) >= PRESENT);
    if at(7) >= PRESENT || (third && colour) {
        return Some(chord_name(bass, shares));
    }
    let root = (0..12)
        .filter(|&pc| shares[pc] >= PRESENT)
        .max_by(|&a, &b| {
            fits(a, shares)
                .cmp(&fits(b, shares))
                .then(shares[a].total_cmp(&shares[b]))
        })?;
    (fits(root, shares) >= 2).then(|| format!("{}/{}", chord_name(root, shares), NAMES[bass]))
}

// how many of a root's own note, third and fifth are heard, the bass counting as its own note
fn fits(root: usize, shares: &[f64]) -> usize {
    let at = |interval: usize| shares[(root + interval) % 12];
    1 + usize::from(at(3).max(at(4)) >= PRESENT) + usize::from(at(7) >= PRESENT)
}

// a chord name like Am9, Gm11 or Bbmaj9 from the bass note and the share of each note name heard above it
pub fn chord_name(root: usize, shares: &[f64]) -> String {
    let has = |interval: usize| shares[(root + interval) % 12] >= PRESENT;
    let weight = |interval: usize| shares[(root + interval) % 12];
    let third = |interval: usize| weight(interval) >= THIRD;
    let minor = third(3) && (!third(4) || weight(3) >= weight(4));
    let major = third(4) && !minor;
    let flat_seven = has(10) && (!has(11) || weight(10) >= weight(11));
    let major_seven = has(11) && !flat_seven;
    let extension = if has(9) && (flat_seven || major_seven) {
        "13"
    } else if has(5) && (minor || major) && (flat_seven || major_seven) {
        "11"
    } else if has(2) && (flat_seven || major_seven) {
        "9"
    } else {
        "7"
    };
    let name = NAMES[root];
    match (minor, major, flat_seven, major_seven) {
        (true, _, true, _) => format!("{}m{}", name, extension),
        (true, _, _, true) => format!("{}m(maj7)", name),
        (_, true, true, _) => format!("{}{}", name, extension),
        (_, true, _, true) => format!("{}maj{}", name, extension),
        (true, _, false, false) if has(2) => format!("{}madd9", name),
        (true, _, false, false) => format!("{}m", name),
        (_, true, false, false) if has(2) => format!("{}add9", name),
        (_, true, false, false) => name.to_string(),
        _ if has(5) => format!("{}{}sus4", name, if flat_seven { "7" } else { "" }),
        _ if has(2) => format!("{}sus2", name),
        _ if has(7) => format!("{}5", name),
        _ => name.to_string(),
    }
}

// the grid letter for a hit's loudness, the same letters compose reads
fn symbol(velocity: u8) -> char {
    match velocity {
        95.. => 'X',
        75.. => 'x',
        55.. => 'o',
        _ => 'g',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // the shares of each note name from a list of names and percents
    fn shares(tones: &[(usize, f64)]) -> Vec<f64> {
        let mut out = vec![0.0; 12];
        for &(pc, percent) in tones {
            out[pc] = percent / 100.0;
        }
        out
    }

    // the chords the another_medium agent worked out by hand get the same names
    #[test]
    fn names_extended_chords() {
        let g = shares(&[
            (2, 35.0),
            (0, 20.0),
            (5, 20.0),
            (10, 14.0),
            (9, 7.0),
            (4, 3.0),
        ]);
        assert_eq!(chord_name(7, &g), "Gm11");
        let b_flat = shares(&[(5, 35.0), (0, 26.0), (9, 16.0), (10, 15.0), (2, 8.0)]);
        assert_eq!(chord_name(10, &b_flat), "Bbmaj9");
        let a = shares(&[
            (4, 41.0),
            (7, 31.0),
            (0, 8.0),
            (11, 8.0),
            (2, 7.0),
            (9, 4.0),
        ]);
        assert_eq!(chord_name(9, &a), "Am9");
        let c = shares(&[(0, 40.0), (4, 30.0), (7, 30.0)]);
        assert_eq!(chord_name(0, &c), "C");
        let a_seven = shares(&[(4, 47.0), (7, 37.0), (0, 5.0), (2, 4.0), (9, 3.0)]);
        assert_eq!(name_over_bass(9, &a_seven).as_deref(), Some("Am7"));
        let over_b_flat = shares(&[(4, 45.0), (7, 34.0), (0, 8.0), (11, 7.0), (2, 6.0)]);
        assert_eq!(name_over_bass(10, &over_b_flat).as_deref(), Some("C/Bb"));
        let thin = shares(&[(4, 55.0), (2, 45.0)]);
        assert_eq!(name_over_bass(10, &thin), None);
    }
}
