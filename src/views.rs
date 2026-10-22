use crate::music::{BAR, Note, Part, STEP};
use crate::rebuild::{self, Session};
use crate::tools;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

const NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
const PRESENT: f64 = 0.08;
const BASS_PLAYS: f64 = 0.25;
const THIRD: f64 = 0.04;
const SAME_PART: f64 = 0.3;
const VARIATION: f64 = 0.15;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drums_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drums_top_khz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bass_db: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_db: Option<i32>,
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
pub struct PartLine {
    pub from_bar: u32,
    pub bars: u32,
    pub label: String,
    pub notes: usize,
    pub same_as_first: f64,
    pub closest_other: Option<(String, f64)>,
}

#[derive(Serialize)]
pub struct PartGroup {
    pub label: String,
    pub bars: Vec<u32>,
    pub variation_of: Option<String>,
    pub note_count: usize,
    pub notes: Vec<tools::NoteView>,
}

#[derive(Serialize)]
pub struct Parts {
    pub name: String,
    pub bars_per_part: u32,
    pub parts: Vec<PartLine>,
    pub groups: Vec<PartGroup>,
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

// one line per bar with its section, how many drum, bass and other notes it holds, the chord heard, how loud each stem is and how high the drums reach
pub fn overview(folder: &Path) -> Result<Vec<BarLine>, String> {
    let session = rebuild::load_session(folder)?;
    let measures = crate::listen::measure(folder, &session);
    let db = |values: &[Option<f64>], bar: u32| {
        values
            .get(bar as usize)
            .copied()
            .flatten()
            .map(|v| v.round() as i32)
    };
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
            drums_db: db(&measures.drums_db, bar),
            drums_top_khz: measures
                .drums_top
                .get(bar as usize)
                .copied()
                .flatten()
                .map(|v| (v * 10.0).round() / 10.0),
            bass_db: db(&measures.bass_db, bar),
            other_db: db(&measures.other_db, bar),
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

// cuts a part into stretches of some bars from a first bar, letters the stretches that play the same notes, and gives the notes each letter's repeats agree on
pub fn parts(
    folder: &Path,
    name: &str,
    from_bar: u32,
    bars_per_part: u32,
    to_bar: Option<u32>,
) -> Result<Parts, String> {
    let session = rebuild::load_session(folder)?;
    let mut notes = tools::notes_named(&session, name)?;
    match name {
        "bass" => notes = crate::tidy::melody(&on_roots(&notes)),
        "drums" | "chords" | "leftover" => {}
        _ => notes = crate::tidy::melody(&notes),
    }
    let length = bars_per_part.max(1) * BAR;
    let first = from_bar.max(1) - 1;
    let last = to_bar
        .unwrap_or(session.bars)
        .min(session.bars)
        .max(first + 1);
    let mut stretches: Vec<(u32, Vec<Note>)> = Vec::new();
    let mut start = first * BAR;
    while start < last * BAR {
        let end = (start + length).min(last * BAR);
        let inside: Vec<Note> = notes
            .iter()
            .filter(|n| n.start >= start && n.start < end)
            .map(|n| Note {
                start: n.start - start,
                length: n.length.min(end - n.start),
                ..*n
            })
            .collect();
        stretches.push((start / BAR + 1, inside));
        start = end;
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut labels: Vec<Option<usize>> = vec![None; stretches.len()];
    for (i, (_, notes)) in stretches.iter().enumerate() {
        if notes.is_empty() {
            continue;
        }
        let best = groups
            .iter()
            .enumerate()
            .map(|(g, members)| (g, likeness(notes, &stretches[members[0]].1)))
            .filter(|&(_, l)| l >= SAME_PART)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((g, _)) => {
                groups[g].push(i);
                labels[i] = Some(g);
            }
            None => {
                labels[i] = Some(groups.len());
                groups.push(vec![i]);
            }
        }
    }
    let letter = |g: usize| {
        let base = (b'A' + (g % 26) as u8) as char;
        if g < 26 {
            base.to_string()
        } else {
            format!("{}{}", base, g / 26)
        }
    };

    let parts = stretches
        .iter()
        .enumerate()
        .map(|(i, (bar, notes))| {
            let (label, same, closest) = match labels[i] {
                None => ("-".to_string(), 0.0, None),
                Some(g) => {
                    let same = likeness(notes, &stretches[groups[g][0]].1);
                    let closest = groups
                        .iter()
                        .enumerate()
                        .filter(|&(other, _)| other != g)
                        .map(|(other, members)| (other, likeness(notes, &stretches[members[0]].1)))
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(other, l)| (letter(other), round(l)));
                    (letter(g), round(same), closest)
                }
            };
            PartLine {
                from_bar: *bar,
                bars: (length / BAR).min(last + 1 - bar),
                label,
                notes: notes.len(),
                same_as_first: same,
                closest_other: closest,
            }
        })
        .collect();

    let voted: Vec<Vec<Note>> = groups
        .iter()
        .map(|members| {
            agreed(
                members.iter().map(|&m| stretches[m].1.as_slice()).collect(),
                !matches!(name, "drums" | "chords" | "leftover"),
            )
        })
        .collect();
    let groups = groups
        .iter()
        .enumerate()
        .map(|(g, members)| {
            let backbone = (members.len() == 1 && !matches!(name, "bass" | "drums"))
                .then(|| {
                    groups
                        .iter()
                        .enumerate()
                        .filter(|&(other, m)| other != g && m.len() > 1)
                        .map(|(other, m)| {
                            (
                                other,
                                likeness(&stretches[members[0]].1, &stretches[m[0]].1),
                            )
                        })
                        .filter(|&(_, l)| l >= VARIATION)
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(other, _)| other)
                })
                .flatten();
            let notes = match backbone {
                Some(other) => filled(&voted[g], &voted[other]),
                None => voted[g].clone(),
            };
            PartGroup {
                label: letter(g),
                bars: members.iter().map(|&m| stretches[m].0).collect(),
                variation_of: backbone.map(letter),
                note_count: notes.len(),
                notes: notes
                    .iter()
                    .map(|n| tools::NoteView {
                        bar: n.start / BAR + 1,
                        step: n.start % BAR / STEP + 1,
                        note: crate::music::note_name(n.key),
                        key: n.key,
                        steps: n.length.div_ceil(STEP),
                        velocity: n.velocity,
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(Parts {
        name: name.to_string(),
        bars_per_part: bars_per_part.max(1),
        parts,
        groups,
    })
}

impl Parts {
    // the letters with their bars and note counts, and the notes of one letter only when it is asked for
    pub fn only_letter(mut self, letter: Option<&str>) -> Parts {
        for group in &mut self.groups {
            if letter.is_none_or(|l| !l.eq_ignore_ascii_case(&group.label)) {
                group.notes.clear();
            }
        }
        if let Some(letter) = letter {
            self.groups.retain(|g| g.label.eq_ignore_ascii_case(letter));
        }
        self
    }
}

// what a note is when comparing stretches, its step, note name and drum, so an octave slip still matches
fn mark(note: &Note) -> (u32, u8, Part) {
    (note.start / STEP, note.key % 12, note.part)
}

// how much two stretches share, from 0 for nothing to 1 for the same notes
fn likeness(a: &[Note], b: &[Note]) -> f64 {
    let x: std::collections::HashSet<_> = a.iter().map(mark).collect();
    let y: std::collections::HashSet<_> = b.iter().map(mark).collect();
    if x.is_empty() && y.is_empty() {
        return 1.0;
    }
    2.0 * x.intersection(&y).count() as f64 / (x.len() + y.len()) as f64
}

// the notes a third of the stretches agree on (at least two), each with its most common octave, length and loudness, keeping the most voted note per step for a line
fn agreed(stretches: Vec<&[Note]>, one_per_step: bool) -> Vec<Note> {
    let mut seen: HashMap<(u32, u8, Part), Vec<Note>> = HashMap::new();
    for stretch in &stretches {
        let mut here = std::collections::HashSet::new();
        for note in stretch.iter() {
            if here.insert(mark(note)) {
                seen.entry(mark(note)).or_default().push(*note);
            }
        }
    }
    let needed = match stretches.len() {
        1 => 1,
        n => n.div_ceil(3).max(2),
    };
    let mut voted: Vec<Vec<Note>> = seen
        .into_values()
        .filter(|notes| notes.len() >= needed)
        .collect();
    if one_per_step {
        let mut best: HashMap<(u32, Part), Vec<Note>> = HashMap::new();
        for notes in voted {
            let place = (notes[0].start / STEP, notes[0].part);
            if best.get(&place).is_none_or(|b| notes.len() > b.len()) {
                best.insert(place, notes);
            }
        }
        voted = best.into_values().collect();
    }
    let mut out: Vec<Note> = voted
        .into_iter()
        .map(|notes| {
            let mut keys: HashMap<u8, usize> = HashMap::new();
            for n in &notes {
                *keys.entry(n.key).or_default() += 1;
            }
            let key = keys
                .into_iter()
                .max_by_key(|&(k, count)| (count, std::cmp::Reverse(k)))
                .map_or(notes[0].key, |(k, _)| k);
            let mut lengths: Vec<u32> = notes.iter().map(|n| n.length).collect();
            lengths.sort_unstable();
            let mut loudness: Vec<u8> = notes.iter().map(|n| n.velocity).collect();
            loudness.sort_unstable();
            Note {
                key,
                length: lengths[lengths.len() / 2],
                velocity: loudness[loudness.len() / 2],
                ..notes[0]
            }
        })
        .collect();
    out.sort_by_key(|n| (n.start, n.part, n.key));
    out
}

// a bassline with every note that is not its bar's root or fifth moved to the root at the nearest octave
fn on_roots(notes: &[Note]) -> Vec<Note> {
    let mut weight: HashMap<(u32, usize), f64> = HashMap::new();
    for note in notes {
        *weight
            .entry((note.start / BAR, note.key as usize % 12))
            .or_default() += note.length as f64 * note.velocity as f64;
    }
    notes
        .iter()
        .map(|note| {
            let bar = note.start / BAR;
            let root = (0..12)
                .max_by(|&a, &b| {
                    let w = |pc: usize| weight.get(&(bar, pc)).copied().unwrap_or(0.0);
                    w(a).total_cmp(&w(b))
                })
                .unwrap_or(note.key as usize % 12);
            let pc = note.key as usize % 12;
            if pc == root || pc == (root + 7) % 12 {
                return *note;
            }
            let down = (pc + 12 - root) % 12;
            let key = if down <= 6 {
                note.key.saturating_sub(down as u8)
            } else {
                (note.key + (12 - down) as u8).min(127)
            };
            Note { key, ..*note }
        })
        .collect()
}

// a one-off stretch's own notes with its empty steps filled from the part it varies, which was heard more clearly
fn filled(own: &[Note], backbone: &[Note]) -> Vec<Note> {
    let taken: std::collections::HashSet<u32> = own.iter().map(|n| n.start / STEP).collect();
    let mut out: Vec<Note> = own.to_vec();
    out.extend(
        backbone
            .iter()
            .filter(|n| !taken.contains(&(n.start / STEP)))
            .copied(),
    );
    out.sort_by_key(|n| (n.start, n.part, n.key));
    out
}

// a share rounded to two places
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
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

// one span for each run of bars whose loudest bass note stays the same, and one for each bar where no bass plays
fn bass_spans(session: &Session, first: u32, last: u32) -> Vec<HarmonySpan> {
    let roots: Vec<Option<usize>> = (first..last)
        .map(|bar| bass_root(&session.bass, bar * BAR, (bar + 1) * BAR))
        .collect();
    let mut spans = Vec::new();
    let mut start = 0;
    for i in 1..=roots.len() {
        if i == roots.len() || roots[i] != roots[start] || roots[start].is_none() {
            spans.push(span(session, first + start as u32, (i - start) as u32));
            start = i;
        }
    }
    spans
}

// the loudest bass note between two ticks, when the bass plays for at least a quarter of that time, so a stray low note doesn't count
fn bass_root(bass: &[Note], start: u32, end: u32) -> Option<usize> {
    let played: u32 = bass
        .iter()
        .map(|n| {
            (n.start + n.length)
                .min(end)
                .saturating_sub(n.start.max(start))
        })
        .sum();
    if (played as f64) < BASS_PLAYS * end.saturating_sub(start) as f64 {
        return None;
    }
    let low = weights(bass, start, end);
    (0..12)
        .filter(|&pc| low[pc] > 0.0)
        .max_by(|&a, &b| low[a].total_cmp(&low[b]))
}

// the harmony of some bars starting at a 0 based bar
fn span(session: &Session, bar: u32, bars: u32) -> HarmonySpan {
    let start = bar * BAR;
    let end = start + bars * BAR;
    let other = weights(&session.other, start, end);
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
    let root = bass_root(&session.bass, start, end);
    let shares: Vec<f64> = other.iter().map(|w| w / total.max(1e-9)).collect();
    let chord = match root {
        _ if total <= 0.0 => None,
        Some(root) => name_over_bass(root, &shares),
        None => name_alone(&shares),
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
    let root = best_root(shares)?;
    (fits(root, shares) >= 2).then(|| format!("{}/{}", chord_name(root, shares), NAMES[bass]))
}

// the chord the notes make on their own when no bass plays, or none when they don't make one
pub fn name_alone(shares: &[f64]) -> Option<String> {
    let root = best_root(shares)?;
    (fits(root, shares) >= 2).then(|| chord_name(root, shares))
}

// the heard note with the most of its own third and fifth heard too, the louder one winning a tie
fn best_root(shares: &[f64]) -> Option<usize> {
    (0..12)
        .filter(|&pc| shares[pc] >= PRESENT)
        .max_by(|&a, &b| {
            fits(a, shares)
                .cmp(&fits(b, shares))
                .then(shares[a].total_cmp(&shares[b]))
        })
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

    // with no bass the chord is named from the notes alone, and two notes a tone apart make none
    #[test]
    fn names_chords_without_bass() {
        let a_minor = shares(&[(9, 40.0), (0, 30.0), (4, 30.0)]);
        assert_eq!(name_alone(&a_minor).as_deref(), Some("Am"));
        let e_flat = shares(&[(3, 35.0), (7, 30.0), (10, 20.0), (2, 15.0)]);
        assert_eq!(name_alone(&e_flat).as_deref(), Some("Ebmaj7"));
        let cluster = shares(&[(2, 50.0), (4, 50.0)]);
        assert_eq!(name_alone(&cluster), None);
    }

    // a stray short low note is not taken as the bass, a held one is
    #[test]
    fn ignores_stray_bass_notes() {
        let note = |start: u32, length: u32, key: u8| Note {
            start,
            length,
            key,
            velocity: 80,
            part: Part::Bass,
        };
        let stray = [note(10 * STEP, 2 * STEP, 46)];
        assert_eq!(bass_root(&stray, 0, BAR), None);
        let held = [note(0, 15 * STEP, 24)];
        assert_eq!(bass_root(&held, 0, BAR), Some(0));
    }
}
