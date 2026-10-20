#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::chunks_exact_to_as_chunks
)]

mod analysis;
mod arrange;
mod audio;
mod basic_pitch;
mod compare;
mod doctor;
mod eval;
mod evidence;
mod findings;
mod flp;
mod matching;
mod mcp;
mod midi;
mod music;
mod piano_roll;
mod rebuild;
mod report;
mod separate;
mod sounds;
mod synth;
mod theory;
mod tools;

use audio::wav::Wav;
use clap::{Args, Parser, Subcommand};
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
        #[command(flatten)]
        song: SongArgs,
        #[command(flatten)]
        project: ProjectArgs,
    },

    #[command(
        about = "listen to a song and save what it hears in the rebuild folder, with melody candidates to choose from"
    )]
    Transcribe {
        file: PathBuf,
        #[command(flatten)]
        song: SongArgs,
        #[arg(
            long,
            help = "a folder with your own drum sounds, used to find the hits: kick, snare, hat"
        )]
        sounds: Option<PathBuf>,
    },

    #[command(about = "list the melody candidates of a transcribed rebuild folder")]
    Candidates { folder: PathBuf },

    #[command(about = "choose which melody candidate the project uses")]
    Choose { folder: PathBuf, candidate: String },

    #[command(
        about = "list the notes of a part or candidate in some bars: drums, bass, melody, chords or a candidate's name"
    )]
    Inspect {
        folder: PathBuf,
        name: String,
        #[arg(long, default_value_t = 1, help = "first bar, counting from 1")]
        from_bar: u32,
        #[arg(long, default_value_t = 4, help = "how many bars")]
        bars: u32,
    },

    #[command(about = "draw a piano roll png of up to four parts or candidates over each other")]
    PianoRoll {
        folder: PathBuf,
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(long, default_value_t = 1, help = "first bar, counting from 1")]
        from_bar: u32,
        #[arg(long, default_value_t = 8, help = "how many bars")]
        bars: u32,
    },

    #[command(about = "write the fl studio project of a transcribed rebuild folder")]
    Write {
        folder: PathBuf,
        #[command(flatten)]
        project: ProjectArgs,
    },

    #[command(about = "list the version, tempo, channels and patterns of an fl studio project")]
    FlpInfo { file: PathBuf },

    #[command(
        about = "score a rebuild against the song's real notes, its fl studio project or a slakh track folder"
    )]
    Eval { project: PathBuf, rebuild: PathBuf },

    #[command(about = "run as an mcp server so ai agents can use unbake, over stdin and stdout")]
    Mcp,

    #[command(
        about = "check that ffmpeg, demucs, basic-pitch and the gpu are found, and say how to fix what isn't"
    )]
    Doctor {
        #[arg(long, help = "print the checks as json")]
        json: bool,
    },
}

#[derive(Args)]
struct SongArgs {
    #[arg(
        short,
        long,
        help = "output folder, also the fl studio project folder [default: <song>]"
    )]
    out: Option<PathBuf>,
    #[arg(long, help = "use the stems in this folder instead of running demucs")]
    stems: Option<PathBuf>,
}

#[derive(Args)]
struct ProjectArgs {
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
    #[arg(
        long,
        help = "write the chords as clean triads that follow the key, easier to edit but less exact"
    )]
    simple_chords: bool,
    #[arg(
        long,
        help = "arrange each part as up to 4 reused 4 bar loops instead of one pattern copying the whole song"
    )]
    loops: bool,
}

// the rebuild options for a song from the command line arguments
fn options(file: PathBuf, song: SongArgs, project: ProjectArgs) -> rebuild::Options {
    rebuild::Options {
        input: file,
        out: song.out,
        stems: song.stems,
        template: project.template,
        sounds: project.sounds,
        fl_folder: project.fl_folder,
        match_sounds: !project.no_match,
        simple_chords: project.simple_chords,
        loops: project.loops,
    }
}

// prints the melody candidates with their evidence, marking the chosen one
fn print_candidates(candidates: &[tools::CandidateView]) {
    println!("Melody candidates:");
    for c in candidates {
        let mark = if c.chosen { "*" } else { " " };
        println!("{} {:<9} {}", mark, c.name, c.description);
        for line in c.finding.lines() {
            println!("            {}", line);
        }
    }
}

// prints what a rebuild made: tempo, key, parts, sounds, project and how sure each finding is
fn print_report(report: &report::Report, folder: &Path) {
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
    print_findings(&report.findings);
    println!("Report:      {}", folder.join("report.html").display());
}

