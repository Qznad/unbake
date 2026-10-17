mod audio;

fn main() {
    // the first argument after the program name is the file to open
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: unbake <file.wav>");
            std::process::exit(1);
        }
    };

    match audio::wav::read_wav(&path) {
        Ok(wav) => {
            let info = &wav.info;
            println!("File:        {}", path);
            println!("Sample rate: {} Hz", info.sample_rate);
            println!("Channels:    {}", info.channels);
            println!("Bit depth:   {}-bit", info.bits_per_sample);
            println!("Frames:      {}", info.num_frames);
            println!("Duration:    {:.2} s", info.duration_seconds());

            let peak = audio::math::peak(&wav.samples);
            let mono = audio::math::to_mono(&wav.samples, info.channels);
            println!("Samples:     {}", wav.samples.len());
            println!("Peak:        {}", peak);
            println!("Peak:        {:.2} dBFS", audio::math::peak_dbfs(peak));
            println!("Mono:        {} samples", mono.len());
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}
