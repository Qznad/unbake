fn main() {
    println!(
        "132300 samples at 44100 Hz = {} s",
        samples_to_seconds(132300, 44100)
    );
    println!(
        "2.0 s at 48000 Hz = {} samples",
        seconds_to_samples(2.0, 48000)
    );
    println!(
        "2.5 s at 48000 Hz = {} samples",
        seconds_to_samples(2.5, 48000)
    );
    println!("120 BPM = {} ms per beat", bpm_to_ms_per_beat(120.0));
    println!("128 BPM = {:.2} ms per step", ms_per_step(128.0));

    let sample_rate = 44100;
    let bpm = 120.0;
    let ms_per_beat = bpm_to_ms_per_beat(bpm);
    let seconds_per_beat = ms_per_beat / 1000.0;
    let samples_per_beat = seconds_to_samples(seconds_per_beat, sample_rate);
    println!("Samples per beat: {}", samples_per_beat);

    let samples: Vec<i16> = vec![0, 1200, -3000, 500, -32768, 20000, -150, 11000];
    println!("Peak: {}", peak(&samples));
    println!("Loud samples: {}", count_loud(&samples, 10000));
    println!("As floats: {:?}", to_float(&samples));
}

fn samples_to_seconds(samples: u32, sample_rate: u32) -> f64 {
    samples as f64 / sample_rate as f64
}

fn seconds_to_samples(seconds: f64, sample_rate: u32) -> u32 {
    (seconds * sample_rate as f64).round() as u32
}

fn bpm_to_ms_per_beat(bpm: f64) -> f64 {
    60_000.0 / bpm
}

fn ms_per_step(bpm: f64) -> f64 {
    bpm_to_ms_per_beat(bpm) / 4.0
}


fn peak(samples: &[i16]) -> i32 {
    samples.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0)
}


fn count_loud(samples: &[i16], threshold: i32) -> usize {
    samples
        .iter()
        .filter(|&&s| (s as i32).abs() > threshold)
        .count()
}


fn to_float(samples: &[i16]) -> Vec<f32> {
    let mut float_samples = Vec::new();
    for &n in samples {
        float_samples.push(n as f32 / 32768.0);
    }
    float_samples
}