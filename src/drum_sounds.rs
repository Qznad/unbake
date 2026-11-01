use crate::analysis::drums::{self, BANDS, Bands};
use crate::analysis::spectrum::Spectrogram;
use crate::audio::convert;
use crate::audio::signal::Signal;
use crate::audio::wav;
use crate::compose;
use crate::music::{Note, Part};
use crate::rebuild::{self, Session};
use crate::separate;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FRAME_SECONDS: f64 = 0.0232;
const FRAMES: usize = 20;
const ATTACK_FRAMES: usize = 6;
const FLOOR_DB: f32 = -50.0;
const BEFORE: f64 = 0.03;
const ALONE: f64 = 0.1;
const MOST_HITS: usize = 12;
const FEWEST_HITS: usize = 3;
const LONGEST_SAMPLE: f64 = 2.0;
const CLIP_HITS: usize = 4;
const CLIP_GAP: f64 = 0.43;
const RANGES: [f64; 4] = [120.0, 500.0, 1000.0, 5000.0];
const RING_WEIGHT: f64 = 0.02;
const LAYER_POOL: usize = 10;
const LAYERS: usize = 3;
const FITS: f64 = 0.1;
const CLOSE: f64 = 0.3;
const SHARE_GAP: f64 = 0.1;

#[derive(Clone)]
struct Print {
    cells: Vec<Bands>,
}

pub struct Reference {
    part: Part,
    session: Session,
    stem: Signal,
    attacks: Vec<f64>,
    song: Print,
    pack: Vec<(String, Print)>,
}

#[derive(Serialize)]
pub struct Traits {
    pub brightness_khz: f64,
    pub sub: f64,
    pub body: f64,
    pub mid: f64,
    pub crack: f64,
    pub sizzle: f64,
    pub tail_db: f64,
}

#[derive(Serialize)]
pub struct Suggestion {
    pub sample: String,
    pub rank: usize,
    pub distance: f64,
    pub gap: f64,
    #[serde(flatten)]
    pub traits: Traits,
}

#[derive(Serialize)]
pub struct Layer {
    pub samples: Vec<String>,
    pub gap: f64,
    #[serde(flatten)]
    pub traits: Traits,
}

#[derive(Serialize)]
pub struct Suggestions {
    pub part: String,
    pub song_hits: usize,
    pub song: Traits,
    pub compared: usize,
    pub closest: Vec<Suggestion>,
    pub layers: Vec<Layer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<String>,
}

#[derive(Serialize)]
pub struct Judged {
    pub rank: usize,
    pub compared: usize,
    pub verdict: String,
}

// the song's own sound of a drum, from the hits that play alone in its drums stem, next to every pack sample of that drum
pub fn reference(folder: &Path, part: Part) -> Result<Reference, String> {
    let session = rebuild::load_session(folder)?;
    let stem_path = folder.join(separate::stem_file("drums"));
    let stem = Signal::from_wav(&wav::read_wav(&stem_path)?);
    let attacks = song_attacks(&session, &stem, part);
    if attacks.len() < FEWEST_HITS {
        return Err(format!(
            "only {} {} hits play alone in the song, too few to tell its sound",
            attacks.len(),
            part.name()
        ));
    }
    let prints: Vec<Print> = attacks
        .iter()
        .map(|&t| print(&stem.slice(t, t + 0.2)))
        .collect();
    let song = median(&prints);
    let pack = pack_prints(part)?;
    Ok(Reference {
        part,
        session,
        stem,
        attacks,
        song,
        pack,
    })
}

