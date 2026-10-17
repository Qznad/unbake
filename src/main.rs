mod audio;

fn main() {
    // The first argument after the program name is the file to open
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: unbake <file.wav>");
            std::process::exit(1);
        }
    };

    match audio::wav::read_wav(&path) {
        Ok(info) => {
            println!("File:        {}", path);
            println!("Sample rate: {} Hz", info.sample_rate);
            println!("Channels:    {}", info.channels);
            println!("Bit depth:   {}-bit", info.bits_per_sample);
            println!("Frames:      {}", info.num_frames);
            println!("Duration:    {:.2} s", info.duration_seconds());
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}
