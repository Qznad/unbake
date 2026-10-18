#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::chunks_exact_to_as_chunks
)]

mod analysis;
mod arrange;
mod audio;
mod compare;
mod eval;
mod flp;
mod matching;
mod mcp;
mod midi;
mod music;
mod rebuild;
mod report;
mod separate;
mod sounds;
mod synth;

use audio::wav::Wav;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "unbake",
    version,
    about = "turn a song into an editable fl studio project"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "show the format and level of an audio file")]
    Info { file: PathBuf },

    #[command(about = "detect the tempo, key and first bar of a song")]
    Analyze { file: PathBuf },

    #[command(about = "separate a song into drums, bass, other and vocals with demucs")]
    Split {
        file: PathBuf,
        #[arg(short, long, help = "folder for the stems [default: <song>]")]
        out: Option<PathBuf>,
    },

    #[command(
        about = "rebuild a song as stems, midi, an fl studio project, a preview and a report"
    )]
    Rebuild {
        file: PathBuf,
        #[arg(
            short,
            long,
            help = "output folder, also the fl studio project folder [default: <song>]"
        )]
        out: Option<PathBuf>,
        #[arg(long, help = "use the stems in this folder instead of running demucs")]
        stems: Option<PathBuf>,
        #[arg(
            long,
            help = "an FL Studio project with channels named Kick, Snare, Hat, Bass, Lead and Pad"
        )]
        template: Option<PathBuf>,
        #[arg(
            long,
            help = "a folder with your own sounds: kick, snare, hat, bass, lead, pad (.wav, .mp3, ...)"
        )]
        sounds: Option<PathBuf>,
        #[arg(
            long,
            help = "the windows folder you will copy the output folder to [default: %FLStudioUserData%\\Projects\\unbake\\<output folder name>]"
        )]
        fl_folder: Option<String>,
        #[arg(long, help = "skip tuning the synth sounds to the stems, faster")]
        no_match: bool,
    },

    #[command(about = "list the version, tempo, channels and patterns of an fl studio project")]
    FlpInfo { file: PathBuf },

    #[command(about = "score a rebuild against the fl studio project the song was made from")]
    Eval { project: PathBuf, rebuild: PathBuf },

    #[command(about = "run as an mcp server so ai agents can use unbake, over stdin and stdout")]
    Mcp,
}

// runs the program and prints any error
fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli.command) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

