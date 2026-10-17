use crate::flp::events::{self, Project, Value};

pub const NOTE_SIZE: usize = 24;

pub struct Channel {
    pub id: u16,
    pub name: String,
}

pub struct PatternInfo {
    pub id: u16,
    pub name: String,
    pub notes: usize,
}

pub struct Summary {
    pub version: String,
    pub ppq: u16,
    pub tempo: Option<f64>,
    pub channels: Vec<Channel>,
    pub patterns: Vec<PatternInfo>,
    pub playlist_items: usize,
}

// collects the version, tempo, channels, patterns and playlist size of a project
pub fn summarize(project: &Project) -> Summary {
    let mut tempo = None;
    let mut channels: Vec<Channel> = Vec::new();
    let mut patterns: Vec<PatternInfo> = Vec::new();
    let mut playlist_items = 0;
    let mut current_pattern: Option<usize> = None;
    let mut in_channel = false;

    for event in &project.events {
        match (event.id, &event.value) {
            (events::TEMPO, Value::Dword(v)) => tempo = Some(*v as f64 / 1000.0),
            (events::CHANNEL_NEW, Value::Word(id)) => {
                channels.push(Channel {
                    id: *id,
                    name: format!("Channel {}", id),
                });
                in_channel = true;
                current_pattern = None;
            }
            (events::CHANNEL_NAME, Value::Data(data)) if in_channel => {
                if let Some(channel) = channels.last_mut() {
                    channel.name = events::decode_text(data);
                }
            }
            (events::PATTERN_NEW, Value::Word(id)) => {
                in_channel = false;
                current_pattern = Some(match patterns.iter().position(|p| p.id == *id) {
                    Some(i) => i,
                    None => {
                        patterns.push(PatternInfo {
                            id: *id,
                            name: format!("Pattern {}", id),
                            notes: 0,
                        });
                        patterns.len() - 1
                    }
                });
            }
            (events::PATTERN_NAME, Value::Data(data)) => {
                if let Some(i) = current_pattern {
                    patterns[i].name = events::decode_text(data);
                }
            }
            (events::PATTERN_NOTES, Value::Data(data)) => {
                if let Some(i) = current_pattern {
                    patterns[i].notes += data.len() / NOTE_SIZE;
                }
            }
            (events::ARRANGEMENT_NEW, _) => {
                in_channel = false;
                current_pattern = None;
            }
            (events::PLAYLIST, Value::Data(data)) => {
                playlist_items += data.len() / playlist_item_size(project);
            }
            _ => {}
        }
    }

    Summary {
        version: project.version(),
        ppq: project.ppq,
        tempo,
        channels,
        patterns,
        playlist_items,
    }
}

// how many bytes one playlist clip takes, fl studio 21 added 28 bytes to each one
pub fn playlist_item_size(project: &Project) -> usize {
    if project.major_version() >= 21 {
        60
    } else {
        32
    }
}