// the pack samples of a drum that sound closest to the song's own hits, pairs of them that come closer layered, and listening clips when asked
pub fn suggest(
    folder: &Path,
    part: Part,
    limit: usize,
    listen: Option<&Path>,
) -> Result<Suggestions, String> {
    let reference = reference(folder, part)?;
    let song = traits(&reference.song);
    let mut ranked: Vec<(&String, f64, &Print)> = reference
        .pack
        .iter()
        .map(|(name, p)| (name, distance(&reference.song, p), p))
        .collect();
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(b.0)));

    let closest: Vec<Suggestion> = ranked
        .iter()
        .take(limit)
        .enumerate()
        .map(|(n, (name, d, p))| {
            let own = traits(p);
            Suggestion {
                sample: name.to_string(),
                rank: n + 1,
                distance: round(*d),
                gap: round(gap(&song, &own)),
                traits: own,
            }
        })
        .collect();
    let layers = layers(&song, &ranked[..ranked.len().min(LAYER_POOL)]);

    let mut clips = Vec::new();
    if let Some(dir) = listen {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("can't create {}: {}", dir.display(), e))?;
        let song_clip = song_clip(&reference);
        let level = hit_level(&song_clip);
        let mut save = |file: String, clip: Signal| -> Result<(), String> {
            let path = dir.join(file);
            wav::write_wav(&path, &clip.to_wav())?;
            clips.push(path.display().to_string());
            Ok(())
        };
        save(format!("song {}.wav", part.name()), song_clip)?;
        let work = std::env::temp_dir().join("unbake").join("clips");
        for single in &closest {
            let sound = load_sound(&single.sample, &work)?;
            save(
                format!("{} {}.wav", single.rank, short_name(&single.sample)),
                sample_clip(&sound, level),
            )?;
        }
        for (n, layer) in layers.iter().enumerate() {
            let sounds = layer
                .samples
                .iter()
                .map(|s| load_sound(s, &work))
                .collect::<Result<Vec<_>, _>>()?;
            let names: Vec<String> = layer.samples.iter().map(|s| short_name(s)).collect();
            save(
                format!("layer {} {}.wav", n + 1, names.join(" + ")),
                sample_clip(&mixed(&sounds), level),
            )?;
        }
    }

    Ok(Suggestions {
        part: part.name().to_string(),
        song_hits: reference.attacks.len(),
        song,
        compared: ranked.len(),
        closest,
        layers,
        clips,
    })
}

// how close one sample, or several layered, comes to the song's own hits of a drum, ranked against every pack sample and said in producer words
pub fn judge(reference: &Reference, samples: &[String]) -> Result<Judged, String> {
    let work = std::env::temp_dir().join("unbake").join("judge");
    let prints = samples
        .iter()
        .map(|s| load_sound(s, &work).map(|sound| print(&sound)))
        .collect::<Result<Vec<_>, _>>()?;
    let mine = layered(&prints.iter().collect::<Vec<_>>());
    let own = distance(&reference.song, &mine);
    let compared = reference.pack.len();
    let rank = 1 + reference
        .pack
        .iter()
        .filter(|(_, p)| distance(&reference.song, p) < own)
        .count();
    let share = rank as f64 / compared.max(1) as f64;
    let part = reference.part.name();
    let differences = differences(&traits(&reference.song), &traits(&mine));
    let what = if samples.len() > 1 {
        "together they"
    } else {
        "it"
    };
    let verdict = if share <= FITS {
        format!(
            "sounds like the song's {}: {} would rank {} of FL Studio's {} {}s",
            part, what, rank, compared, part
        )
    } else if share <= CLOSE {
        format!(
            "near the song's {}, {} would rank {} of {}: {}",
            part, what, rank, compared, differences
        )
    } else {
        format!(
            "doesn't sound like the song's {}, {} would rank {} of {}: {}; suggest_sounds lists closer ones",
            part, what, rank, compared, differences
        )
    };
    Ok(Judged {
        rank,
        compared,
        verdict,
    })
}

// the pairs from the closest samples whose layered sound matches the song's body, crack and sizzle better than any of them alone
fn layers(song: &Traits, pool: &[(&String, f64, &Print)]) -> Vec<Layer> {
    let best_single = pool
        .iter()
        .map(|(_, _, p)| gap(song, &traits(p)))
        .fold(f64::MAX, f64::min);
    let mut found = Vec::new();
    for (i, (first, _, a)) in pool.iter().enumerate() {
        for (second, _, b) in pool.iter().skip(i + 1) {
            let together = traits(&layered(&[a, b]));
            let g = gap(song, &together);
            if g < best_single {
                found.push(Layer {
                    samples: vec![first.to_string(), second.to_string()],
                    gap: round(g),
                    traits: together,
                });
            }
        }
    }
    found.sort_by(|a, b| a.gap.total_cmp(&b.gap));
    found.truncate(LAYERS);
    found
}