// does what the chosen command asks for
fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Info { file } => {
            let wav = load(&file)?;
            print_info(&file, &wav);
            Ok(())
        }
        Command::Analyze { file } => {
            let info = rebuild::analyze_song(&file)?;
            println!("Tempo:       {} bpm", info.tempo_bpm);
            println!(
                "Key:         {} (same notes as {})",
                info.key, info.relative_key
            );
            println!("First bar:   {:.3} s", info.first_bar_seconds);
            println!("Bars:        {}", info.bars);
            Ok(())
        }
        Command::Split { file, out } => {
            let dir = out.unwrap_or_else(|| {
                rebuild::output_dir(&rebuild::Options {
                    input: file.clone(),
                    out: None,
                    stems: None,
                    template: None,
                    sounds: None,
                    fl_folder: None,
                    match_sounds: false,
                })
            });
            separate::split(&file, &dir)?;
            println!("Stems saved in {}", dir.display());
            Ok(())
        }
        Command::Rebuild {
            file,
            out,
            stems,
            template,
            sounds,
            fl_folder,
            no_match,
        } => {
            let options = rebuild::Options {
                input: file,
                out,
                stems,
                template,
                sounds,
                fl_folder,
                match_sounds: !no_match,
            };
            let report = rebuild::run(&options)?;
            println!("Tempo:       {} bpm", report.tempo_bpm);
            println!(
                "Key:         {} (same notes as {})",
                report.key, report.relative_key
            );
            println!("Bars:        {}", report.bars);
            let sections: Vec<String> = report
                .sections
                .iter()
                .map(|s| format!("{} ({} bars)", s.name, s.bars))
                .collect();
            println!("Sections:    {}", sections.join(", "));
            println!("Chords:      {}", report.chords.join(" "));
            for part in &report.parts {
                let scores: Vec<String> = part
                    .scores
                    .iter()
                    .map(|s| format!("{} {:.2}", s.dimension, s.value))
                    .collect();
                println!(
                    "{:<12} {} notes ({}), {}",
                    format!("{}:", part.part),
                    part.notes,
                    part.detail,
                    scores.join(", ")
                );
            }
            for m in &report.sound_matching {
                println!(
                    "Sounds:      {} stem similarity {:.2} -> {:.2} after {} renders",
                    m.stem, m.score_before, m.score_after, m.renders
                );
            }
            for sound in &report.sounds {
                println!("Sound:       {}", sound);
            }
            println!("FL project:  {}", report.flp);
            for warning in &report.warnings {
                println!("Warning:     {}", warning);
            }
            println!(
                "Report:      {}",
                rebuild::output_dir(&options).join("report.html").display()
            );
            Ok(())
        }
        Command::FlpInfo { file } => {
            let project = flp::events::read(&file)?;
            let summary = flp::info::summarize(&project);
            println!("Version:     {}", summary.version);
            println!("PPQ:         {}", summary.ppq);
            match summary.tempo {
                Some(bpm) => println!("Tempo:       {} bpm", bpm),
                None => println!("Tempo:       not set"),
            }
            println!("Events:      {}", project.events.len());
            println!("Channels:    {}", summary.channels.len());
            for channel in &summary.channels {
                println!("  {:>3}  {}", channel.id, channel.name);
            }
            println!("Patterns:    {}", summary.patterns.len());
            for pattern in &summary.patterns {
                println!(
                    "  {:>3}  {} ({} notes)",
                    pattern.id, pattern.name, pattern.notes
                );
            }
            println!("Playlist:    {} clips", summary.playlist_items);
            Ok(())
        }
        Command::Eval { project, rebuild } => {
            let result = eval::evaluate(&project, &rebuild)?;
            println!(
                "Tempo:       {} bpm found, {} bpm real",
                result.tempo_found, result.tempo_truth
            );
            println!("             real   found  matched  precision  recall     f");
            let mut rows = result.drums.clone();
            rows.push(result.bass.clone());
            for s in &rows {
                println!(
                    "{:<10} {:>6}  {:>6}  {:>7}  {:>9.2}  {:>6.2}  {:>4.2}",
                    s.name, s.truth, s.found, s.matched, s.precision, s.recall, s.f
                );
            }
            println!(
                "Bass:        compared with \"{}\", found {} octaves off",
                result.bass_channel, result.bass_octave_shift
            );
            println!("Harmony:     {:.2} similarity per bar", result.harmony);
            match result.patterns_found {
                Some(n) => println!(
                    "Patterns:    {} used, the real project uses {}",
                    n, result.patterns_truth
                ),
                None => println!(
                    "Patterns:    no .flp in the rebuild, the real project uses {}",
                    result.patterns_truth
                ),
            }
            for note in &result.notes {
                println!("Note:        {}", note);
            }
            Ok(())
        }
        Command::Mcp => mcp::serve(),
    }
}

// opens any audio file, converting it with ffmpeg in a temporary folder when needed
fn load(file: &Path) -> Result<Wav, String> {
    audio::convert::load(file, &std::env::temp_dir().join("unbake"))
}

// prints the details of a wav file
fn print_info(path: &Path, wav: &Wav) {
    let info = &wav.info;
    let peak = audio::math::peak(&wav.samples);

    println!("File:        {}", path.display());
    println!("Sample rate: {} Hz", info.sample_rate);
    println!("Channels:    {}", info.channels);
    println!("Bit depth:   {}-bit", info.bits_per_sample);
    println!("Duration:    {:.2} s", info.duration_seconds());
    println!("Peak:        {:.2} dBFS", audio::math::peak_dbfs(peak));
}
