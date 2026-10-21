use crate::flp::events::{self, Event, Project, Value};

pub const SAMPLER: u8 = 0;
pub const AUDIO_CLIP: u8 = 4;

#[derive(Clone, Debug)]
pub struct Block {
    pub start: usize,
    pub end: usize,
    pub id: u16,
    pub kind: u8,
    pub name: String,
}

// where each channel's events start and end, with its number, type and name
pub fn blocks(project: &Project) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (i, event) in project.events.iter().enumerate() {
        let ends_block = matches!(
            event.id,
            events::CHANNEL_NEW | events::PATTERN_NEW | events::ARRANGEMENT_NEW
        );
        if ends_block {
            if let Some(last) = out.last_mut() {
                if last.end == usize::MAX {
                    last.end = i;
                }
            }
        }
        match (event.id, &event.value) {
            (events::CHANNEL_NEW, Value::Word(id)) => out.push(Block {
                start: i,
                end: usize::MAX,
                id: *id,
                kind: SAMPLER,
                name: String::new(),
            }),
            (events::CHANNEL_TYPE, Value::Byte(kind)) => {
                if let Some(last) = out.last_mut().filter(|b| b.end == usize::MAX) {
                    last.kind = *kind;
                }
            }
            (events::CHANNEL_NAME, Value::Data(data)) => {
                if let Some(last) = out.last_mut().filter(|b| b.end == usize::MAX) {
                    last.name = events::decode_text(data);
                }
            }
            _ => {}
        }
    }
    if let Some(last) = out.last_mut() {
        if last.end == usize::MAX {
            last.end = project.events.len();
        }
    }
    out
}

// a copy of a channel's events with a new number, name, sample file and the note the sample plays
pub fn copy_channel(
    project: &Project,
    model: &Block,
    id: u16,
    name: &str,
    sample: &str,
    root: u8,
) -> Vec<Event> {
    let mut copy: Vec<Event> = project.events[model.start..model.end].to_vec();
    for event in copy.iter_mut() {
        match event.id {
            events::CHANNEL_NEW => event.value = Value::Word(id),
            events::CHANNEL_NAME => event.value = Value::Data(events::encode_text(name)),
            _ => {}
        }
    }
    set_sample(&mut copy, sample, root);
    copy
}

// a copy of another project's channel with its plugin and preset, given a new number and name
pub fn copy_block(from: &Project, block: &Block, id: u16, name: &str) -> Vec<Event> {
    let mut copy: Vec<Event> = from.events[block.start..block.end].to_vec();
    if let Some(first) = copy.first_mut() {
        first.value = Value::Word(id);
    }
    let text = Value::Data(events::encode_text(name));
    match copy.iter_mut().find(|e| e.id == events::CHANNEL_NAME) {
        Some(event) => event.value = text,
        None => {
            let at = copy
                .iter()
                .position(|e| e.id == events::PLUGIN_DATA)
                .or_else(|| copy.iter().position(|e| e.id == events::PLUGIN_NAME))
                .map_or(1, |i| i + 1)
                .min(copy.len());
            copy.insert(
                at,
                Event {
                    id: events::CHANNEL_NAME,
                    value: text,
                },
            );
        }
    }
    copy
}

// a channel's volume as fl studio stores it, where 12800 is full and 10000 the default
pub fn volume(events: &[Event]) -> Option<u32> {
    events.iter().find_map(|e| match (e.id, &e.value) {
        (events::CHANNEL_LEVELS, Value::Data(data)) if data.len() >= 8 => {
            Some(u32::from_le_bytes([data[4], data[5], data[6], data[7]]))
        }
        _ => None,
    })
}

// sets a channel's volume, where 12800 is full and 10000 the default
pub fn set_volume(events: &mut [Event], volume: u32) {
    for event in events.iter_mut() {
        if let (events::CHANNEL_LEVELS, Value::Data(data)) = (event.id, &mut event.value) {
            if data.len() >= 8 {
                data[4..8].copy_from_slice(&volume.to_le_bytes());
            }
        }
    }
}

// sends a channel's events to a mixer insert
pub fn route(events: &mut [Event], insert: u8) {
    for event in events.iter_mut() {
        if event.id == events::CHANNEL_INSERT {
            event.value = Value::Byte(insert);
        }
    }
}

// gives an existing channel a new sample file and root note, and a new name when one is given
pub fn set_channel(project: &mut Project, id: u16, name: Option<&str>, sample: &str, root: u8) {
    let Some(block) = blocks(project).into_iter().find(|b| b.id == id) else {
        return;
    };
    let mut events: Vec<Event> = project.events[block.start..block.end].to_vec();
    if let Some(name) = name {
        for event in events.iter_mut() {
            if event.id == events::CHANNEL_NAME {
                event.value = Value::Data(events::encode_text(name));
            }
        }
    }
    set_sample(&mut events, sample, root);
    project.events.splice(block.start..block.end, events);
}

// points a channel's events at a sample file and its root note, placed where fl studio writes them
fn set_sample(events: &mut Vec<Event>, sample: &str, root: u8) {
    let value = Value::Data(events::encode_text(sample));
    let path_at = match events.iter().position(|e| e.id == events::SAMPLE_PATH) {
        Some(i) => {
            events[i].value = value;
            i
        }
        None => {
            let at = events
                .iter()
                .position(|e| e.id == events::PING_PONG)
                .map(|i| i + 1)
                .unwrap_or(events.len());
            events.insert(
                at,
                Event {
                    id: events::SAMPLE_PATH,
                    value,
                },
            );
            at
        }
    };

    events.retain(|e| e.id != events::ROOT_NOTE);
    if root != 60 {
        let at = events
            .iter()
            .position(|e| e.id == events::SAMPLE_PATH)
            .unwrap_or(path_at);
        events.insert(
            at + 1,
            Event {
                id: events::ROOT_NOTE,
                value: Value::Dword(root as u32),
            },
        );
    }
}
