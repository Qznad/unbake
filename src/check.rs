use crate::flp::events::Project;
use crate::flp::info;
use crate::flp::timeline::{self, ClipSource};
use crate::music::{BAR, Note, PPQ, Part, STEP};
use crate::rebuild::{self, Session};
use serde::Serialize;
use std::path::Path;

const STRETCH_BARS: u32 = 4;
const FITS: f64 = 0.5;
const WEAK: f64 = 0.25;
const DRUMS_FIT: f64 = 0.6;
const DRUMS_WEAK: f64 = 0.35;
const ENOUGH: usize = 4;
const LONG: u32 = 4 * STEP;
const UNCHECKED: [&str; 11] = [
    "crash", "ride", "cymbal", "break", "perc", "tom", "drum", "fx", "riser", "impact", "noise",
];

#[derive(Serialize)]
pub struct ChannelFit {
    pub channel: String,
    pub against: String,
    pub fitting: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<StretchFit>,
}

#[derive(Serialize)]
pub struct StretchFit {
    pub song_bar: i64,
    pub project_bar: u32,
    pub fit: f64,
    pub verdict: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Truth {
    Other,
    Bass,
    Drum(Part),
}

struct Played {
    channel: u16,
    start: u32,
    length: u32,
    key: u8,
}

// compares every channel of a project with what the song plays in the same bars, stretch by stretch, and lists where a part doesn't fit
pub fn check(project: &Project, folder: &Path, song_bar: f64) -> Result<Vec<ChannelFit>, String> {
    let session = rebuild::load_session(folder)?;
    let measures = crate::listen::measure(folder, &session);
    let shift = ((song_bar - 1.0) * BAR as f64).round() as i64;
    let notes = played(project);
    let end = notes.iter().map(|n| n.start + n.length).max().unwrap_or(0);
    let stretch = STRETCH_BARS * BAR;
    let mut out = Vec::new();
    for channel in info::summarize(project).channels {
        let Some(truth) = truth_for(&channel.name) else {
            continue;
        };
        let mine: Vec<&Played> = notes.iter().filter(|n| n.channel == channel.id).collect();
        if mine.is_empty() {
            continue;
        }
        let mut fit = ChannelFit {
            channel: channel.name.clone(),
            against: match truth {
                Truth::Other => "the notes heard in the other stem".to_string(),
                Truth::Bass => "the notes heard in the bass stem".to_string(),
                Truth::Drum(part) => format!("the {} hits heard in the drums stem", part.name()),
            },
            fitting: 0,
            problems: Vec::new(),
        };
        let mut from = 0;
        while from < end {
            let inside: Vec<&Played> = mine
                .iter()
                .filter(|n| n.start >= from && n.start < from + stretch)
                .copied()
                .collect();
            if inside.len() >= ENOUGH {
                let window = (from as i64 + shift, (from + stretch) as i64 + shift);
                let (score, mut verdict) = judge(&session, truth, &inside, shift, window);
                let first_bar = window.0.div_euclid(BAR as i64) + 1;
                if !verdict.is_empty() && first_bar >= 1 && first_bar <= session.bars as i64 {
                    let bars = STRETCH_BARS.min(session.bars + 1 - first_bar as u32);
                    let (sound, unclear) =
                        crate::song_map::sound_of(&measures, first_bar as u32, bars);
                    if unclear {
                        verdict = format!(
                            "unclear here, the song is {}, so unbake can't check this part; ask the user",
                            sound
                        );
                    }
                }
                if verdict.is_empty() {
                    fit.fitting += 1;
                } else {
                    fit.problems.push(StretchFit {
                        song_bar: first_bar,
                        project_bar: from / BAR + 1,
                        fit: (score * 100.0).round() / 100.0,
                        verdict,
                    });
                }
            }
            from += stretch;
        }
        out.push(fit);
    }
    Ok(out)
}

// how well some notes of a channel fit the song between two song ticks, and what is wrong when they don't, empty when they fit
fn judge(
    session: &Session,
    truth: Truth,
    mine: &[&Played],
    shift: i64,
    window: (i64, i64),
) -> (f64, String) {
    let near = |n: &Note| {
        let start = n.start as i64;
        start + n.length as i64 > window.0 - STEP as i64 && start < window.1 + STEP as i64
    };
    match truth {
        Truth::Drum(part) => {
            let heard: Vec<i64> = session
                .drums
                .iter()
                .filter(|n| n.part == part && near(n))
                .map(|n| n.start as i64)
                .collect();
            if heard.len() < ENOUGH {
                return (
                    0.0,
                    format!(
                        "the song has almost no {} hits heard here, so this can't be checked; ask the user",
                        part.name()
                    ),
                );
            }
            let starts: Vec<i64> = mine.iter().map(|n| n.start as i64 + shift).collect();
            let score = hit_fit(&starts, &heard);
            let verdict = if score >= DRUMS_FIT {
                String::new()
            } else if score >= DRUMS_WEAK {
                "weak: the hits only partly match the song's here, check them by ear".to_string()
            } else {
                "doesn't fit: the song's hits fall elsewhere here, this groove is probably from other bars"
                    .to_string()
            };
            (score, verdict)
        }
        Truth::Other | Truth::Bass => {
            let source = if truth == Truth::Bass {
                &session.bass
            } else {
                &session.other
            };
            let heard: Vec<&Note> = source.iter().filter(|n| near(n)).collect();
            if heard.len() < ENOUGH {
                return (
                    0.0,
                    "the song plays almost nothing here that unbake can hear; check with the user that this part belongs here".to_string(),
                );
            }
            let notes: Vec<(i64, u32, u8)> = mine
                .iter()
                .map(|n| (n.start as i64 + shift, n.length, n.key))
                .collect();
            let score = pitch_fit(&notes, &heard);
            let verdict = if score >= FITS {
                String::new()
            } else if score >= WEAK {
                "weak: only some of these notes are heard in the song here, check them by ear"
                    .to_string()
            } else {
                "doesn't fit: the song plays other notes here, this part is probably copied from other bars or wrong"
                    .to_string()
            };
            (score, verdict)
        }
    }
}

// how well drum hits line up with the song's, as the balance of the share of hits the song also has and the share of the song's hits they cover
fn hit_fit(starts: &[i64], heard: &[i64]) -> f64 {
    if starts.is_empty() || heard.is_empty() {
        return 0.0;
    }
    let close = |a: i64, b: &[i64]| b.iter().any(|&x| (x - a).abs() <= STEP as i64);
    let precision =
        starts.iter().filter(|&&s| close(s, heard)).count() as f64 / starts.len() as f64;
    let recall = heard.iter().filter(|&&h| close(h, starts)).count() as f64 / heard.len() as f64;
    if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    }
}

