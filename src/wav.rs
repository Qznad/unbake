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
        crate::audio::samples_to_seconds(self.num_frames, self.sample_rate)
    }
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

// reads 2 little endian bytes starting at offset as a u16.
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
