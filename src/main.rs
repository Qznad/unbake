mod audio;

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
}