// the exact start of the loudest hits of a drum that play with no other drum just before or after them
fn song_attacks(session: &Session, stem: &Signal, part: Part) -> Vec<f64> {
    let grid = &session.grid;
    let time = |n: &Note| grid.tick_time(n.start);
    let mut hits: Vec<&Note> = session.drums.iter().filter(|n| n.part == part).collect();
    hits.sort_by_key(|h| std::cmp::Reverse(h.velocity));
    let alone = |hit: &Note| {
        let t = time(hit);
        session.drums.iter().all(|other| {
            std::ptr::eq(other, hit)
                || (other.start != hit.start
                    && (time(other) <= t - BEFORE || time(other) >= t + ALONE))
        })
    };
    let mut attacks: Vec<f64> = hits
        .into_iter()
        .filter(|h| alone(h))
        .take(MOST_HITS)
        .map(|h| attack(stem, time(h)))
        .collect();
    attacks.sort_by(|a, b| a.total_cmp(b));
    attacks
}

// where a hit really starts near a time on the grid: the loudest moment around it, walked back to where it began to rise
fn attack(stem: &Signal, near: f64) -> f64 {
    let rate = stem.sample_rate as f64;
    let block = 64;
    let from = ((near - 0.06).max(0.0) * rate) as usize;
    let to = (((near + 0.06) * rate) as usize).min(stem.samples.len());
    let energies: Vec<f32> = stem.samples[from.min(to)..to]
        .chunks(block)
        .map(|c| c.iter().map(|s| s * s).sum::<f32>())
        .collect();
    let Some(loudest) = (0..energies.len()).max_by(|&a, &b| energies[a].total_cmp(&energies[b]))
    else {
        return near;
    };
    let mut start = loudest;
    while start > 0 && energies[start - 1] >= 0.2 * energies[loudest] {
        start -= 1;
    }
    (from + start * block) as f64 / rate
}

// how a hit sounds over its first 120 ms: 40 bands spaced like hearing in short frames, in decibels below its loudest cell
fn print(sound: &Signal) -> Print {
    let rate = sound.sample_rate as f64;
    let frame = ((FRAME_SECONDS * rate) as usize).max(64);
    let hop = (frame / 4).max(1);
    let length = frame / 2 + hop * FRAMES;
    let mut head = sound.slice(0.0, length as f64 / rate);
    head.samples.resize(length, 0.0);
    let spec = Spectrogram::new(&head, frame, hop);
    let cells: Vec<Bands> = drums::band_frames(&spec)
        .into_iter()
        .take(FRAMES)
        .map(|bands| bands.map(|v| 20.0 * (v + 1e-9).log10()))
        .collect();
    below_loudest(cells)
}

// cells in decibels moved so the loudest is 0, with everything quieter than the floor raised to it
fn below_loudest(mut cells: Vec<Bands>) -> Print {
    let loudest = cells
        .iter()
        .flat_map(|b| b.iter().copied())
        .fold(f32::MIN, f32::max);
    for bands in cells.iter_mut() {
        for v in bands.iter_mut() {
            *v = (*v - loudest).max(FLOOR_DB);
        }
    }
    Print { cells }
}

// the print of several sounds played together, each at the same peak level
fn layered(prints: &[&Print]) -> Print {
    let mut cells = vec![[0.0f32; BANDS]; FRAMES];
    for (f, bands) in cells.iter_mut().enumerate() {
        for (b, value) in bands.iter_mut().enumerate() {
            let power: f64 = prints
                .iter()
                .map(|p| 10f64.powf(p.cells[f][b] as f64 / 10.0))
                .sum();
            *value = (10.0 * power.log10()) as f32;
        }
    }
    below_loudest(cells)
}