// how far above chance a line's notes are heard in the song, 1 when all are and 0 when no more than notes moved to other note names would be
fn pitch_fit(notes: &[(i64, u32, u8)], heard: &[&Note]) -> f64 {
    if notes.is_empty() {
        return 0.0;
    }
    let found = |transpose: u8| {
        let hits = notes
            .iter()
            .filter(|&&(start, length, key)| {
                let pc = (key + transpose) % 12;
                heard.iter().any(|h| {
                    let at = h.start as i64;
                    h.key % 12 == pc
                        && ((at - start).abs() <= STEP as i64
                            || (length >= LONG
                                && at < start + length as i64
                                && at + h.length as i64 > start))
                })
            })
            .count();
        hits as f64 / notes.len() as f64
    };
    let real = found(0);
    let chance = (1..12).map(found).sum::<f64>() / 11.0;
    if chance >= 0.99 {
        0.0
    } else {
        ((real - chance) / (1.0 - chance)).clamp(-1.0, 1.0)
    }
}

// which of the song's heard notes a channel should be compared with, judged from its name, or none for sounds unbake can't check
fn truth_for(name: &str) -> Option<Truth> {
    let low = name.to_lowercase();
    if low.contains("stem") {
        return None;
    }
    if low.contains("kick") {
        return Some(Truth::Drum(Part::Kick));
    }
    if low.contains("snare") || low.contains("clap") {
        return Some(Truth::Drum(Part::Snare));
    }
    if low.contains("hat") || low.contains("hh") {
        return Some(Truth::Drum(Part::Hat));
    }
    if UNCHECKED.iter().any(|w| low.contains(w)) {
        return None;
    }
    if ["bass", "sub", "808"].iter().any(|w| low.contains(w)) {
        return Some(Truth::Bass);
    }
    Some(Truth::Other)
}

