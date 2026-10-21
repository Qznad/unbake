use crate::audio::wav::{read_u16_le, read_u32_le};
use std::path::Path;

pub const PING_PONG: u8 = 20;
pub const CHANNEL_TYPE: u8 = 21;
pub const CHANNEL_INSERT: u8 = 22;
pub const SLOT_INDEX: u8 = 98;
pub const CHANNEL_NEW: u8 = 64;
pub const PATTERN_NEW: u8 = 65;
pub const ARRANGEMENT_NEW: u8 = 99;
pub const ROOT_NOTE: u8 = 135;
pub const MARKER_POSITION: u8 = 148;
pub const TEMPO: u8 = 156;
pub const PATTERN_NAME: u8 = 193;
pub const TITLE: u8 = 194;
pub const VERSION: u8 = 199;
pub const PLUGIN_NAME: u8 = 201;
pub const PLUGIN_DATA: u8 = 212;
pub const CHANNEL_NAME: u8 = 203;
pub const INSERT_NAME: u8 = 204;
pub const CHANNEL_LEVELS: u8 = 219;
pub const MARKER_NAME: u8 = 205;
pub const SAMPLE_PATH: u8 = 196;
pub const DATA_PATH: u8 = 202;
pub const PATTERN_NOTES: u8 = 224;
pub const PLAYLIST: u8 = 233;
pub const TRACK_DATA: u8 = 238;
pub const INSERT_ROUTING: u8 = 235;
pub const INSERT_FLAGS: u8 = 236;
pub const TRACK_NAME: u8 = 239;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Word(u16),
    Dword(u32),
    Data(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: u8,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub format: u16,
    pub channel_count: u16,
    pub ppq: u16,
    pub events: Vec<Event>,
}

impl Project {
    // the project as flp file bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for event in &self.events {
            body.push(event.id);
            match &event.value {
                Value::Byte(v) => body.push(*v),
                Value::Word(v) => body.extend_from_slice(&v.to_le_bytes()),
                Value::Dword(v) => body.extend_from_slice(&v.to_le_bytes()),
                Value::Data(data) => {
                    push_varint(&mut body, data.len());
                    body.extend_from_slice(data);
                }
            }
        }

        let mut out = Vec::with_capacity(body.len() + 22);
        out.extend_from_slice(b"FLhd");
        out.extend_from_slice(&6u32.to_le_bytes());
        out.extend_from_slice(&self.format.to_le_bytes());
        out.extend_from_slice(&self.channel_count.to_le_bytes());
        out.extend_from_slice(&self.ppq.to_le_bytes());
        out.extend_from_slice(b"FLdt");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    // the position of the first event with an id
    pub fn find(&self, id: u8) -> Option<usize> {
        self.events.iter().position(|e| e.id == id)
    }

    // the fl studio version that saved the file, like 20.8.4.2576
    pub fn version(&self) -> String {
        match self.find(VERSION).map(|i| &self.events[i].value) {
            Some(Value::Data(data)) => String::from_utf8_lossy(data)
                .trim_end_matches('\0')
                .to_string(),
            _ => "unknown".to_string(),
        }
    }

    // the first number of the version, like 20
    pub fn major_version(&self) -> u32 {
        self.version()
            .split('.')
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
}

// reads an flp file from disk
pub fn read(path: &Path) -> Result<Project, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {}", path.display(), e))?;
    parse(&bytes)
}

// saves a project as an flp file
pub fn write(path: &Path, project: &Project) -> Result<(), String> {
    std::fs::write(path, project.to_bytes())
        .map_err(|e| format!("can't write {}: {}", path.display(), e))
}

