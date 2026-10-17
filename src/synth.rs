use crate::audio::math;
use crate::audio::signal::Signal;
use crate::music::{Grid, Note, Part};
use serde::Serialize;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Wave {
    Sine,
    Triangle,
    Saw,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Sound {
    pub wave: Wave,
    pub cutoff: f32,
    pub release: f32,
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Voices {
    pub bass: Sound,
    pub melody: Sound,
    pub chords: Sound,
}

impl Voices {
    // the starting sounds before any matching
    pub fn defaults() -> Voices {
        Voices {
            bass: Sound {
                wave: Wave::Saw,
                cutoff: 800.0,
                release: 0.02,
                gain: 1.0,
            },
            melody: Sound {
                wave: Wave::Square,
                cutoff: 3200.0,
                release: 0.08,
                gain: 1.0,
            },
            chords: Sound {
                wave: Wave::Saw,
                cutoff: 1600.0,
                release: 0.25,
                gain: 0.5,
            },
        }
    }

    // the sound used for a part
    pub fn get(&self, part: Part) -> Sound {
        match part {
            Part::Bass => self.bass,
            Part::Melody => self.melody,
            _ => self.chords,
        }
    }

    // a copy with the sound of one part replaced
    pub fn with(&self, part: Part, sound: Sound) -> Voices {
        let mut copy = *self;
        match part {
            Part::Bass => copy.bass = sound,
            Part::Melody => copy.melody = sound,
            _ => copy.chords = sound,
        }
        copy
    }
}

struct Noise {
    state: u32,
}

impl Noise {
    // a noise generator that gives the same values every run
    fn new(seed: u32) -> Noise {
        Noise { state: seed.max(1) }
    }

    // the next random value between -1 and 1
    fn next(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

// plays notes and returns the audio, drums use built in drum sounds and other parts their voice
pub fn render(notes: &[Note], grid: &Grid, voices: &Voices, seconds: f64, rate: u32) -> Signal {
    let mut out = Signal::silence(seconds, rate);
    let mut noise = Noise::new(7);
    for note in notes {
        let start = (grid.tick_time(note.start) * rate as f64).round() as usize;
        let length = note.length as f64 * grid.tick_seconds();
        let level = note.velocity as f32 / 127.0;
        match note.part {
            Part::Kick => add_kick(&mut out.samples, start, level, rate),
            Part::Snare => add_snare(&mut out.samples, start, level, rate, &mut noise),
            Part::Hat => add_hat(&mut out.samples, start, level, rate, &mut noise),
            _ => add_tone(
                &mut out.samples,
                start,
                length,
                note.key,
                level,
                &voices.get(note.part),
                rate,
            ),
        }
    }
    out
}

// one synth note, a waveform through a low pass filter with a short fade in and a release tail
fn add_tone(
    out: &mut [f32],
    start: usize,
    length: f64,
    key: u8,
    level: f32,
    sound: &Sound,
    rate: u32,
) {
    let rate = rate as f32;
    let hz = math::midi_to_hz(key as f32);
    let release = sound.release.max(0.01);
    let held = (length as f32 * rate) as usize;
    let tail = (release * 6.0 * rate) as usize;
    let smoothing = 1.0 - (-2.0 * PI * sound.cutoff / rate).exp();

    let mut phase = 0.0f32;
    let mut filtered = 0.0f32;
    for i in 0..held + tail {
        let Some(slot) = out.get_mut(start + i) else {
            break;
        };
        phase = (phase + hz / rate).fract();
        filtered += smoothing * (wave_value(sound.wave, phase) - filtered);

        let mut envelope = (i as f32 / rate / 0.005).min(1.0);
        if i >= held {
            envelope *= (-((i - held) as f32 / rate) / release).exp();
        }
        *slot += 0.3 * sound.gain * level * envelope * filtered;
    }
}

// the value of a waveform at a point in its cycle, from 0 to 1
fn wave_value(wave: Wave, phase: f32) -> f32 {
    match wave {
        Wave::Sine => (2.0 * PI * phase).sin(),
        Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        Wave::Saw => 2.0 * phase - 1.0,
        Wave::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

// a kick drum, a low sine that drops quickly in pitch
fn add_kick(out: &mut [f32], start: usize, level: f32, rate: u32) {
    let rate = rate as f32;
    let mut phase = 0.0f32;
    for i in 0..(0.4 * rate) as usize {
        let Some(slot) = out.get_mut(start + i) else {
            break;
        };
        let t = i as f32 / rate;
        phase += 2.0 * PI * (45.0 + 90.0 * (-t / 0.05).exp()) / rate;
        *slot += 0.8 * level * phase.sin() * (-t / 0.25).exp();
    }
}

// a snare drum, a short tone with a burst of noise
fn add_snare(out: &mut [f32], start: usize, level: f32, rate: u32, noise: &mut Noise) {
    let rate = rate as f32;
    for i in 0..(0.3 * rate) as usize {
        let Some(slot) = out.get_mut(start + i) else {
            break;
        };
        let t = i as f32 / rate;
        let body = (2.0 * PI * 200.0 * t).sin() * (-t / 0.06).exp();
        let rattle = noise.next() * (-t / 0.12).exp();
        *slot += level * (0.3 * body + 0.4 * rattle);
    }
}

// a closed hi hat, very short bright noise
fn add_hat(out: &mut [f32], start: usize, level: f32, rate: u32, noise: &mut Noise) {
    let rate = rate as f32;
    let mut previous = 0.0;
    for i in 0..(0.08 * rate) as usize {
        let Some(slot) = out.get_mut(start + i) else {
            break;
        };
        let t = i as f32 / rate;
        let white = noise.next();
        *slot += 0.2 * level * (white - previous) * (-t / 0.03).exp();
        previous = white;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a rendered a4 on a sine wave is found again at 440 hz by the pitch tracker
    #[test]
    fn renders_the_right_pitch() {
        let grid = Grid {
            bpm: 120.0,
            origin: 0.0,
        };
        let note = Note {
            part: Part::Melody,
            key: 69,
            velocity: 100,
            start: 0,
            length: 192,
        };
        let mut voices = Voices::defaults();
        voices.melody = Sound {
            wave: Wave::Sine,
            cutoff: 8000.0,
            release: 0.05,
            gain: 1.0,
        };
        let signal = render(&[note], &grid, &voices, 1.5, 11025);
        let frames = crate::analysis::pitch::track(&signal, 128, 100.0, 1000.0);
        let hz = frames[frames.len() / 3].hz;
        assert!((hz - 440.0).abs() < 3.0, "got {}", hz);
    }
}
