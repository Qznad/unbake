use crate::listen::{self, Measures};
use crate::rebuild::{self, Session};
use crate::views::{self, Parts};
use serde::Serialize;
use std::path::Path;

const SAME: f64 = 0.5;
const CLOSE: f64 = 0.3;
const FEW_NOTES: usize = 3;
const SILENT_DB: f64 = -30.0;
const CHORDS_SAME: f64 = 0.75;
const CHORDS_CLOSE: f64 = 0.5;
const FILTERED_KHZ: f64 = 2.0;
const QUIET_DB: f64 = -12.0;

#[derive(Serialize)]
pub struct Row {
    pub from_bar: u32,
    pub bars: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub drums: String,
    pub bass: String,
    pub chords: String,
    pub lead: String,
    pub sound: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sounds_like: Option<String>,
    pub work: String,
}

#[derive(Serialize)]
pub struct Timeline {
    pub bars_per_row: u32,
    pub rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Silent,
    Same,
    Close,
    New,
    Once,
}

struct Cell {
    text: String,
    kind: Kind,
}

// the song stretch by stretch: whether each part repeats an earlier stretch, is new or is silent, how clearly unbake hears it, and what work that leaves
pub fn timeline(
    folder: &Path,
    from_bar: u32,
    bars: u32,
    approved: &[(u32, u32)],
) -> Result<Timeline, String> {
    let session = rebuild::load_session(folder)?;
    let measures = listen::measure(folder, &session);
    let length = bars.max(1);
    let start = (from_bar.max(1) - 1) % length + 1;
    let chords: Vec<Option<String>> = views::harmony(folder, 1, session.bars, 1)?
        .into_iter()
        .map(|span| span.chord)
        .collect();

    let mut stretches: Vec<(u32, u32)> = Vec::new();
    if start > 1 {
        stretches.push((1, start - 1));
    }
    let mut bar = start;
    while bar <= session.bars {
        stretches.push((bar, length.min(session.bars + 1 - bar)));
        bar += length;
    }

    let parts = |name: &str| -> Result<(Option<Parts>, Parts), String> {
        let head = (start > 1)
            .then(|| views::parts(folder, name, 1, start - 1, Some(start - 1)))
            .transpose()?;
        Ok((head, views::parts(folder, name, start, length, None)?))
    };
    let drums = parts("drums")?;
    let bass = parts("bass")?;
    let lead = parts("melody")?;

    let mut rows = Vec::new();
    for (i, &(from, count)) in stretches.iter().enumerate() {
        let heard = |levels: &[Option<f64>]| {
            levels
                .get(from as usize - 1..(from + count - 1) as usize)
                .unwrap_or(&[])
                .iter()
                .flatten()
                .any(|&db| db > SILENT_DB)
        };
        let cell = |(head, body): &(Option<Parts>, Parts), audible: bool| {
            if !audible {
                return silent();
            }
            match head {
                Some(head) if i == 0 => part_cell(head, from),
                _ => part_cell(body, from),
            }
        };
        let cells = [
            ("drums", cell(&drums, heard(&measures.drums_db))),
            ("bass", cell(&bass, heard(&measures.bass_db))),
            ("chords", chord_cell(&chords, &stretches, i)),
            ("lead", cell(&lead, heard(&measures.other_db))),
        ];
        let (sound, unsure) = sound_of(&measures, from, count);
        let sounds_like = unsure
            .then(|| listen::likeness(&measures, &session, from, count).ok())
            .flatten()
            .map(|like| {
                let best = |found: &[listen::Match], what: &str| {
                    found
                        .first()
                        .map(|m| format!("{} like bar {} ({:.2})", what, m.bar, m.likeness))
                };
                [best(&like.chords, "chords"), best(&like.drums, "drums")]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|like| !like.is_empty());
        let done = approved
            .iter()
            .any(|&(a, b)| from >= a && from + count - 1 <= b);
        let work = if done {
            "done (approved)".to_string()
        } else {
            work_for(&cells, unsure)
        };
        let [d, b, c, l] = cells;
        rows.push(Row {
            from_bar: from,
            bars: count,
            section: section_in(&session, from, count),
            drums: d.1.text,
            bass: b.1.text,
            chords: c.1.text,
            lead: l.1.text,
            sound,
            sounds_like,
            work,
        });
    }
    Ok(Timeline {
        bars_per_row: length,
        rows,
    })
}

// a part's state in the stretch starting at a bar, from its song_parts letters
fn part_cell(parts: &Parts, from: u32) -> Cell {
    let Some(line) = parts.parts.iter().find(|p| p.from_bar == from) else {
        return silent();
    };
    if line.notes < FEW_NOTES {
        return silent();
    }
    let Some(group) = parts.groups.iter().find(|g| g.label == line.label) else {
        return silent();
    };
    if let Some((bar, like)) = line.like_earlier.filter(|&(_, like)| like >= CLOSE) {
        let (word, kind) = if like >= SAME {
            ("same as", Kind::Same)
        } else {
            ("close to", Kind::Close)
        };
        return Cell {
            text: format!("{} bar {} (letter {}, {:.2})", word, bar, group.label, like),
            kind,
        };
    }
    match (&group.variation_of, group.bars.len()) {
        (_, n) if n > 1 => Cell {
            text: format!("new, letter {}, heard {} times", group.label, n),
            kind: Kind::New,
        },
        (Some(of), _) => Cell {
            text: format!("new, letter {}, a variation of letter {}", group.label, of),
            kind: Kind::New,
        },
        (None, _) => Cell {
            text: format!("new, letter {}, heard once", group.label),
            kind: Kind::Once,
        },
    }
}

// a part that plays nothing in the stretch
fn silent() -> Cell {
    Cell {
        text: "silent".to_string(),
        kind: Kind::Silent,
    }
}

// the stretch's chord names bar by bar, compared by root and minor or major with every earlier stretch
fn chord_cell(chords: &[Option<String>], stretches: &[(u32, u32)], i: usize) -> Cell {
    let names = |(from, count): (u32, u32)| -> Vec<Option<String>> {
        (from..from + count)
            .map(|bar| chords.get(bar as usize - 1).cloned().flatten())
            .collect()
    };
    let mine = names(stretches[i]);
    if mine.iter().all(|n| n.is_none()) {
        return Cell {
            text: "none heard".to_string(),
            kind: Kind::Silent,
        };
    }
    let shown: Vec<String> = mine
        .iter()
        .map(|n| n.clone().unwrap_or("-".into()))
        .collect();
    let best = stretches[..i]
        .iter()
        .map(|&s| {
            let theirs = names(s);
            let same = mine
                .iter()
                .zip(&theirs)
                .filter(|(a, b)| a.is_some() && a.as_deref().map(root) == b.as_deref().map(root))
                .count();
            (s.0, same as f64 / mine.len() as f64)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
    match best {
        Some((bar, share)) if share >= CHORDS_SAME => Cell {
            text: format!("same as bar {} ({})", bar, shown.join(" ")),
            kind: Kind::Same,
        },
        Some((bar, share)) if share >= CHORDS_CLOSE => Cell {
            text: format!("close to bar {} ({})", bar, shown.join(" ")),
            kind: Kind::Close,
        },
        _ => Cell {
            text: format!("new ({})", shown.join(" ")),
            kind: Kind::New,
        },
    }
}

// a chord's root with m when it is minor, so Ebmaj13 and Ebmaj7 count as the same chord
fn root(name: &str) -> String {
    let mut chars = name.chars();
    let mut out: String = chars.next().into_iter().collect();
    let rest: String = chars.collect();
    if rest.starts_with('b') || rest.starts_with('#') {
        out.push_str(&rest[..1]);
    }
    let after = &rest[out.len() - 1..];
    if after.starts_with('m') && !after.starts_with("maj") {
        out.push('m');
    }
    out
}

// how the stretch sounds from its levels, and whether it is quiet or filtered enough that its notes can't be trusted
pub fn sound_of(measures: &Measures, from: u32, count: u32) -> (String, bool) {
    let range = (from as usize - 1)..((from + count) as usize - 1).min(measures.drums_db.len());
    let mean = |values: &[Option<f64>]| {
        let heard: Vec<f64> = values
            .get(range.clone())
            .unwrap_or(&[])
            .iter()
            .flatten()
            .copied()
            .collect();
        (!heard.is_empty()).then(|| heard.iter().sum::<f64>() / heard.len() as f64)
    };
    let top = mean(&measures.drums_top);
    let drums = mean(&measures.drums_db);
    let other = mean(&measures.other_db);
    if let Some(top) = top.filter(|&t| t < FILTERED_KHZ) {
        return (
            format!("filtered: the drums reach only {:.1} kHz", top),
            true,
        );
    }
    let quiet = |level: Option<f64>| level.is_none_or(|db| db < QUIET_DB);
    if quiet(other) && quiet(drums) {
        return (
            format!(
                "quiet: drums {}, other {}",
                drums.map_or("silent".into(), |d| format!("{:.0} dB", d)),
                other.map_or("silent".into(), |d| format!("{:.0} dB", d))
            ),
            true,
        );
    }
    ("clear".to_string(), false)
}

// what is left to do in a stretch: nothing when every part repeats one already made, build what is new, and ask the user about what unbake can't hear clearly
fn work_for(cells: &[(&str, Cell); 4], unsure: bool) -> String {
    let mut build = Vec::new();
    let mut ask = Vec::new();
    for (name, cell) in cells {
        match cell.kind {
            Kind::Silent => {}
            _ if unsure => ask.push(format!("{} (unclear here)", name)),
            Kind::Once => ask.push(format!("{} (heard once)", name)),
            Kind::New | Kind::Close => build.push(name.to_string()),
            Kind::Same => {}
        }
    }
    if cells.iter().all(|(_, cell)| cell.kind == Kind::Silent) {
        return "nothing plays".to_string();
    }
    match (ask.is_empty(), build.is_empty()) {
        (true, true) => "reuse".to_string(),
        (true, false) => format!("build: {}", build.join(", ")),
        (false, true) => format!("ask: {}", ask.join(", ")),
        (false, false) => format!("ask: {}; build: {}", ask.join(", "), build.join(", ")),
    }
}

// the names of the sections that start inside a stretch
fn section_in(session: &Session, from: u32, count: u32) -> Option<String> {
    let names: Vec<String> = session
        .sections
        .iter()
        .filter(|s| s.start_bar + 1 >= from && s.start_bar + 1 < from + count)
        .map(|s| s.name.clone())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

// approved stretches given as "33-40" or "33", as first and last bars
pub fn parse_approved(ranges: &[String]) -> Result<Vec<(u32, u32)>, String> {
    ranges
        .iter()
        .map(|r| {
            let (a, b) = r.split_once('-').unwrap_or((r, r));
            let number = |s: &str| {
                s.trim()
                    .parse::<u32>()
                    .map_err(|_| format!("{} is not a bar range like 33-40", r))
            };
            Ok((number(a)?, number(b)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // chord names compare by root and minor or major
    #[test]
    fn roots_of_chord_names() {
        assert_eq!(root("Ebmaj13"), "Eb");
        assert_eq!(root("Cm9"), "Cm");
        assert_eq!(root("F5"), "F");
        assert_eq!(root("Bbm7"), "Bbm");
        assert_eq!(root("C#madd9"), "C#m");
        assert_eq!(root("Em/Bb"), "Em");
    }

    // work is reuse when every part repeats, build for new parts and ask when the stretch is unclear or a part was heard once
    #[test]
    fn work_labels() {
        let cell = |kind: Kind| Cell {
            text: String::new(),
            kind,
        };
        let all = |kinds: [Kind; 4]| {
            [
                ("drums", cell(kinds[0])),
                ("bass", cell(kinds[1])),
                ("chords", cell(kinds[2])),
                ("lead", cell(kinds[3])),
            ]
        };
        use Kind::*;
        assert_eq!(work_for(&all([Same, Same, Same, Silent]), false), "reuse");
        assert_eq!(
            work_for(&all([Silent, Silent, Silent, Silent]), true),
            "nothing plays"
        );
        assert_eq!(
            work_for(&all([Same, New, Same, Once]), false),
            "ask: lead (heard once); build: bass"
        );
        assert_eq!(
            work_for(&all([Same, Silent, Same, New]), true),
            "ask: drums (unclear here), chords (unclear here), lead (unclear here)"
        );
        assert_eq!(
            parse_approved(&["33-40".into(), "9".into()]).unwrap(),
            vec![(33, 40), (9, 9)]
        );
    }
}
