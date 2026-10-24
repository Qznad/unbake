use crate::compose::{
    self, AudioClipSpec, ChannelSpec, ClipSpec, CopySpec, Loop, MarkerSpec, PatternCopy,
    PatternSpec,
};
use crate::flp::channels::{self, Block};
use crate::flp::events::{self, Event, Project};
use crate::flp::info;
use crate::flp::mixer;
use crate::flp::timeline::{self, Clip, ClipSource, Track};
use crate::music::{BAR, PPQ};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

type NoteKey = (String, u32, u32, u8, u8);

#[derive(Serialize)]
pub struct Playlist {
    pub bars: f64,
    pub tracks: Vec<TrackView>,
}

#[derive(Serialize)]
pub struct TrackView {
    pub track: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    pub clips: Vec<ClipView>,
}

#[derive(Serialize)]
pub struct ClipView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    pub bar: f64,
    pub bars: f64,
    #[serde(skip_serializing_if = "is_one")]
    pub times: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_bar: Option<f64>,
}

struct SectionChannel {
    name: String,
    base: String,
    sound: Vec<Event>,
}

struct SectionPattern {
    name: String,
    base: String,
    notes: Vec<NoteKey>,
}

struct Placed {
    pattern: u16,
    track: u16,
    position: i64,
    length: u32,
    cut: Option<u32>,
    fill: bool,
}

// whether a clip plays once, so its count can be left out
fn is_one(times: &u32) -> bool {
    *times == 1
}

// a project's playlist track by track, each clip with its bar, length and repeats, the way a loop description places clips
pub fn playlist(project: &Project) -> Playlist {
    let bar = project.ppq.max(1) as f64 * 4.0;
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let summary = info::summarize(project);
    let pattern_names: HashMap<u16, String> = summary
        .patterns
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    let channel_names: HashMap<u16, String> = summary
        .channels
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    let tracks = timeline::tracks(project);
    let mut clips = timeline::clips(project);
    clips.sort_by_key(|c| (c.track, c.start));
    let mut out: Vec<TrackView> = Vec::new();
    let mut end = 0;
    for clip in clips {
        end = end.max(clip.start + clip.length);
        let number = clip.track as u32 + 1;
        if out.last().is_none_or(|t| t.track != number) {
            let found = tracks.iter().find(|t| t.number == number);
            out.push(TrackView {
                track: number,
                name: found.and_then(|t| t.name.clone()),
                muted: found.is_some_and(|t| !t.enabled),
                clips: Vec::new(),
            });
        }
        let Some(track) = out.last_mut() else {
            continue;
        };
        let (pattern, audio) = match clip.source {
            ClipSource::Pattern(id) => (
                Some(
                    pattern_names
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| format!("Pattern {}", id)),
                ),
                None,
            ),
            ClipSource::Channel(id) => (
                None,
                Some(
                    channel_names
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| format!("Channel {}", id)),
                ),
            ),
        };
        let view = ClipView {
            pattern,
            audio,
            bar: round(clip.start as f64 / bar + 1.0),
            bars: round(clip.length as f64 / bar),
            times: 1,
            from_bar: clip
                .cut
                .filter(|&from| from > 0)
                .map(|from| round(from as f64 / bar + 1.0)),
        };
        if let Some(last) = track.clips.last_mut() {
            let follows = (last.bar + last.bars * last.times as f64 - view.bar).abs() < 0.01;
            if follows
                && last.pattern == view.pattern
                && last.audio == view.audio
                && last.bars == view.bars
                && last.from_bar == view.from_bar
            {
                last.times += 1;
                continue;
            }
        }
        track.clips.push(view);
    }
    Playlist {
        bars: round(end as f64 / bar),
        tracks: out,
    }
}

