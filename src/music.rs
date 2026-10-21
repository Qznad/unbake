use serde::{Deserialize, Serialize};

pub const PPQ: u32 = 96;
pub const STEP: u32 = PPQ / 4;
pub const BAR: u32 = PPQ * 4;

pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    Kick,
    Snare,
    Hat,
    Bass,
    Melody,
    Chords,
}

impl Part {
    // the name used for files, tracks and channels
    pub fn name(&self) -> &'static str {
        match self {
            Part::Kick => "kick",
            Part::Snare => "snare",
            Part::Hat => "hat",
            Part::Bass => "bass",
            Part::Melody => "melody",
            Part::Chords => "chords",
        }
    }

    // the name of the part's fl studio channel and of its sound file
    pub fn channel_name(&self) -> &'static str {
        match self {
            Part::Kick => "Kick",
            Part::Snare => "Snare",
            Part::Hat => "Hat",
            Part::Bass => "Bass",
            Part::Melody => "Lead",
            Part::Chords => "Pad",
        }
    }

    // true for the parts that come from the drum stem
    pub fn is_drum(&self) -> bool {
        matches!(self, Part::Kick | Part::Snare | Part::Hat)
    }

    // the general midi drum key, used so drum tracks play the right sound in any daw
    pub fn drum_key(&self) -> u8 {
        match self {
            Part::Kick => 36,
            Part::Snare => 38,
            _ => 42,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub part: Part,
    pub key: u8,
    pub velocity: u8,
    pub start: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Grid {
    pub bpm: f64,
    pub origin: f64,
}

impl Grid {
    // how many seconds one tick lasts
    pub fn tick_seconds(&self) -> f64 {
        60.0 / self.bpm / PPQ as f64
    }

    // how many seconds one bar lasts
    pub fn bar_seconds(&self) -> f64 {
        self.tick_seconds() * BAR as f64
    }

    // the time in seconds of a tick
    pub fn tick_time(&self, tick: u32) -> f64 {
        self.origin + tick as f64 * self.tick_seconds()
    }

    // the tick of a time, snapped to the nearest step, none before the first bar
    pub fn step_tick(&self, time: f64) -> Option<u32> {
        let steps = ((time - self.origin) / (self.tick_seconds() * STEP as f64)).round();
        if steps < 0.0 {
            return None;
        }
        Some(steps as u32 * STEP)
    }

    // how many bars fit in a length of time
    pub fn bar_count(&self, seconds: f64) -> u32 {
        ((seconds - self.origin) / self.bar_seconds())
            .ceil()
            .max(0.0) as u32
    }
}

// a note number as a name with an octave, like a3 for midi note 57
pub fn note_name(key: u8) -> String {
    format!("{}{}", NOTE_NAMES[key as usize % 12], key as i32 / 12 - 1)
}

// the midi key of a note name like C4, D#3 or Eb5, where C4 is 60
pub fn parse_note(name: &str) -> Option<u8> {
    let name = name.trim();
    let mut chars = name.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, octave) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest)
    };
    let octave: i32 = octave.parse().ok()?;
    let key = (octave + 1) * 12 + base + shift;
    (0..=127).contains(&key).then_some(key as u8)
}