// the middle value of every cell over several prints, so one hit with something else in it doesn't count much
fn median(prints: &[Print]) -> Print {
    let mut cells = vec![[0.0f32; BANDS]; FRAMES];
    for (f, bands) in cells.iter_mut().enumerate() {
        for (b, value) in bands.iter_mut().enumerate() {
            let mut values: Vec<f32> = prints.iter().map(|p| p.cells[f][b]).collect();
            values.sort_by(|x, y| x.total_cmp(y));
            *value = values[values.len() / 2];
        }
    }
    Print { cells }
}

// how far apart two prints are in decibels, loud cells counting more than quiet ones the way the ear does
fn distance(a: &Print, b: &Print) -> f64 {
    let mut total = 0.0;
    let mut weights = 0.0;
    for (x, y) in a.cells.iter().zip(&b.cells) {
        for (p, q) in x.iter().zip(y) {
            let weight = 10f64.powf(p.max(*q) as f64 / 20.0);
            total += weight * (p - q).abs() as f64;
            weights += weight;
        }
    }
    total / weights.max(1e-12)
}

// what a print says in producer terms: how bright its attack is, its share of sub, body, mid, crack and sizzle, and how much it still rings after 70 ms
fn traits(print: &Print) -> Traits {
    let power = |db: f32| 10f64.powf(db as f64 / 10.0);
    let centres = band_centres();
    let mut weighted = 0.0;
    let mut ranges = [0.0f64; 5];
    for bands in print.cells.iter().take(ATTACK_FRAMES) {
        for (b, &db) in bands.iter().enumerate() {
            let p = power(db);
            weighted += p * centres[b];
            let range = RANGES.iter().position(|&top| centres[b] < top).unwrap_or(4);
            ranges[range] += p;
        }
    }
    let total: f64 = ranges.iter().sum::<f64>().max(1e-12);
    let frame_db = |bands: &Bands| 10.0 * bands.iter().map(|&db| power(db)).sum::<f64>().log10();
    let level = |range: std::ops::Range<usize>| {
        let frames = &print.cells[range];
        frames.iter().map(frame_db).sum::<f64>() / frames.len().max(1) as f64
    };
    Traits {
        brightness_khz: round(weighted / total / 1000.0),
        sub: round(ranges[0] / total),
        body: round(ranges[1] / total),
        mid: round(ranges[2] / total),
        crack: round(ranges[3] / total),
        sizzle: round(ranges[4] / total),
        tail_db: round(level(12..FRAMES) - level(0..4)),
    }
}

// how far two sounds are apart in sub, body, mid, crack and sizzle, plus a little for how long they ring
fn gap(a: &Traits, b: &Traits) -> f64 {
    (a.sub - b.sub).abs()
        + (a.body - b.body).abs()
        + (a.mid - b.mid).abs()
        + (a.crack - b.crack).abs()
        + (a.sizzle - b.sizzle).abs()
        + RING_WEIGHT * (a.tail_db - b.tail_db).abs()
}

// how a sound differs from the song's in producer words, the biggest differences only
fn differences(song: &Traits, mine: &Traits) -> String {
    let mut out = Vec::new();
    for (name, theirs, ours) in [
        ("body", song.body, mine.body),
        ("crack", song.crack, mine.crack),
        ("sizzle", song.sizzle, mine.sizzle),
    ] {
        if ours - theirs >= SHARE_GAP {
            out.push(format!("more {} ({:.2} against {:.2})", name, ours, theirs));
        } else if theirs - ours >= SHARE_GAP {
            out.push(format!("less {} ({:.2} against {:.2})", name, ours, theirs));
        }
    }
    let ratio = mine.brightness_khz / song.brightness_khz.max(0.01);
    if ratio >= 1.3 {
        out.push(format!(
            "brighter ({:.1} kHz against {:.1})",
            mine.brightness_khz, song.brightness_khz
        ));
    } else if ratio <= 0.77 {
        out.push(format!(
            "darker ({:.1} kHz against {:.1})",
            mine.brightness_khz, song.brightness_khz
        ));
    }
    if mine.tail_db - song.tail_db >= 10.0 {
        out.push("rings longer".to_string());
    } else if song.tail_db - mine.tail_db >= 10.0 {
        out.push("dies away faster".to_string());
    }
    if out.is_empty() {
        "close in body, crack and sizzle, but not in the details".to_string()
    } else {
        out.join(", ")
    }
}