// a description with each section's finished loop project turned into channels, patterns and clips copied from it and placed at the section's bars
pub fn expand(spec: &Loop, out: &Path) -> Result<(Loop, Vec<String>), String> {
    let mut song = spec.clone();
    song.sections.clear();
    song.playlist = compose::default_playlist(spec);
    let mut warnings = Vec::new();

    let mut tracks = spec.tracks.clone();
    let busy = song
        .playlist
        .iter()
        .map(|c| c.track as usize)
        .max()
        .unwrap_or(0);
    if tracks.len() < busy {
        tracks.resize(busy, String::new());
    }
    let mut sections = Vec::new();
    for section in &spec.sections {
        let path = compose::project_path(&section.project, out);
        let project = events::read(&path)
            .map_err(|e| format!("can't read section {}: {}", section.name, e))?;
        sections.push((section, path.display().to_string(), project));
    }
    let mut playing_somewhere: Vec<String> = Vec::new();
    for (_, _, project) in &sections {
        let summary = info::summarize(project);
        let their_tracks = timeline::tracks(project);
        for clip in timeline::clips(project) {
            let (name, on) = track_name(&their_tracks, &clip, &summary);
            if on && !playing_somewhere.contains(&name) {
                playing_somewhere.push(name);
            }
        }
    }
    let mut muted_tracks: Vec<usize> = Vec::new();
    let mut sounds: Vec<SectionChannel> = Vec::new();
    let mut lines: Vec<SectionPattern> = Vec::new();

    for (section, source, project) in &sections {
        let summary = info::summarize(project);
        if let Some(bpm) = summary.tempo {
            if (bpm - spec.tempo).abs() > 0.01 {
                warnings.push(format!(
                    "section {} is {} bpm but the song is {} bpm",
                    section.name, bpm, spec.tempo
                ));
            }
        }
        let scale = |tick: u32| (tick as u64 * PPQ as u64 / project.ppq.max(1) as u64) as u32;
        let their_tracks = timeline::tracks(project);
        let blocks = channels::blocks(project);
        let all = timeline::clips(project);
        let mut clips: Vec<Clip> = all
            .iter()
            .filter(|c| matches!(c.source, ClipSource::Pattern(_)))
            .copied()
            .collect();
        clips.sort_by_key(|c| (c.start, c.track));
        let mut sounding: Vec<Clip> = Vec::new();
        let mut left_out = 0;
        for clip in &all {
            let ClipSource::Channel(id) = clip.source else {
                continue;
            };
            let (_, on) = track_name(&their_tracks, clip, &summary);
            let is_audio = blocks
                .iter()
                .any(|b| b.id == id && b.kind == channels::AUDIO_CLIP);
            match (on, is_audio) {
                (true, true) => sounding.push(*clip),
                (true, false) => left_out += 1,
                (false, _) => {}
            }
        }
        if left_out > 0 {
            warnings.push(format!(
                "section {} has {} automation clips, which were left out",
                section.name, left_out
            ));
        }
        let loop_end = clips
            .iter()
            .map(|c| scale(c.start + c.length))
            .max()
            .ok_or_else(|| {
                format!(
                    "section {} has no pattern clips in its playlist",
                    section.name
                )
            })?;
        let loop_length = loop_end.div_ceil(BAR) * BAR;
        let length = section
            .bars
            .map_or(loop_length, |bars| (bars * BAR as f64).round() as u32);
        if length == 0 {
            return Err(format!("section {} is 0 bars long", section.name));
        }

        let notes = timeline::pattern_notes(project);
        let mut used: Vec<u16> = Vec::new();
        for clip in &clips {
            if let ClipSource::Pattern(id) = clip.source {
                if !used.contains(&id) {
                    used.push(id);
                }
            }
        }
        let playing: Vec<u16> = used
            .iter()
            .flat_map(|id| notes.get(id).into_iter().flatten().map(|n| n.channel))
            .chain(sounding.iter().filter_map(|c| match c.source {
                ClipSource::Channel(id) => Some(id),
                ClipSource::Pattern(_) => None,
            }))
            .collect();

        let mut channel_names: HashMap<u16, String> = HashMap::new();
        let mut their_names: HashMap<u16, String> = HashMap::new();
        for block in &blocks {
            if !playing.contains(&block.id) {
                continue;
            }
            their_names.insert(block.id, block.name.clone());
            if spec.channels.iter().any(|c| c.name == block.name) {
                channel_names.insert(block.id, block.name.clone());
                continue;
            }
            let sound = sound_of(project, block);
            if let Some(same) = sounds
                .iter()
                .find(|s| s.base == block.name && s.sound == sound)
            {
                channel_names.insert(block.id, same.name.clone());
                continue;
            }
            let taken: Vec<String> = spec
                .channels
                .iter()
                .map(|c| c.name.clone())
                .chain(sounds.iter().map(|s| s.name.clone()))
                .collect();
            let wanted = if block.kind == channels::AUDIO_CLIP && is_stem(&block.name) {
                format!("{} ({})", block.name, section.name)
            } else {
                block.name.clone()
            };
            let name = unique_name(&wanted, &section.name, &taken);
            if name != wanted {
                warnings.push(format!(
                    "{} in section {} sounds different from the {} before it, so it was kept as {}",
                    block.name, section.name, block.name, name
                ));
            }
            song.channels.push(ChannelSpec {
                name: name.clone(),
                sample: None,
                root: None,
                copy: Some(CopySpec {
                    project: source.clone(),
                    channel: block.name.clone(),
                }),
                effects: true,
                volume: None,
            });
            channel_names.insert(block.id, name.clone());
            sounds.push(SectionChannel {
                name,
                base: block.name.clone(),
                sound,
            });
        }

        let mut pattern_names: HashMap<u16, String> = HashMap::new();
        for &id in &used {
            let base = summary
                .patterns
                .iter()
                .find(|p| p.id == id)
                .map_or_else(|| format!("Pattern {}", id), |p| p.name.clone());
            let mut key: Vec<NoteKey> = notes
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|n| {
                    let channel = channel_names.get(&n.channel)?.clone();
                    Some((channel, scale(n.start), scale(n.length), n.key, n.velocity))
                })
                .collect();
            key.sort();
            if let Some(same) = lines.iter().find(|l| l.base == base && l.notes == key) {
                pattern_names.insert(id, same.name.clone());
                continue;
            }
            let taken: Vec<String> = spec
                .patterns
                .iter()
                .map(|p| p.name.clone())
                .chain(lines.iter().map(|l| l.name.clone()))
                .collect();
            let name = unique_name(&base, &section.name, &taken);
            let longest = clips
                .iter()
                .filter(|c| c.source == ClipSource::Pattern(id))
                .map(|c| scale(c.length))
                .max()
                .unwrap_or(BAR);
            let mapping: HashMap<String, String> = notes
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|n| {
                    Some((
                        their_names.get(&n.channel)?.clone(),
                        channel_names.get(&n.channel)?.clone(),
                    ))
                })
                .collect();
            song.patterns.push(PatternSpec {
                name: name.clone(),
                bars: longest as f64 / BAR as f64,
                notes: Vec::new(),
                grids: Vec::new(),
                copy: Some(PatternCopy {
                    project: source.clone(),
                    pattern: base.clone(),
                    channels: mapping,
                    transpose: 0,
                    from_bar: None,
                    bars: None,
                }),
                from_parts: None,
            });
            pattern_names.insert(id, name.clone());
            lines.push(SectionPattern {
                name,
                base,
                notes: key,
            });
        }

        let mut track_numbers: HashMap<u16, u16> = HashMap::new();
        for clip in clips.iter().chain(&sounding) {
            if track_numbers.contains_key(&clip.track) {
                continue;
            }
            let (name, on) = track_name(&their_tracks, clip, &summary);
            let name = if matches!(clip.source, ClipSource::Channel(_)) && is_stem(&name) {
                format!("{} ({})", name, section.name)
            } else if !on && playing_somewhere.contains(&name) {
                format!("{} (muted)", name)
            } else {
                name
            };
            let index = match tracks.iter().position(|t| *t == name) {
                Some(index) => index,
                None => {
                    tracks.push(name);
                    tracks.len() - 1
                }
            };
            if !on && !muted_tracks.contains(&index) {
                muted_tracks.push(index);
            }
            track_numbers.insert(clip.track, index as u16 + 1);
        }

        let repeats = length.div_ceil(loop_length).max(1);
        let name_of = |id: u16| {
            summary
                .patterns
                .iter()
                .find(|p| p.id == id)
                .map_or("", |p| p.name.as_str())
        };
        let mut placed: Vec<Placed> = Vec::new();
        for (n, clip) in clips.iter().enumerate() {
            let ClipSource::Pattern(id) = clip.source else {
                continue;
            };
            let start = scale(clip.start) as i64;
            let length_of = scale(clip.length);
            let fill = section.last_only.iter().any(|p| p == name_of(id));
            let positions: Vec<i64> = if fill {
                vec![length as i64 - loop_length as i64 + start]
            } else if section.first_only.iter().any(|p| p == name_of(id)) {
                vec![start]
            } else {
                (0..repeats)
                    .map(|r| (r * loop_length) as i64 + start)
                    .collect()
            };
            for position in positions {
                placed.push(Placed {
                    pattern: id,
                    track: clip.track,
                    position,
                    length: length_of,
                    cut: clip.cut.map(scale),
                    fill,
                });
            }
            let before = clips[..n]
                .iter()
                .rev()
                .find(|b| b.track == clip.track && b.start + b.length == clip.start);
            if let (true, Some(before)) = (fill, before) {
                let ClipSource::Pattern(before_id) = before.source else {
                    continue;
                };
                let step = scale(before.length).max(1);
                for r in 0..repeats - 1 {
                    let slot = (r * loop_length) as i64 + start;
                    let mut k = 0;
                    while k < length_of {
                        placed.push(Placed {
                            pattern: before_id,
                            track: clip.track,
                            position: slot + k as i64,
                            length: step.min(length_of - k),
                            cut: before.cut.map(scale),
                            fill: false,
                        });
                        k += step;
                    }
                }
            }
        }
        let fills: Vec<(u16, i64, i64)> = placed
            .iter()
            .filter(|p| p.fill)
            .map(|p| (p.track, p.position, p.position + p.length as i64))
            .collect();
        for &at in &section.at {
            for p in &placed {
                let mut end = (p.position + p.length as i64).min(length as i64);
                if !p.fill {
                    for &(track, from, to) in &fills {
                        if track == p.track && p.position < to && from < end {
                            end = end.min(from);
                        }
                    }
                }
                if p.position < 0 || end <= p.position {
                    continue;
                }
                song.playlist.push(section_clip(
                    &pattern_names[&p.pattern],
                    track_numbers[&p.track],
                    at,
                    p.position as u32,
                    (end - p.position) as u32,
                    p.cut,
                ));
            }
            for clip in &sounding {
                let ClipSource::Channel(id) = clip.source else {
                    continue;
                };
                for r in 0..repeats {
                    let position = r * loop_length + scale(clip.start);
                    if position >= length {
                        continue;
                    }
                    let bars = scale(clip.length).min(length - position) as f64 / BAR as f64;
                    song.audio_clips.push(AudioClipSpec {
                        channel: channel_names[&id].clone(),
                        track: track_numbers[&clip.track],
                        bar: at + position as f64 / BAR as f64,
                        bars,
                    });
                }
            }
            song.markers.push(MarkerSpec {
                bar: at,
                name: section.name.clone(),
            });
        }
        let mastered = summary
            .inserts
            .first()
            .is_some_and(|master| !master.effects.is_empty());
        if song.master.is_none() && mastered {
            song.master = Some(source.clone());
        }
    }

    muted_tracks.sort();
    for index in muted_tracks {
        let number = index as u16 + 1;
        if !song.mute.contains(&number) {
            song.mute.push(number);
        }
    }
    song.markers.sort_by(|a, b| a.bar.total_cmp(&b.bar));
    song.tracks = tracks;
    Ok((song, warnings))
}

