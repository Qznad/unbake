use crate::audio::signal::Signal;
use crate::music::Grid;
use serde::{Deserialize, Serialize};

const PHRASE: u32 = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub start_bar: u32,
    pub bars: u32,
}

// splits the song where the stems get clearly louder or quieter and names each part by what plays
pub fn detect(drums: &Signal, others: &[&Signal], grid: &Grid, bars: u32) -> Vec<Section> {
    let mut stems = vec![drums];
    stems.extend_from_slice(others);
    let levels: Vec<Vec<f64>> = stems.iter().map(|s| bar_levels(s, grid, bars)).collect();

    let mut last_bar = 0;
    for bar in 0..bars as usize {
        if levels.iter().any(|stem| stem[bar] > -50.0) {
            last_bar = bar as u32 + 1;
        }
    }
    if last_bar == 0 {
        return Vec::new();
    }

    let mut starts = vec![0];
    let mut bar = PHRASE;
    while bar < last_bar {
        let mut change = 0.0;
        for stem in &levels {
            let before = average(&stem[(bar - PHRASE) as usize..bar as usize]);
            let after = average(&stem[bar as usize..(bar + PHRASE).min(last_bar) as usize]);
            change += (after - before).abs();
        }
        if change / levels.len() as f64 > 6.0 {
            starts.push(bar);
        }
        bar += PHRASE;
    }

    let drum_top = levels[0].iter().cloned().fold(f64::MIN, f64::max);
    let mut sections: Vec<(u32, u32, bool, f64)> = Vec::new();
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(last_bar);
        let range = start as usize..end as usize;
        let drums_on = average(&levels[0][range.clone()]) > drum_top - 15.0;
        let mut energy = 0.0;
        for stem in &levels {
            energy += 10f64.powf(average(&stem[range.clone()]) / 10.0);
        }
        sections.push((start, end - start, drums_on, 10.0 * energy.log10()));
    }

    let loudest = sections.iter().map(|s| s.3).fold(f64::MIN, f64::max);
    let mut names: Vec<String> = Vec::new();
    for (i, &(_, _, drums_on, energy)) in sections.iter().enumerate() {
        let name = if i == 0 && !drums_on {
            "intro"
        } else if i == sections.len() - 1 && !drums_on {
            "outro"
        } else if drums_on && energy > loudest - 3.0 {
            "drop"
        } else if !drums_on {
            "break"
        } else {
            "verse"
        };
        names.push(name.to_string());
    }

    let mut out = Vec::new();
    for (i, &(start, length, _, _)) in sections.iter().enumerate() {
        let total = names.iter().filter(|n| **n == names[i]).count();
        let number = names[..=i].iter().filter(|n| **n == names[i]).count();
        let name = if total > 1 {
            format!("{} {}", names[i], number)
        } else {
            names[i].clone()
        };
        out.push(Section {
            name,
            start_bar: start,
            bars: length,
        });
    }
    out
}

// the loudness of every bar in decibels, never below -60
fn bar_levels(signal: &Signal, grid: &Grid, bars: u32) -> Vec<f64> {
    let mut levels = Vec::with_capacity(bars as usize);
    for bar in 0..bars {
        let start = grid.origin + bar as f64 * grid.bar_seconds();
        let piece = signal.slice(start, start + grid.bar_seconds());
        let mut power = 0.0;
        for &s in &piece.samples {
            power += (s * s) as f64;
        }
        let power = power / piece.samples.len().max(1) as f64;
        levels.push((10.0 * (power + 1e-12).log10()).max(-60.0));
    }
    levels
}

// the average of a list of numbers
fn average(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    // a steady tone whose loudness is set bar by bar
    fn bars_of(levels: &[f32], grid: &Grid, rate: u32) -> Signal {
        let mut samples = Vec::new();
        for &level in levels {
            for i in 0..(grid.bar_seconds() * rate as f64) as usize {
                samples.push(level * (i as f32 * 0.05).sin());
            }
        }
        Signal {
            samples,
            sample_rate: rate,
        }
    }

    // four bars without drums then eight bars with drums become an intro and a drop
    #[test]
    fn intro_then_drop() {
        let grid = Grid {
            bpm: 120.0,
            origin: 0.0,
        };
        let mut drum_levels = vec![0.0; 4];
        drum_levels.extend(vec![0.5; 8]);
        let drums = bars_of(&drum_levels, &grid, 8000);
        let pad = bars_of(&[0.2; 12], &grid, 8000);
        let sections = detect(&drums, &[&pad], &grid, 12);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["intro", "drop"]);
        assert_eq!(sections[1].start_bar, 4);
    }
}
