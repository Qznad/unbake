use crate::analysis::chords::ChordSpan;
use crate::analysis::drums::Hit;
use crate::analysis::key::Key;
use crate::analysis::tempo::Guess;
use crate::basic_pitch;
use crate::evidence::{self, Alternative, Finding};
use crate::music::{Note, Part, note_name};
use std::collections::HashMap;

const CLOSE: f64 = 0.7;
const CLEAR_VELOCITY: u8 = 10;

// the tempo with the related tempos that also fit, and whether half or double time is in doubt
pub fn tempo(guesses: &[Guess]) -> Finding {
    let chosen = guesses.first().map_or(120.0, |g| g.bpm);
    let mut finding = Finding::new("tempo", format!("{} bpm", chosen));
    let mut others: Vec<&Guess> = guesses.iter().skip(1).collect();
    others.sort_by(|a, b| b.score.total_cmp(&a.score));
    let runner_up = others.first().map_or(0.0, |g| g.score);
    finding.confidence = Some(evidence::margin(1.0, runner_up));
    for other in &others {
        finding.alternatives.push(Alternative {
            value: format!("{} bpm", other.bpm),
            score: evidence::round(other.score),
        });
        if other.score < CLOSE {
            continue;
        }
        let ratio = other.bpm / chosen;
        let doubled = (ratio - 2.0).abs() < 0.05 || (ratio - 0.5).abs() < 0.05;
        finding.unknowns.push(if doubled {
            format!(
                "half or double time: {} bpm fits {:.0}% as well",
                other.bpm,
                other.score * 100.0
            )
        } else {
            format!(
                "{} bpm fits {:.0}% as well, the beat may be felt in threes",
                other.bpm,
                other.score * 100.0
            )
        });
    }
    finding
        .limitations
        .push("one tempo for the whole song, tempo changes are not followed".to_string());
    finding.limitations.push(
        "notes are snapped to a straight 1/16 grid in 4/4, swing, triplets and other meters are lost"
            .to_string(),
    );
    finding
        .limitations
        .push("tempos are kept between 60 and 200 bpm".to_string());
    finding
}

// the key with the next closest keys, and whether its relative key could be the home key instead
pub fn key(ranked: &[Key]) -> Finding {
    let Some(best) = ranked.first() else {
        return Finding::new("key", "unknown".to_string());
    };
    let mut finding = Finding::new("key", best.name());
    let best_score = best.confidence.max(1e-6) as f64;
    let runner_up = ranked.get(1).map_or(0.0, |k| k.confidence as f64);
    finding.confidence = Some(evidence::round((best_score - runner_up) * 5.0));
    for other in ranked.iter().skip(1).take(3) {
        finding.alternatives.push(Alternative {
            value: other.name(),
            score: evidence::round(other.confidence as f64 / best_score),
        });
    }
    let relative = best.relative();
    for other in ranked.iter().skip(1).take(3) {
        if (other.confidence as f64) < 0.9 * best_score {
            continue;
        }
        let same_notes = other.tonic == relative.tonic && other.minor == relative.minor;
        finding.unknowns.push(if same_notes {
            format!(
                "{} uses the same notes, so the home note could be either",
                other.name()
            )
        } else {
            format!(
                "{} fits {:.0}% as well",
                other.name(),
                100.0 * other.confidence as f64 / best_score
            )
        });
    }
    finding
        .limitations
        .push("major and minor only, modes and key changes are not found".to_string());
    finding
}

// the melody line, how clearly it stands out from the notes around it, and its range
pub fn melody(melody: &[Note], heard: &[Note], from_basic_pitch: bool) -> Finding {
    let mut finding = Finding::new("melody", describe(melody));
    if from_basic_pitch && !melody.is_empty() {
        let block = basic_pitch::block_chords(heard);
        let mut by_step: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, note) in heard.iter().enumerate() {
            by_step.entry(note.start).or_default().push(i);
        }
        let clear = melody
            .iter()
            .filter(|note| {
                let rival = by_step
                    .get(&note.start)
                    .into_iter()
                    .flatten()
                    .filter(|&&j| !block[j] && heard[j].key != note.key)
                    .map(|&j| heard[j].velocity)
                    .max();
                rival.is_none_or(|v| note.velocity >= v.saturating_add(CLEAR_VELOCITY))
            })
            .count();
        let share = clear as f64 / melody.len() as f64;
        finding.confidence = Some(evidence::round(share));
        if share < 0.6 {
            finding.unknowns.push(format!(
                "on {:.0}% of its notes another note starts almost as loud, so it may belong to a different instrument",
                (1.0 - share) * 100.0
            ));
        }
        finding.limitations.push(
            "the melody is the loudest note on each step of the other stem, which can hold several instruments"
                .to_string(),
        );
        finding.limitations.push(
            "Basic Pitch misses some notes in dense mixes, so a missing note is unknown, not silence"
                .to_string(),
        );
    } else if !from_basic_pitch {
        finding
            .limitations
            .push("found by the built-in tracker because Basic Pitch could not run".to_string());
    }
    if let Some(span) = span(melody) {
        if span > 24 {
            finding.unknowns.push(format!(
                "it spans {} semitones, more than two octaves, so it may jump between instruments",
                span
            ));
        }
    }
    finding
}