// whether a name is one of the song's stems, like Drums stem
fn is_stem(name: &str) -> bool {
    name.to_lowercase().ends_with("stem")
}

// a section clip's track name, its own or else the name of what it plays, and whether that track is switched on
fn track_name(tracks: &[Track], clip: &Clip, summary: &info::Summary) -> (String, bool) {
    let found = tracks.iter().find(|t| t.number == clip.track as u32 + 1);
    let name = found
        .and_then(|t| t.name.clone())
        .or_else(|| match clip.source {
            ClipSource::Pattern(id) => summary
                .patterns
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone()),
            ClipSource::Channel(id) => summary
                .channels
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone()),
        })
        .unwrap_or_else(|| format!("Track {}", clip.track + 1));
    (name, found.is_none_or(|t| t.enabled))
}

// one clip of a section placed in the song, with its length and where it starts inside its pattern in ticks
fn section_clip(
    pattern: &str,
    track: u16,
    at: f64,
    position: u32,
    length: u32,
    cut: Option<u32>,
) -> ClipSpec {
    let bar = BAR as f64;
    ClipSpec {
        pattern: pattern.to_string(),
        track,
        bar: at + position as f64 / bar,
        times: 1,
        at: Vec::new(),
        bars: Some(length as f64 / bar),
        from_bar: cut.map(|from| from as f64 / bar + 1.0),
    }
}

