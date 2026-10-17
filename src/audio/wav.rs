// reading wav files
// groups the four header numbers into one value
#[derive(Debug)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub num_frames: u32,
}

impl WavInfo {
    pub fn duration_seconds(&self) -> f64 {
        crate::audio::math::samples_to_seconds(self.num_frames, self.sample_rate)
    }
}

// a whole wav file: header info plus every sample, interleaved as left right left right
pub struct Wav {
    pub info: WavInfo,
    pub samples: Vec<i16>,
}

// reads 4 little endian bytes starting at offset as a u32
pub fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    if offset + 4 > bytes.len() {
        return Err(format!(
            "need 4 bytes at offset {}, but there are only {} bytes",
            offset,
            bytes.len()
        ));
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

// reads 2 little endian bytes starting at offset as a u16
pub fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, String> {
    if offset + 2 > bytes.len() {
        return Err(format!(
            "need 2 bytes at offset {}, but there are only {} bytes",
            offset,
            bytes.len()
        ));
    }
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

// reads a wav file from disk and returns its header info and samples
pub fn read_wav(path: &str) -> Result<Wav, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {}", path, e))?;
    parse_wav(&bytes)
}

// walks through the chunks of a wav file and pulls out the header info
// layout: "RIFF" [size] "WAVE", then chunks of [4-byte id] [u32 size] [content]
pub fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file (missing RIFF/WAVE header)".to_string());
    }

    // filled in when we find the chunks; none means "not found yet"
    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut bits_per_sample: Option<u16> = None;
    let mut data: Option<(usize, usize)> = None; // (where the samples start, how many bytes)

    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = read_u32_le(bytes, offset + 4)? as usize;
        let content = offset + 8;

        if id == b"fmt " {
            if size < 16 {
                return Err(format!("fmt chunk is too short ({} bytes)", size));
            }
            let format = read_u16_le(bytes, content)?;
            if format != 1 {
                return Err(format!(
                    "unsupported format {} (only PCM is supported)",
                    format
                ));
            }
            channels = Some(read_u16_le(bytes, content + 2)?);
            sample_rate = Some(read_u32_le(bytes, content + 4)?);
            bits_per_sample = Some(read_u16_le(bytes, content + 14)?);
        } else if id == b"data" {
            // some files claim more data than they contain, so cap it at what's left
            data = Some((content, size.min(bytes.len() - content)));
        }

        // chunks with an odd size are followed by one padding byte
        offset = content + size + size % 2;
    }

    // turn each option into a result, so ? can return the error
    let channels = channels.ok_or("missing fmt chunk".to_string())?;
    let sample_rate = sample_rate.ok_or("missing fmt chunk".to_string())?;
    let bits_per_sample = bits_per_sample.ok_or("missing fmt chunk".to_string())?;
    let (data_start, data_size) = data.ok_or("missing data chunk".to_string())?;

    if bits_per_sample != 16 {
        return Err(format!(
            "{}-bit audio is not supported (only 16-bit)",
            bits_per_sample
        ));
    }
    if channels == 0 {
        return Err("file says it has 0 channels".to_string());
    }
    if sample_rate == 0 {
        return Err("file says its sample rate is 0".to_string());
    }

    let bytes_per_frame = channels as usize * (bits_per_sample as usize / 8);
    let num_frames = data_size / bytes_per_frame;

    // only whole frames: a cut-off frame at the end is ignored
    let data_bytes = &bytes[data_start..data_start + num_frames * bytes_per_frame];

    // every 2 bytes is one little-endian i16 sample
    let mut samples = Vec::with_capacity(data_bytes.len() / 2);
    for pair in data_bytes.chunks_exact(2) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }

    Ok(Wav {
        info: WavInfo {
            sample_rate,
            channels,
            bits_per_sample,
            num_frames: num_frames as u32,
        },
        samples,
    })
}
