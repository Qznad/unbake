use crate::analysis::chroma::Chroma;
use crate::audio::math;
use crate::music::NOTE_NAMES;
use serde::{Deserialize, Serialize};

const MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Key {
    pub tonic: usize,
    pub minor: bool,
    pub confidence: f32,
}

impl Key {
    // the key written out, like a minor
    pub fn name(&self) -> String {
        let mode = if self.minor { "minor" } else { "major" };
        format!("{} {}", NOTE_NAMES[self.tonic], mode)
    }

    // the key with the same notes in the other mode, c major and a minor share all their notes
    pub fn relative(&self) -> Key {
        let tonic = if self.minor {
            (self.tonic + 3) % 12
        } else {
            (self.tonic + 9) % 12
        };
        Key {
            tonic,
            minor: !self.minor,
            confidence: self.confidence,
        }
    }
}

// all 24 major and minor keys, best match first, each with how well its profile matches the notes heard
pub fn ranked(chroma: &[Chroma]) -> Vec<Key> {
    let mut total = [0.0f32; 12];
    for frame in chroma {
        for pc in 0..12 {
            total[pc] += frame[pc];
        }
    }

    let mut keys = Vec::with_capacity(24);
    for tonic in 0..12 {
        for minor in [false, true] {
            let profile = if minor { MINOR } else { MAJOR };
            let mut rotated = [0.0f32; 12];
            for pc in 0..12 {
                rotated[(pc + tonic) % 12] = profile[pc];
            }
            keys.push(Key {
                tonic,
                minor,
                confidence: math::correlation(&total, &rotated),
            });
        }
    }
    keys.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    // the notes of a c major scale with c, e and g weighted up is detected as c major
    #[test]
    fn c_major_scale() {
        let mut chroma = [0.0f32; 12];
        for (pc, weight) in [
            (0, 3.0),
            (2, 1.0),
            (4, 2.0),
            (5, 1.0),
            (7, 2.5),
            (9, 1.0),
            (11, 1.0),
        ] {
            chroma[pc] = weight;
        }
        let key = ranked(&[chroma])[0];
        assert_eq!(key.name(), "C major");
        assert_eq!(key.relative().name(), "A minor");
        assert_eq!(ranked(&[chroma]).len(), 24);
    }
}