// every note the project plays on tracks that are switched on, with times in unbake's ticks
fn played(project: &Project) -> Vec<Played> {
    let scale = |tick: u32| (tick as u64 * PPQ as u64 / project.ppq.max(1) as u64) as u32;
    let patterns = timeline::pattern_notes(project);
    let tracks = timeline::tracks(project);
    let mut out = Vec::new();
    for clip in timeline::clips(project) {
        let ClipSource::Pattern(id) = clip.source else {
            continue;
        };
        let muted = tracks
            .iter()
            .any(|t| t.number == clip.track as u32 + 1 && !t.enabled);
        if muted {
            continue;
        }
        let from = clip.cut.unwrap_or(0);
        for note in patterns.get(&id).into_iter().flatten() {
            if note.start >= from && note.start < from + clip.length {
                out.push(Played {
                    channel: note.channel,
                    start: scale(clip.start + note.start - from),
                    length: scale(note.length),
                    key: note.key,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // a line playing the heard notes fits, the same rhythm on other notes doesn't
    #[test]
    fn lines_fit_only_the_notes_heard() {
        let tune = [60u8, 63, 67, 70, 72, 70, 67, 63];
        let heard: Vec<Note> = tune
            .iter()
            .enumerate()
            .map(|(i, &key)| Note {
                part: Part::Melody,
                key,
                velocity: 90,
                start: i as u32 * 2 * STEP,
                length: 2 * STEP,
            })
            .chain((0..4).map(|i| Note {
                part: Part::Chords,
                key: 48,
                velocity: 70,
                start: i * BAR,
                length: BAR,
            }))
            .collect();
        let heard: Vec<&Note> = heard.iter().collect();
        let line = |keys: &[u8]| -> Vec<(i64, u32, u8)> {
            keys.iter()
                .enumerate()
                .map(|(i, &k)| ((i as u32 * 2 * STEP) as i64, 2 * STEP, k))
                .collect()
        };
        assert!(pitch_fit(&line(&tune), &heard) > 0.8);
        let other = [62u8, 65, 69, 62, 65, 69, 62, 65];
        assert!(pitch_fit(&line(&other), &heard) < 0.25);
    }

    // hits on the song's own steps fit, the same count on other steps don't
    #[test]
    fn drums_fit_only_where_the_song_hits() {
        let song: Vec<i64> = (0..8).map(|i| i * 4 * STEP as i64).collect();
        assert!(hit_fit(&song, &song) > 0.99);
        let off: Vec<i64> = (0..8)
            .map(|i| i * 4 * STEP as i64 + 2 * STEP as i64)
            .collect();
        assert!(hit_fit(&off, &song) < 0.01);
    }

    // channel names pick what they are compared with, and unchecked sounds are left out
    #[test]
    fn channels_by_name() {
        assert!(truth_for("Kick") == Some(Truth::Drum(Part::Kick)));
        assert!(truth_for("Snare fill") == Some(Truth::Drum(Part::Snare)));
        assert!(truth_for("Bass Layer") == Some(Truth::Bass));
        assert!(truth_for("Sub") == Some(Truth::Bass));
        assert!(truth_for("Lead") == Some(Truth::Other));
        assert!(truth_for("Crash").is_none());
        assert!(truth_for("Break").is_none());
        assert!(truth_for("Other stem").is_none());
    }
}