// prints how sure each finding is, with what else fits, what is unknown and what the method can't do
fn print_findings(findings: &[evidence::Finding]) {
    println!("How sure:");
    for finding in findings {
        for line in finding.lines() {
            println!("  {}", line);
        }
    }
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
            print_findings(&info.findings);
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
                    simple_chords: false,
                    loops: false,
                })
            });
            separate::split(&file, &dir)?;
            println!("Stems saved in {}", dir.display());
            Ok(())
        }
        Command::Rebuild {
            file,
            song,
            project,
        } => {
            let options = options(file, song, project);
            let report = rebuild::run(&options)?;
            print_report(&report, &rebuild::output_dir(&options));
            Ok(())
        }
        Command::Transcribe { file, song, sounds } => {
            let mut options = options(
                file,
                song,
                ProjectArgs {
                    template: None,
                    sounds,
                    fl_folder: None,
                    no_match: true,
                    simple_chords: false,
                    loops: false,
                },
            );
            let folder = rebuild::output_dir(&options);
            options.out = Some(folder.clone());
            let session = rebuild::transcribe(&options)?;
            println!(
                "Saved:       {}",
                folder.join("extras/session.json").display()
            );
            print_findings(&session.findings);
            print_candidates(&tools::candidates(&folder)?);
            Ok(())
        }
        Command::Candidates { folder } => {
            print_candidates(&tools::candidates(&folder)?);
            Ok(())
        }
        Command::Choose { folder, candidate } => {
            rebuild::choose_melody(&folder, &candidate)?;
            println!(
                "The melody is now {}, run write to make the project",
                candidate
            );
            Ok(())
        }
        Command::Inspect {
            folder,
            name,
            from_bar,
            bars,
        } => {
            let view = tools::inspect(&folder, &name, from_bar, bars)?;
            println!(
                "{}: {} notes in bars {} to {}, {} in the song",
                view.name,
                view.notes.len(),
                view.from_bar,
                view.from_bar + view.bars - 1,
                view.notes_in_song
            );
            for n in &view.notes {
                println!(
                    "  bar {:>3} step {:>2}  {:<4} {:>2} steps  velocity {}",
                    n.bar, n.step, n.note, n.steps, n.velocity
                );
            }
            if let Some(finding) = &view.finding {
                print_findings(std::slice::from_ref(finding));
            }
            Ok(())
        }
        Command::PianoRoll {
            folder,
            names,
            from_bar,
            bars,
        } => {
            let roll = tools::piano_roll(&folder, &names, from_bar, bars)?;
            println!("Saved:       {}", roll.path);
            println!("Colours:     {}", roll.legend.join(", "));
            println!("Scale:       {}", roll.scale);
            Ok(())
        }
        Command::Write { folder, project } => {
            let options = options(
                folder.clone(),
                SongArgs {
                    out: Some(folder.clone()),
                    stems: None,
                },
                project,
            );
            let report = tools::write(&folder, options)?;
            print_report(&report, &folder);
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
            rows.push(result.melody.clone());
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
            println!("Melody:      compared with \"{}\"", result.melody_channel);
            println!("Chords:      {:.2} similarity per bar", result.chords);
            println!("Harmony:     {:.2} similarity per bar", result.harmony);
            let found = result
                .patterns_found
                .map_or("no .flp in the rebuild".to_string(), |n| {
                    format!("{} used", n)
                });
            match result.patterns_truth {
                Some(n) => println!("Patterns:    {}, the real project uses {}", found, n),
                None => println!("Patterns:    {}", found),
            }
            for note in &result.notes {
                println!("Note:        {}", note);
            }
            Ok(())
        }
        Command::Mcp => mcp::serve(),
        Command::Doctor { json } => {
            let checks = doctor::run();
            if json {
                let text = serde_json::to_string_pretty(&checks).map_err(|e| e.to_string())?;
                println!("{}", text);
                return Ok(());
            }
            for c in &checks {
                let status = match c.status {
                    doctor::Status::Ok => "ok",
                    doctor::Status::Warning => "warning",
                    doctor::Status::Missing => "missing",
                };
                println!("{:<12} {:<8} {}", c.name, status, c.detail);
                if c.status != doctor::Status::Ok {
                    println!("{:<21} needed for {}", "", c.needed_for);
                }
                if let Some(fix) = &c.fix {
                    println!("{:<21} fix: {}", "", fix);
                }
            }
            Ok(())
        }
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
