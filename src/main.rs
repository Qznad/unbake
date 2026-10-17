mod audio;
mod wav;

fn main() {
    println!(
        "132300 samples at 44100 Hz = {} s",
        audio::samples_to_seconds(132300, 44100)
    );
    println!(
        "2.5 s at 48000 Hz = {} samples",
        audio::seconds_to_samples(2.5, 48000)
    );
    println!("120 BPM = {} ms per beat", audio::bpm_to_ms_per_beat(120.0));
    println!("128 BPM = {:.2} ms per step", audio::ms_per_step(128.0));

    let samples: Vec<i16> = vec![0, 1200, -3000, 500, -32768, 20000, -150, 11000];
    println!("Peak: {}", audio::peak(&samples));
    println!("As floats: {:?}", audio::to_float(&samples));

    // a wav header built by hand 3 seconds of CD-quality stereo
    let info = wav::WavInfo {
        sample_rate: 44100,
        channels: 2,
        bits_per_sample: 16,
        num_frames: 132300,
    };
    println!("{:?}", info);
    println!("Duration: {} s", info.duration_seconds());
    println!(
        "{} channels, {}-bit, {} Hz",
        info.channels, info.bits_per_sample, info.sample_rate
    );

    // 44100 stored as little endian bytes
    let bytes: Vec<u8> = vec![0x44, 0xAC, 0x00, 0x00];

    match wav::read_u32_le(&bytes, 0) {
        Ok(n) => println!("u32 at 0: {}", n),
        Err(e) => println!("Error: {}", e),
    }
    match wav::read_u32_le(&bytes, 2) {
        Ok(n) => println!("u32 at 2: {}", n),
        Err(e) => println!("Error: {}", e),
    }
    match wav::read_u16_le(&bytes, 0) {
        Ok(n) => println!("u16 at 0: {}", n),
        Err(e) => println!("Error: {}", e),
    }
}