// splits flp bytes into the header and a list of events
pub fn parse(bytes: &[u8]) -> Result<Project, String> {
    if bytes.len() < 22 || &bytes[0..4] != b"FLhd" || &bytes[14..18] != b"FLdt" {
        return Err("not an FL Studio project (missing FLhd/FLdt)".to_string());
    }
    let format = read_u16_le(bytes, 8)?;
    let channel_count = read_u16_le(bytes, 10)?;
    let ppq = read_u16_le(bytes, 12)?;
    let size = read_u32_le(bytes, 18)? as usize;
    let end = (22 + size).min(bytes.len());

    let mut events = Vec::new();
    let mut pos = 22;
    while pos < end {
        let id = bytes[pos];
        pos += 1;
        let value = if id < 64 {
            let v = *bytes.get(pos).ok_or("event cut off")?;
            pos += 1;
            Value::Byte(v)
        } else if id < 128 {
            let v = read_u16_le(bytes, pos)?;
            pos += 2;
            Value::Word(v)
        } else if id < 192 {
            let v = read_u32_le(bytes, pos)?;
            pos += 4;
            Value::Dword(v)
        } else {
            let len = read_varint(bytes, &mut pos)?;
            if pos + len > bytes.len() {
                return Err(format!("event {} at byte {} runs past the end", id, pos));
            }
            let data = bytes[pos..pos + len].to_vec();
            pos += len;
            Value::Data(data)
        };
        events.push(Event { id, value });
    }

    Ok(Project {
        format,
        channel_count,
        ppq,
        events,
    })
}

// reads a length stored in groups of 7 bits, smallest first, the top bit says another group follows
fn read_varint(bytes: &[u8], pos: &mut usize) -> Result<usize, String> {
    let mut value = 0usize;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*pos).ok_or("length cut off")?;
        *pos += 1;
        value |= ((byte & 0x7F) as usize) << shift;
        if byte < 0x80 {
            return Ok(value);
        }
        shift += 7;
        if shift > 28 {
            return Err("length is too long".to_string());
        }
    }
}

// writes a length in groups of 7 bits, smallest first
fn push_varint(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

// turns fl studio text into a string, utf-16 with a zero at the end, or one byte a letter in projects older than fl studio 12
pub fn decode_text(data: &[u8]) -> String {
    let pairs = data.len() / 2;
    let wide = data.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    if pairs > 0 && wide * 2 < pairs {
        return data
            .iter()
            .map(|&b| b as char)
            .collect::<String>()
            .trim_end_matches('\0')
            .to_string();
    }
    let mut units = Vec::with_capacity(data.len() / 2);
    for pair in data.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

// turns a string into fl studio text, utf-16 with a zero at the end
pub fn encode_text(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in text.encode_utf16().chain(std::iter::once(0)) {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // parsing a project and writing it again gives back the exact same bytes
    #[test]
    fn round_trip() {
        let project = Project {
            format: 0,
            channel_count: 1,
            ppq: 96,
            events: vec![
                Event {
                    id: VERSION,
                    value: Value::Data(b"20.8.4.2576\0".to_vec()),
                },
                Event {
                    id: 9,
                    value: Value::Byte(1),
                },
                Event {
                    id: CHANNEL_NEW,
                    value: Value::Word(0),
                },
                Event {
                    id: TEMPO,
                    value: Value::Dword(128_000),
                },
                Event {
                    id: CHANNEL_NAME,
                    value: Value::Data(vec![7; 300]),
                },
            ],
        };
        let bytes = project.to_bytes();
        let back = parse(&bytes).unwrap();
        assert_eq!(back.events, project.events);
        assert_eq!(back.to_bytes(), bytes);
        assert_eq!(back.version(), "20.8.4.2576");
        assert_eq!(back.major_version(), 20);
    }

    // a 300 byte length takes two groups, 0xac then 0x02
    #[test]
    fn varint_example() {
        let mut out = Vec::new();
        push_varint(&mut out, 300);
        assert_eq!(out, vec![0xAC, 0x02]);
        let mut pos = 0;
        assert_eq!(read_varint(&out, &mut pos).unwrap(), 300);
    }

    // names survive the trip through utf-16
    #[test]
    fn text_round_trip() {
        assert_eq!(decode_text(&encode_text("Kick")), "Kick");
    }
}
