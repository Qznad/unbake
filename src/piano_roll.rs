use crate::music::{BAR, Note, PPQ, STEP, note_name};
use std::collections::{HashMap, HashSet};

const STEP_PX: f64 = 8.0;
const KEY_PX: u32 = 7;
const MIN_KEYS: u8 = 24;
pub const COLORS: [(&str, [u8; 3]); 4] = [
    ("orange", [255, 150, 30]),
    ("cyan", [40, 200, 255]),
    ("pink", [240, 90, 220]),
    ("green", [130, 230, 70]),
];
pub const SHARED: (&str, [u8; 3]) = ("white", [235, 235, 235]);

pub struct Layer<'a> {
    pub notes: &'a [Note],
    pub color: [u8; 3],
}

pub struct Image {
    pub png: Vec<u8>,
    pub lowest: u8,
    pub highest: u8,
}

// draws the notes of each layer as a piano roll png, time across and pitch up, for some bars from a first bar
pub fn render(layers: &[Layer], from_bar: u32, bars: u32) -> Result<Image, String> {
    let start = from_bar * BAR;
    let end = start + bars.max(1) * BAR;
    let visible = |n: &&Note| n.start < end && n.start + n.length > start;
    let keys: Vec<u8> = layers
        .iter()
        .flat_map(|l| l.notes.iter().filter(visible).map(|n| n.key))
        .collect();
    let (mut lowest, mut highest) = match (keys.iter().min(), keys.iter().max()) {
        (Some(&low), Some(&high)) => (low.saturating_sub(2), high.saturating_add(2).min(127)),
        _ => (48, 72),
    };
    while highest - lowest < MIN_KEYS {
        lowest = lowest.saturating_sub(1);
        highest = (highest + 1).min(127);
    }

    let width = ((end - start) as f64 / STEP as f64 * STEP_PX) as u32;
    let rows = (highest - lowest + 1) as u32;
    let height = rows * KEY_PX;
    let mut pixels = vec![0u8; (width * height * 3) as usize];
    let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 3]| {
        for y in y0..y1.min(height) {
            for x in x0..x1.min(width) {
                let i = ((y * width + x) * 3) as usize;
                pixels[i..i + 3].copy_from_slice(&color);
            }
        }
    };

    for row in 0..rows {
        let key = highest - row as u8;
        let black = matches!(key % 12, 1 | 3 | 6 | 8 | 10);
        let shade = if black { [24, 24, 30] } else { [36, 36, 44] };
        fill(0, row * KEY_PX, width, (row + 1) * KEY_PX, shade);
        if key.is_multiple_of(12) {
            fill(
                0,
                (row + 1) * KEY_PX - 1,
                width,
                (row + 1) * KEY_PX,
                [70, 70, 84],
            );
        }
    }
    let x_of = |tick: u32| ((tick.saturating_sub(start)) as f64 / STEP as f64 * STEP_PX) as u32;
    let mut tick = start;
    while tick < end {
        let color = if tick.is_multiple_of(BAR) {
            [120, 120, 140]
        } else {
            [55, 55, 66]
        };
        let x = x_of(tick);
        fill(x, 0, x + 1, height, color);
        tick += PPQ;
    }

    let mut layers_with: HashMap<(u32, u8), usize> = HashMap::new();
    for layer in layers {
        let own: HashSet<(u32, u8)> = layer.notes.iter().map(|n| (n.start, n.key)).collect();
        for place in own {
            *layers_with.entry(place).or_default() += 1;
        }
    }
    for layer in layers {
        for note in layer.notes.iter().filter(visible) {
            if note.key < lowest || note.key > highest {
                continue;
            }
            let shared = layers_with[&(note.start, note.key)] > 1;
            let base = if shared { SHARED.1 } else { layer.color };
            let level = 0.45 + 0.55 * note.velocity as f64 / 127.0;
            let color = base.map(|c| (c as f64 * level) as u8);
            let x0 = x_of(note.start.max(start));
            let x1 = x_of((note.start + note.length).min(end)).max(x0 + 2);
            let row = (highest - note.key) as u32;
            fill(x0 + 1, row * KEY_PX + 1, x1, (row + 1) * KEY_PX - 1, color);
        }
    }

    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&pixels)
            .map_err(|e| e.to_string())?;
    }
    Ok(Image {
        png,
        lowest,
        highest,
    })
}

// a line saying which notes the bottom and top rows are and how many pixels a step and a key take
pub fn scale(image: &Image) -> String {
    format!(
        "rows go from {} at the bottom to {} at the top, {} pixels per key, {} pixels per 1/16 step, bright lines are bars and dim lines are beats",
        note_name(image.lowest),
        note_name(image.highest),
        KEY_PX,
        STEP_PX
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Part;

    // two bars with one note make a png as wide as 32 steps and at least two octaves tall
    #[test]
    fn draws_a_png() {
        let note = Note {
            part: Part::Melody,
            key: 60,
            velocity: 100,
            start: 0,
            length: PPQ,
        };
        let image = render(
            &[Layer {
                notes: &[note],
                color: COLORS[0].1,
            }],
            0,
            2,
        )
        .unwrap();
        assert_eq!(&image.png[1..4], b"PNG");
        assert!(image.lowest <= 58 && image.highest >= 62);
        assert!(image.highest - image.lowest >= MIN_KEYS);
    }
}
