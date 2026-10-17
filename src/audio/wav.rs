#[derive(Debug, Clone)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub num_frames: u32,
}

impl WavInfo {
    // how long the audio lasts in seconds
    pub fn duration_seconds(&self) -> f64 {
        crate::audio::math::samples_to_seconds(self.num_frames, self.sample_rate)
    }
}

pub struct Wav {
    pub info: WavInfo,
    pub samples: Vec<i16>,
}

impl Wav {
    // turns the wav into file bytes, a 44 byte header followed by the samples
    pub fn to_bytes(&self) -> Vec<u8> {
        let channels = self.info.channels;
        let sample_rate = self.info.sample_rate;
        let data_size = (self.samples.len() * 2) as u32;

        let mut out = Vec::with_capacity(44 + data_size as usize);

        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_size).to_le_bytes());
        out.extend_from_slice(b"WAVE");

        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());

        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_size.to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&s.to_le_bytes());
        }

        out
    }
}

// saves a wav file to disk
pub fn write_wav(path: &str, wav: &Wav) -> Result<(), String> {
    std::fs::write(path, wav.to_bytes()).map_err(|e| format!("can't write {}: {}", path, e))
}

// reads 4 bytes at offset as a little endian u32
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

// reads 2 bytes at offset as a little endian u16
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

// loads a wav file from disk
pub fn read_wav(path: &str) -> Result<Wav, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {}", path, e))?;
    parse_wav(&bytes)
}

// turns wav file bytes into header info and samples
pub fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file (missing RIFF/WAVE header)".to_string());
    }

    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut bits_per_sample: Option<u16> = None;
    let mut data: Option<(usize, usize)> = None;

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
            data = Some((content, size.min(bytes.len() - content)));
        }

        offset = content + size + size % 2;
    }

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

    let data_bytes = &bytes[data_start..data_start + num_frames * bytes_per_frame];

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
