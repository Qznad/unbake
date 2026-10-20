use crate::music::{Note, STEP};

const RANGE: u8 = 36;
const GHOST: f64 = 0.75;
const ECHO_STEPS: u32 = 3;

// a melody one instrument could play: in its main three octaves, without echoes or ghost notes, one note at a time
pub fn melody(notes: &[Note]) -> Vec<Note> {
    let mut line: Vec<Note> = notes.to_vec();
    if line.is_empty() {
        return line;
    }
    let low = main_range(&line);
    let high = low + RANGE;
    for note in &mut line {
        while note.key < low {
            note.key += 12;
        }
        while note.key > high {
            note.key -= 12;
        }
    }
    line = without_echoes(line);

    let mut velocities: Vec<u8> = line.iter().map(|n| n.velocity).collect();
    velocities.sort_unstable();
    let median = velocities.get(velocities.len() / 2).copied().unwrap_or(0) as f64;
    line.retain(|n| n.length > STEP || n.velocity as f64 >= GHOST * median);

    line.sort_by_key(|n| (n.start, std::cmp::Reverse(n.velocity)));
    line.dedup_by_key(|n| n.start);
    for i in 1..line.len() {
        let next = line[i].start;
        let note = &mut line[i - 1];
        if note.start + note.length > next {
            note.length = next - note.start;
        }
    }
    line
}

// drops notes between eighths that repeat a louder note of the same pitch from a few steps before, like a delay's echoes
fn without_echoes(line: Vec<Note>) -> Vec<Note> {
    line.iter()
        .filter(|n| {
            (n.start / STEP).is_multiple_of(2)
                || !line.iter().any(|m| {
                    m.start < n.start
                        && n.start - m.start <= ECHO_STEPS * STEP
                        && m.key == n.key
                        && m.velocity > n.velocity
                })
        })
        .copied()
        .collect()
}

// the lowest note of the three octave window holding the most of the line, weighted by loudness and length, centred on it when several hold as much
fn main_range(notes: &[Note]) -> u8 {
    let mut weight = [0f64; 128];
    for note in notes {
        weight[note.key as usize] += note.velocity as f64 * note.length as f64;
    }
    let total: f64 = weight.iter().sum();
    let mean = weight
        .iter()
        .enumerate()
        .map(|(key, w)| key as f64 * w)
        .sum::<f64>()
        / total.max(1e-9);
    let sum = |low: u8| {
        weight[low as usize..=(low + RANGE) as usize]
            .iter()
            .sum::<f64>()
    };
    let off_centre = |low: u8| (low as f64 + RANGE as f64 / 2.0 - mean).abs();
    (0..=(127 - RANGE))
        .max_by(|&a, &b| {
            sum(a)
                .total_cmp(&sum(b))
                .then(off_centre(b).total_cmp(&off_centre(a)))
        })
        .unwrap_or(48)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Part;

    // a note at a step with a key, a velocity and a length in steps
    fn note(start: u32, key: u8, velocity: u8, length: u32) -> Note {
        Note {
            part: Part::Melody,
            key,
            velocity,
            start: start * STEP,
            length: length * STEP,
        }
    }

    // a far low note folds up, an echo and a quiet blip go, and overlapping notes are cut at the next one
    #[test]
    fn tidies_a_line() {
        let notes = vec![
            note(0, 72, 100, 4),
            note(2, 74, 100, 4),
            note(3, 74, 60, 1),
            note(4, 24, 100, 2),
            note(6, 76, 40, 1),
            note(8, 79, 100, 2),
        ];
        let line = melody(&notes);
        let keys: Vec<u8> = line.iter().map(|n| n.key).collect();
        assert_eq!(keys, vec![72, 74, 60, 79]);
        assert_eq!(line[0].length, 2 * STEP);
    }
}