// what makes a channel sound the way it does: its events without its number and mixer routing, plus its insert's effects
fn sound_of(project: &Project, block: &Block) -> Vec<Event> {
    let mut sound: Vec<Event> = project.events[block.start + 1..block.end]
        .iter()
        .filter(|e| e.id != events::CHANNEL_INSERT)
        .cloned()
        .collect();
    if let Some(insert) = mixer::channel_insert(project, block.start, block.end) {
        sound.extend(mixer::effects(project, insert));
    }
    sound
}

// a name nobody uses yet, with the section's name added in brackets when the plain one is taken
fn unique_name(base: &str, section: &str, taken: &[String]) -> String {
    let mut name = base.to_string();
    let mut n = 1;
    while taken.contains(&name) {
        name = if n == 1 {
            format!("{} ({})", base, section)
        } else {
            format!("{} ({} {})", base, section, n)
        };
        n += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::write;

    // writes a loop description into a test folder and returns the project's path
    fn loop_project(dir: &Path, description: &str) -> String {
        let spec: Loop = serde_json::from_str(description).unwrap();
        write(&spec, dir, None).unwrap().path
    }

    // a loop's clips come back per track with their repeats, and a muted track says so
    #[test]
    fn reads_the_playlist_back() {
        let dir = std::env::temp_dir().join("unbake_playlist_test");
        let path = loop_project(
            &dir,
            r#"{
                "name": "playlist", "tempo": 140, "bars": 4,
                "channels": [ { "name": "Kick", "sample": "C:/kick.wav" }, { "name": "Hat", "sample": "C:/hat.wav" } ],
                "patterns": [
                    { "name": "Kick", "grids": [ { "channel": "Kick", "hits": "x...x...x...x..." } ] },
                    { "name": "Hat", "bars": 2, "grids": [ { "channel": "Hat", "hits": "..x...x...x...x." } ] }
                ],
                "playlist": [ { "pattern": "Kick", "track": 1, "times": 4 }, { "pattern": "Hat", "track": 2, "bar": 3, "bars": 1 } ],
                "mute": [2]
            }"#,
        );
        let playlist = playlist(&events::read(Path::new(&path)).unwrap());
        assert_eq!(playlist.bars, 4.0);
        assert_eq!(playlist.tracks.len(), 2);
        let kick = &playlist.tracks[0];
        assert_eq!(kick.name.as_deref(), Some("Kick"));
        assert!(!kick.muted);
        assert_eq!(kick.clips.len(), 1);
        assert_eq!(
            (kick.clips[0].bar, kick.clips[0].bars, kick.clips[0].times),
            (1.0, 1.0, 4)
        );
        let hat = &playlist.tracks[1];
        assert!(hat.muted);
        assert_eq!((hat.clips[0].bar, hat.clips[0].bars), (3.0, 1.0));
    }

    // two section loops become one song: a kick both share stays one channel, a lead that sounds different keeps its section's name on the same track, repeats fill the section and fills and crashes play only where asked
    #[test]
    fn combines_sections() {
        let dir = std::env::temp_dir().join("unbake_combine_test");
        loop_project(
            &dir,
            r#"{
                "name": "drop", "tempo": 140, "bars": 2,
                "channels": [
                    { "name": "Kick", "sample": "C:/kick.wav" },
                    { "name": "Crash", "sample": "C:/crash.wav" },
                    { "name": "Lead", "sample": "C:/lead.wav" }
                ],
                "patterns": [
                    { "name": "Kick", "grids": [ { "channel": "Kick", "hits": "x...x...x...x..." } ] },
                    { "name": "Fill", "grids": [ { "channel": "Kick", "hits": "x.x.x.x.xxxxxxxx" } ] },
                    { "name": "Crash", "grids": [ { "channel": "Crash", "hits": "x..............." } ] },
                    { "name": "Lead", "bars": 2, "notes": [ { "channel": "Lead", "key": "C5", "steps": 8, "velocity": 64 } ] }
                ],
                "tracks": ["Kick", "Crash", "Lead"],
                "playlist": [
                    { "pattern": "Kick", "track": 1 },
                    { "pattern": "Fill", "track": 1, "bar": 2 },
                    { "pattern": "Crash", "track": 2 },
                    { "pattern": "Lead", "track": 3 }
                ]
            }"#,
        );
        loop_project(
            &dir,
            r#"{
                "name": "intro", "tempo": 140, "bars": 2,
                "channels": [ { "name": "Kick", "sample": "C:/kick.wav" }, { "name": "Lead", "sample": "C:/soft lead.wav" } ],
                "patterns": [
                    { "name": "Kick", "grids": [ { "channel": "Kick", "hits": "x...x...x...x..." } ] },
                    { "name": "Lead", "bars": 2, "notes": [ { "channel": "Lead", "key": "C5", "steps": 8, "velocity": 64 } ] }
                ],
                "tracks": ["Kick", "Lead"]
            }"#,
        );
        let song: Loop = serde_json::from_str(
            r#"{
                "name": "song", "tempo": 140,
                "sections": [
                    { "name": "Drop", "project": "drop.flp", "at": [3], "bars": 6, "first_only": ["Crash"], "last_only": ["Fill"] },
                    { "name": "Intro", "project": "intro.flp", "at": [1] }
                ]
            }"#,
        )
        .unwrap();
        let (expanded, warnings) = expand(&song, &dir).unwrap();
        let names: Vec<&str> = expanded.channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Kick", "Crash", "Lead", "Lead (Intro)"]);
        assert_eq!(warnings.len(), 1);
        let bars_of = |pattern: &str, track: u16| {
            let mut bars: Vec<f64> = expanded
                .playlist
                .iter()
                .filter(|c| c.pattern == pattern && c.track == track)
                .map(|c| c.bar)
                .collect();
            bars.sort_by(f64::total_cmp);
            bars
        };
        assert_eq!(bars_of("Kick", 1), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(bars_of("Fill", 1), vec![8.0]);
        assert_eq!(bars_of("Crash", 2), vec![3.0]);
        assert_eq!(bars_of("Lead", 3), vec![3.0, 5.0, 7.0]);
        assert_eq!(bars_of("Lead (Intro)", 3), vec![1.0]);
        let written = write(&song, &dir, None).unwrap();
        let project = events::read(Path::new(&written.path)).unwrap();
        let summary = info::summarize(&project);
        let lead = summary.patterns.iter().find(|p| p.name == "Lead").unwrap();
        let velocities: Vec<u8> = timeline::pattern_notes(&project)[&lead.id]
            .iter()
            .map(|n| n.velocity)
            .collect();
        assert_eq!(velocities, vec![64]);
        let markers: Vec<&str> = expanded.markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(markers, vec!["Intro", "Drop"]);
    }

    // a track muted in one section but playing in another keeps its muted clips on a muted track of their own
    #[test]
    fn keeps_muted_section_tracks_muted() {
        let dir = std::env::temp_dir().join("unbake_combine_mute_test");
        let section = |name: &str, mute: &str| {
            format!(
                r#"{{
                    "name": "{}", "tempo": 140, "bars": 1,
                    "channels": [ {{ "name": "Kick", "sample": "C:/kick.wav" }}, {{ "name": "Hat", "sample": "C:/hat.wav" }} ],
                    "patterns": [
                        {{ "name": "Kick", "grids": [ {{ "channel": "Kick", "hits": "x...x...x...x..." }} ] }},
                        {{ "name": "Hat", "grids": [ {{ "channel": "Hat", "hits": "..x...x...x...x." }} ] }}
                    ],
                    "tracks": ["Kick", "Hat"],
                    "mute": [{}]
                }}"#,
                name, mute
            )
        };
        loop_project(&dir, &section("quiet", "1"));
        loop_project(&dir, &section("loud", ""));
        let song: Loop = serde_json::from_str(
            r#"{
                "name": "mute song", "tempo": 140,
                "sections": [
                    { "name": "Loud", "project": "loud.flp", "at": [2] },
                    { "name": "Quiet", "project": "quiet.flp", "at": [1] }
                ]
            }"#,
        )
        .unwrap();
        let (expanded, _) = expand(&song, &dir).unwrap();
        assert_eq!(expanded.tracks, vec!["Kick", "Hat", "Kick (muted)"]);
        assert_eq!(expanded.mute, vec![3]);
        let on = |track: u16| {
            let mut bars: Vec<f64> = expanded
                .playlist
                .iter()
                .filter(|c| c.track == track)
                .map(|c| c.bar)
                .collect();
            bars.sort_by(f64::total_cmp);
            bars
        };
        assert_eq!(on(1), vec![2.0]);
        assert_eq!(on(2), vec![1.0, 2.0]);
        assert_eq!(on(3), vec![1.0]);
    }
}