// the middle frequency of each of the 40 bands, spaced like hearing from 30 hz to 16 khz
fn band_centres() -> Vec<f64> {
    let mel = |hz: f64| 2595.0 * (1.0 + hz / 700.0).log10();
    let hz = |m: f64| 700.0 * (10f64.powf(m / 2595.0) - 1.0);
    let (low, high) = (mel(30.0), mel(16000.0));
    (0..BANDS)
        .map(|i| hz(low + (high - low) * (i as f64 + 0.5) / BANDS as f64))
        .collect()
}

// the one-shot samples in fl studio's packs whose path names the drum, loops left out
fn pack_samples(part: Part) -> Result<Vec<String>, String> {
    let words: &[&str] = match part {
        Part::Kick => &["kick"],
        Part::Snare => &["snare"],
        Part::Hat => &["hat"],
        _ => return Err(format!("{} is not a drum", part.name())),
    };
    let mut found = Vec::new();
    for word in words {
        for path in compose::find_samples(word, usize::MAX)? {
            if !path.to_lowercase().contains("loop") && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    Ok(found)
}

// every pack sample of a drum with its print, made once and then read back from a cache file
fn pack_prints(part: Part) -> Result<Vec<(String, Print)>, String> {
    let names = pack_samples(part)?;
    let path = cache_file(part);
    let cached = read_cache(&path);
    let size_of = |name: &str| std::fs::metadata(compose::local_path(name)).map_or(0, |m| m.len());
    let mut entries: Vec<(String, u64, Option<Print>)> = Vec::new();
    let mut missing = Vec::new();
    for name in names {
        let size = size_of(&name);
        match cached.get(&name) {
            Some((was, found)) if *was == size => entries.push((name, size, found.clone())),
            _ => missing.push(name),
        }
    }
    if !missing.is_empty() {
        let loaded: HashMap<String, Signal> = load_all(&missing).into_iter().collect();
        for name in missing {
            let found = loaded.get(&name).map(print);
            let size = size_of(&name);
            entries.push((name, size, found));
        }
        write_cache(&path, &entries);
    }
    Ok(entries
        .into_iter()
        .filter_map(|(name, _, found)| found.map(|p| (name, p)))
        .collect())
}

// where unbake keeps the prints of fl studio's samples between runs
fn cache_file(part: Part) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("unbake")
        .join(format!("drum prints {} v1.bin", part.name()))
}

// the prints an earlier run saved, by sample, with the file size each was made from, none for samples that are not a hit
fn read_cache(path: &Path) -> HashMap<String, (u64, Option<Print>)> {
    let mut out = HashMap::new();
    let Ok(bytes) = std::fs::read(path) else {
        return out;
    };
    let mut at = 0;
    while at < bytes.len() {
        let Some(entry) = read_entry(&bytes, &mut at) else {
            break;
        };
        out.insert(entry.0, (entry.1, entry.2));
    }
    out
}

// one saved sample from the cache bytes: its name, file size and print
fn read_entry(bytes: &[u8], at: &mut usize) -> Option<(String, u64, Option<Print>)> {
    let length = u16::from_le_bytes(take(bytes, at, 2)?.try_into().ok()?) as usize;
    let name = String::from_utf8(take(bytes, at, length)?.to_vec()).ok()?;
    let size = u64::from_le_bytes(take(bytes, at, 8)?.try_into().ok()?);
    let frames = u16::from_le_bytes(take(bytes, at, 2)?.try_into().ok()?) as usize;
    let mut cells = Vec::with_capacity(frames);
    for _ in 0..frames {
        let raw = take(bytes, at, BANDS * 4)?;
        let mut bands = [0.0f32; BANDS];
        for (b, value) in bands.iter_mut().enumerate() {
            *value = f32::from_le_bytes(raw[b * 4..b * 4 + 4].try_into().ok()?);
        }
        cells.push(bands);
    }
    let found = (frames == FRAMES).then_some(Print { cells });
    Some((name, size, found))
}

// the next few bytes of a buffer, moving past them
fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Option<&'a [u8]> {
    let slice = bytes.get(*at..*at + count)?;
    *at += count;
    Some(slice)
}