// the bass line, its range, and whether a higher note often took over from the lowest
pub fn bass(line: &[Note], heard: &[Note], from_basic_pitch: bool) -> Finding {
    let mut finding = Finding::new("bass", describe(line));
    if from_basic_pitch && !line.is_empty() {
        let mut by_step: HashMap<u32, Vec<&Note>> = HashMap::new();
        for note in heard {
            by_step.entry(note.start).or_default().push(note);
        }
        let louder_above = line
            .iter()
            .filter(|low| {
                by_step.get(&low.start).is_some_and(|step| {
                    step.iter()
                        .any(|n| n.key > low.key && n.velocity > low.velocity)
                })
            })
            .count();
        let share = louder_above as f64 / line.len() as f64;
        if share > 0.3 {
            finding.unknowns.push(format!(
                "on {:.0}% of steps a higher note is louder than the lowest one, so the bass may be doubled or an octave off",
                share * 100.0
            ));
        }
        finding.limitations.push(
            "one note at a time, the lowest on each step, so two bass parts at once become one"
                .to_string(),
        );
    } else if !from_basic_pitch {
        finding
            .limitations
            .push("found by the built-in tracker because Basic Pitch could not run".to_string());
    }
    if let Some(span) = span(line) {
        if span > 24 {
            finding.unknowns.push(format!(
                "it spans {} semitones, more than two octaves, so some notes may be octave slips or other instruments",
                span
            ));
        }
    }
    finding
}

// the drum hits found per drum
pub fn drums(hits: &[Hit]) -> Finding {
    let count = |part: Part| hits.iter().filter(|h| h.part == part).count();
    let mut finding = Finding::new(
        "drums",
        format!(
            "kick {}, snare {}, hat {}",
            count(Part::Kick),
            count(Part::Snare),
            count(Part::Hat)
        ),
    );
    if hits.is_empty() {
        finding
            .unknowns
            .push("no drum hits found, the drums may be a sampled loop or absent".to_string());
    }
    finding
        .limitations
        .push("kick, snare and hat only, toms, cymbals and percussion are not kept".to_string());
    finding
        .limitations
        .push("a hat that lands on a snare is often missed".to_string());
    finding
}

// the chord progression names, and what the chords part holds
pub fn chords(spans: &[ChordSpan], simple: bool) -> Finding {
    let mut names: Vec<String> = Vec::new();
    for span in spans {
        let name = span.chord.name();
        if names.last() != Some(&name) {
            names.push(name);
        }
    }
    let shown: Vec<String> = names.iter().take(8).cloned().collect();
    let more = if names.len() > 8 { " …" } else { "" };
    let mut finding = Finding::new("chords", format!("{}{}", shown.join(" "), more));
    finding.limitations.push(
        "chord names are major and minor triads of the key and the usual borrowed chords"
            .to_string(),
    );
    finding.limitations.push(if simple {
        "the chords part plays those triads, not the notes heard".to_string()
    } else {
        "the chords part holds every other-stem note that isn't the melody, not only chord tones"
            .to_string()
    });
    finding
}

// how many notes a line has and its lowest and highest note
fn describe(notes: &[Note]) -> String {
    match (
        notes.iter().map(|n| n.key).min(),
        notes.iter().map(|n| n.key).max(),
    ) {
        (Some(low), Some(high)) => format!(
            "{} notes from {} to {}",
            notes.len(),
            note_name(low),
            note_name(high)
        ),
        _ => "no notes".to_string(),
    }
}

// the distance in semitones between the lowest and highest note
fn span(notes: &[Note]) -> Option<u8> {
    let low = notes.iter().map(|n| n.key).min()?;
    let high = notes.iter().map(|n| n.key).max()?;
    Some(high - low)
}

#[cfg(test)]
mod tests {
    use super::*;

    // a note of a part at a step with a velocity
    fn note(part: Part, key: u8, velocity: u8, start: u32) -> Note {
        Note {
            part,
            key,
            velocity,
            start,
            length: 24,
        }
    }

    // a double time that fits nearly as well is flagged as unknown
    #[test]
    fn tempo_doubt() {
        let guesses = [
            Guess {
                bpm: 120.0,
                score: 1.0,
            },
            Guess {
                bpm: 240.0,
                score: 0.9,
            },
            Guess {
                bpm: 60.0,
                score: 0.4,
            },
        ];
        let finding = tempo(&guesses);
        assert_eq!(finding.value, "120 bpm");
        assert_eq!(finding.confidence, Some(0.1));
        assert_eq!(finding.alternatives[0].value, "240 bpm");
        assert_eq!(finding.unknowns.len(), 1);
        assert!(finding.unknowns[0].starts_with("half or double time"));
    }

    // a melody note with a rival almost as loud on its step is not clear
    #[test]
    fn melody_clarity() {
        let heard = vec![
            note(Part::Melody, 72, 100, 0),
            note(Part::Melody, 60, 50, 0),
            note(Part::Melody, 74, 100, 24),
            note(Part::Melody, 62, 95, 24),
        ];
        let lead = vec![heard[0], heard[2]];
        let finding = melody(&lead, &heard, true);
        assert_eq!(finding.confidence, Some(0.5));
        assert_eq!(finding.value, "2 notes from C5 to D5");
    }

    // a bass whose octave above is louder on most steps is flagged
    #[test]
    fn bass_octave_doubt() {
        let heard = vec![
            note(Part::Bass, 36, 50, 0),
            note(Part::Bass, 48, 90, 0),
            note(Part::Bass, 38, 50, 24),
            note(Part::Bass, 50, 90, 24),
        ];
        let line = vec![heard[0], heard[2]];
        let finding = bass(&line, &heard, true);
        assert_eq!(finding.confidence, None);
        assert_eq!(finding.unknowns.len(), 1);
    }
}
