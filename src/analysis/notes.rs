use crate::analysis::onsets::Onset;
use crate::analysis::pitch::PitchFrame;
use crate::audio::math;
use crate::music::{Grid, Note, Part};

struct Segment {
    key: u8,
    first: usize,
    last: usize,
    level: f32,
    attack: bool,
}

// turns a pitch track into notes that start on the step grid, with a new note at every pitch change or attack
pub fn from_pitch(
    frames: &[PitchFrame],
    frame_rate: f64,
    attacks: &[Onset],
    grid: &Grid,
    part: Part,
    min_clarity: f32,
) -> Vec<Note> {
    let keys = frame_keys(frames, min_clarity);

    let mut attack_frames = vec![false; frames.len()];
    for onset in attacks {
        let f = (onset.time * frame_rate).round() as usize;
        if f < attack_frames.len() {
            attack_frames[f] = true;
        }
    }

    let segments = segments(&keys, frames, &attack_frames);
    let loudest = segments.iter().map(|s| s.level).fold(0.0f32, f32::max);

    let mut notes: Vec<(Note, bool)> = Vec::new();
    for segment in &segments {
        let start_time = segment.first as f64 / frame_rate;
        let end_time = (segment.last + 1) as f64 / frame_rate;
        let Some(start) = grid.step_tick(start_time) else {
            continue;
        };
        let length = ((end_time - start_time) / grid.tick_seconds())
            .round()
            .max(6.0) as u32;
        let db = 20.0 * (segment.level / loudest.max(1e-9)).max(1e-4).log10();
        let velocity = (127.0 + 2.0 * db).clamp(40.0, 127.0) as u8;
        notes.push((
            Note {
                part,
                key: segment.key,
                velocity,
                start,
                length,
            },
            segment.attack,
        ));
    }
    clean_up(notes)
}

// the rounded midi note of each frame, or none when it is quiet or unclear
fn frame_keys(frames: &[PitchFrame], min_clarity: f32) -> Vec<Option<u8>> {
    let mut levels: Vec<f32> = frames.iter().map(|f| f.level).collect();
    levels.retain(|&l| l > 0.0);
    let floor = math::percentile(&levels, 0.98) * 0.03;

    let mut keys = Vec::with_capacity(frames.len());
    for frame in frames {
        if frame.hz > 0.0 && frame.clarity >= min_clarity && frame.level > floor {
            keys.push(Some(
                math::hz_to_midi(frame.hz).round().clamp(0.0, 127.0) as u8
            ));
        } else {
            keys.push(None);
        }
    }

    for i in 1..keys.len().saturating_sub(1) {
        if keys[i - 1].is_some() && keys[i - 1] == keys[i + 1] && keys[i] != keys[i - 1] {
            keys[i] = keys[i - 1];
        }
    }
    keys
}

// groups frames with the same note into segments, ending one when it fades 6 db below its peak
fn segments(keys: &[Option<u8>], frames: &[PitchFrame], attacks: &[bool]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut current: Option<Segment> = None;

    for i in 0..keys.len() {
        let continues = match (&current, keys[i]) {
            (Some(seg), Some(key)) => {
                seg.key == key && !attacks[i] && frames[i].level > 0.5 * seg.level
            }
            _ => false,
        };
        if continues {
            if let Some(seg) = current.as_mut() {
                seg.last = i;
                seg.level = seg.level.max(frames[i].level);
            }
            continue;
        }
        if let Some(seg) = current.take() {
            if seg.last - seg.first >= 2 {
                out.push(seg);
            }
        }
        if let Some(key) = keys[i] {
            current = Some(Segment {
                key,
                first: i,
                last: i,
                level: frames[i].level,
                attack: attacks[i] || (i > 0 && keys[i - 1].is_none()),
            });
        }
    }
    if let Some(seg) = current {
        if seg.last - seg.first >= 2 {
            out.push(seg);
        }
    }
    out
}

// keeps one note at a time, joins pieces of one held note and trims overlaps
fn clean_up(mut notes: Vec<(Note, bool)>) -> Vec<Note> {
    notes.sort_by_key(|(n, _)| (n.start, std::cmp::Reverse(n.length)));
    let mut out: Vec<Note> = Vec::new();

    for (note, attack) in notes {
        if let Some(last) = out.last_mut() {
            if note.start == last.start {
                continue;
            }
            let joins = note.key == last.key && note.start == last.start + last.length && !attack;
            if joins {
                last.length += note.length;
                continue;
            }
            if last.start + last.length > note.start {
                last.length = note.start - last.start;
            }
        }
        out.push(note);
    }
    out
}