// saves the prints for the next run, quietly giving up when the folder can't be written
fn write_cache(path: &Path, entries: &[(String, u64, Option<Print>)]) {
    let mut bytes = Vec::new();
    for (name, size, found) in entries {
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend(size.to_le_bytes());
        let cells = found.as_ref().map_or(&[][..], |p| &p.cells[..]);
        bytes.extend((cells.len() as u16).to_le_bytes());
        for bands in cells {
            for value in bands {
                bytes.extend(value.to_le_bytes());
            }
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, bytes);
}

// one sample loaded and trimmed to where it starts, from a pack, the user's folder or anywhere else
fn load_sound(name: &str, work: &Path) -> Result<Signal, String> {
    let wav = convert::load(&crate::render::expand(name), work)?;
    Ok(Signal::from_wav(&wav).trim_start())
}

// every sample loaded on a few threads at once, skipping the ones too long to be a hit
fn load_all(samples: &[String]) -> Vec<(String, Signal)> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let chunk = samples.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = samples
            .chunks(chunk)
            .enumerate()
            .map(|(n, part)| {
                scope.spawn(move || {
                    let work = std::env::temp_dir()
                        .join("unbake")
                        .join(format!("samples {}", n));
                    part.iter()
                        .filter_map(|name| {
                            let sound = load_sound(name, &work).ok()?;
                            (sound.duration() <= LONGEST_SAMPLE && sound.peak() > 0.0)
                                .then(|| (name.clone(), sound))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

// a few of the song's lone hits one after another, each cut before the next drum
fn song_clip(reference: &Reference) -> Signal {
    let session = &reference.session;
    let times: Vec<f64> = session
        .drums
        .iter()
        .map(|n| session.grid.tick_time(n.start))
        .collect();
    let stem = &reference.stem;
    let mut out = Signal::silence(0.0, stem.sample_rate);
    for &t in reference.attacks.iter().take(CLIP_HITS) {
        let next = times
            .iter()
            .copied()
            .filter(|&x| x > t + BEFORE)
            .fold(t + CLIP_GAP - 0.03, f64::min);
        let hit = stem
            .slice((t - 0.005).max(0.0), next - 0.005)
            .fade_out(0.015);
        out.samples.extend(&hit.samples);
        pad_to(&mut out, CLIP_GAP);
    }
    out
}

// several sounds played together, each brought to the same loudness and to the first one's sample rate
fn mixed(sounds: &[Signal]) -> Signal {
    let rate = sounds.first().map_or(44100, |s| s.sample_rate);
    let mut out = Signal::silence(0.0, rate);
    for sound in sounds {
        let sound = at_rate(sound, rate);
        let gain = 0.1 / hit_level(&sound).max(1e-6);
        if out.samples.len() < sound.samples.len() {
            out.samples.resize(sound.samples.len(), 0.0);
        }
        for (o, s) in out.samples.iter_mut().zip(&sound.samples) {
            *o += s * gain;
        }
    }
    out
}

// a sound resampled to another rate by straight lines between its samples
fn at_rate(sound: &Signal, rate: u32) -> Signal {
    if sound.sample_rate == rate {
        return sound.clone();
    }
    let step = sound.sample_rate as f64 / rate as f64;
    let count = (sound.samples.len() as f64 / step) as usize;
    let samples = (0..count)
        .map(|i| {
            let x = i as f64 * step;
            let j = x as usize;
            let next = sound.samples.get(j + 1).copied().unwrap_or(0.0);
            let here = sound.samples[j];
            here + (next - here) * (x - j as f64) as f32
        })
        .collect();
    Signal {
        samples,
        sample_rate: rate,
    }
}

// a sample played a few times at the same spacing as the song's clip, as loud as the song's hits
fn sample_clip(sound: &Signal, level: f32) -> Signal {
    let hit = sound.slice(0.0, CLIP_GAP - 0.03).fade_out(0.015);
    let own = hit_level(&hit).max(1e-6);
    let gain = (level / own).min(0.99 / hit.peak().max(1e-6));
    let hit = hit.scaled(gain);
    let mut out = Signal::silence(0.0, sound.sample_rate);
    for _ in 0..CLIP_HITS {
        out.samples.extend(&hit.samples);
        pad_to(&mut out, CLIP_GAP);
    }
    out
}

// how loud the first 120 ms of each hit in a clip are, as a root mean square
fn hit_level(clip: &Signal) -> f32 {
    let rate = clip.sample_rate as f64;
    let gap = (CLIP_GAP * rate) as usize;
    let head = (0.12 * rate) as usize;
    let mut total = 0.0;
    let mut count = 0;
    for start in (0..clip.samples.len()).step_by(gap.max(1)) {
        for s in clip.samples.iter().skip(start).take(head) {
            total += s * s;
            count += 1;
        }
    }
    (total / count.max(1) as f32).sqrt()
}

// fills a clip with silence up to the next multiple of the spacing
fn pad_to(clip: &mut Signal, spacing: f64) {
    let step = (spacing * clip.sample_rate as f64) as usize;
    let length = clip.samples.len().div_ceil(step.max(1)) * step;
    clip.samples.resize(length, 0.0);
}

// a sample's file name without its folders and extension
fn short_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    file.rsplit_once('.')
        .map_or(file, |(name, _)| name)
        .to_string()
}

// a number rounded to two decimals
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

// the drum named by a word like kick, snare or hat
pub fn drum_part(name: &str) -> Option<Part> {
    match name.to_lowercase().as_str() {
        "kick" => Some(Part::Kick),
        "snare" => Some(Part::Snare),
        "hat" | "hats" | "hihat" => Some(Part::Hat),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a short burst of noise, low and thumpy or high and crisp, with a decay
    fn burst(low: bool, decay: f64) -> Signal {
        let rate = 44100;
        let mut seed = 12345u32;
        let samples = (0..(0.3 * rate as f64) as usize)
            .map(|i| {
                let t = i as f64 / rate as f64;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                let tone = (2.0 * std::f64::consts::PI * 200.0 * t).sin() as f32;
                let sound = if low { tone } else { noise };
                sound * (-t / decay).exp() as f32
            })
            .collect();
        Signal {
            samples,
            sample_rate: rate,
        }
    }

    // a thump reads as body, noise as crack and sizzle, and a long decay rings longer
    #[test]
    fn describes_sounds_in_producer_terms() {
        let thump = traits(&print(&burst(true, 0.05)));
        let hiss = traits(&print(&burst(false, 0.05)));
        assert!(thump.body > 0.8);
        assert!(hiss.crack + hiss.sizzle > 0.7);
        let long = traits(&print(&burst(false, 0.5)));
        assert!(long.tail_db > hiss.tail_db + 10.0);
        assert!(differences(&thump, &hiss).contains("less body"));
    }

    // a sound is nearest to itself, and layering a thump with noise lands between the two
    #[test]
    fn layers_land_between_their_sounds() {
        let thump = print(&burst(true, 0.05));
        let hiss = print(&burst(false, 0.05));
        assert_eq!(distance(&thump, &thump), 0.0);
        assert!(distance(&thump, &hiss) > 5.0);
        let both = traits(&layered(&[&thump, &hiss]));
        assert!(both.body > traits(&hiss).body && both.body < traits(&thump).body);
    }

    // the cache gives back the prints it saved, and marks samples that are not a hit
    #[test]
    fn cache_round_trips() {
        let path = std::env::temp_dir().join("unbake_drum_cache_test.bin");
        let p = print(&burst(true, 0.05));
        let entries = vec![
            ("a.wav".to_string(), 10, Some(p.clone())),
            ("loop.wav".to_string(), 20, None),
        ];
        write_cache(&path, &entries);
        let back = read_cache(&path);
        assert_eq!(back["a.wav"].0, 10);
        assert_eq!(back["a.wav"].1.as_ref().unwrap().cells, p.cells);
        assert!(back["loop.wav"].1.is_none());
    }
}
