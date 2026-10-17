#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::chunks_exact_to_as_chunks
)]

mod analysis;
mod arrange;
mod audio;
mod compare;
mod flp;
mod matching;
mod mcp;
mod midi;
mod music;
mod rebuild;
mod report;
mod separate;
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
        #[arg(
            short,
            long,
            help = "folder for the stems [default: <song>_rebuild/stems]"
        )]
        out: Option<PathBuf>,
    },

    #[command(
        about = "rebuild a song as stems, midi, an fl studio project, a preview and a report"
    )]
    Rebuild {
        file: PathBuf,
        #[arg(short, long, help = "output folder [default: <song>_rebuild]")]
        out: Option<PathBuf>,
        #[arg(long, help = "use the stems in this folder instead of running demucs")]
        stems: Option<PathBuf>,
        #[arg(
            long,
            help = "an FL Studio project with channels named Kick, Snare, Hat, Bass, Lead and Pad"
        )]
        template: Option<PathBuf>,
        #[arg(long, help = "skip tuning the synth sounds to the stems, faster")]
        no_match: bool,
    },

    #[command(about = "list the version, tempo, channels and patterns of an fl studio project")]
    FlpInfo { file: PathBuf },

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
                    match_sounds: false,
                })
                .join("stems")
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
            no_match,
        } => {
            let options = rebuild::Options {
                input: file,
                out,
                stems,
                template,
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
