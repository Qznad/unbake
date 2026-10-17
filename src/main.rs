mod audio;

use audio::wav::Wav;

// runs the program and prints any error
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

// reads a wav and prints its info, with a second path it also saves a half volume copy
fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("missing input file, usage: unbake <in.wav> [out.wav]".to_string())?;

    let wav = audio::wav::read_wav(&path)?;
    print_info(&path, &wav);

    if let Some(out_path) = std::env::args().nth(2) {
        let quiet = Wav {
            info: wav.info.clone(),
            samples: audio::math::apply_gain(&wav.samples, 0.5),
        };
        audio::wav::write_wav(&out_path, &quiet)?;

        let copy = audio::wav::read_wav(&out_path)?;
        println!();
        print_info(&out_path, &copy);
    }

    Ok(())
}

// prints the details of a wav file
fn print_info(path: &str, wav: &Wav) {
    let info = &wav.info;
    let peak = audio::math::peak(&wav.samples);
    let mono = audio::math::to_mono(&wav.samples, info.channels);

    println!("File:        {}", path);
    println!("Sample rate: {} Hz", info.sample_rate);
    println!("Channels:    {}", info.channels);
    println!("Bit depth:   {}-bit", info.bits_per_sample);
    println!("Frames:      {}", info.num_frames);
    println!("Duration:    {:.2} s", info.duration_seconds());
    println!("Samples:     {}", wav.samples.len());
    println!("Peak:        {}", peak);
    println!("Peak:        {:.2} dBFS", audio::math::peak_dbfs(peak));
    println!("Mono:        {} samples", mono.len());
}
