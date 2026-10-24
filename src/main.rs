#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::chunks_exact_to_as_chunks
)]

mod analysis;
mod arrange;
mod audio;
mod basic_pitch;
mod check;
mod combine;
mod compare;
mod compose;
mod doctor;
mod eval;
mod evidence;
mod findings;
mod flp;
mod listen;
mod matching;
mod mcp;
mod midi;
mod music;
mod piano_roll;
mod rebuild;
mod report;
mod separate;
mod song_map;
mod sounds;
mod synth;
mod theory;
mod tidy;
mod tools;
mod views;

use audio::wav::Wav;
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "unbake",
    version = doctor::VERSION,
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
        about = "list the notes of a part or candidate in some bars: drums, bass, melody, chords, leftover or a candidate's name"
    )]
    Inspect {
        folder: PathBuf,
        name: String,
        #[arg(long, default_value_t = 1, help = "first bar, counting from 1")]
        from_bar: u32,
        #[arg(long, default_value_t = 4, help = "how many bars")]
        bars: u32,
    },

    #[command(
        about = "one line per bar of a transcribed folder: its section, kick, snare, hat, bass and other note counts, the chord heard, each stem's level and how high the drums reach"
    )]
    Overview { folder: PathBuf },

    #[command(
        about = "the song stretch by stretch: whether drums, bass, chords and lead repeat an earlier stretch, are new or silent, how clearly unbake hears them, and the work left"
    )]
    Timeline {
        folder: PathBuf,
        #[arg(
            long,
            default_value_t = 1,
            help = "the first bar of the chord cycle, counting from 1"
        )]
        from_bar: u32,
        #[arg(
            long,
            default_value_t = 4,
            help = "bars per row, the length of the chord cycle"
        )]
        bars: u32,
        #[arg(long, help = "song bars already approved, like 33-40")]
        approved: Vec<String>,
    },

    #[command(
        about = "compare every channel of an fl studio project with what the song plays in the same bars, and list where a part doesn't fit"
    )]
    Check {
        project: PathBuf,
        folder: PathBuf,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "the song bar where the project's bar 1 sits"
        )]
        from_bar: f64,
    },

    #[command(
        about = "the bars elsewhere in the song that sound most like a stretch, in chords and in drum rhythm, however quiet or filtered it is"
    )]
    SoundsLike {
        folder: PathBuf,
        #[arg(long, default_value_t = 1, help = "first bar, counting from 1")]
        from_bar: u32,
        #[arg(long, default_value_t = 4, help = "how many bars")]
        bars: u32,
    },

    #[command(
        about = "show a part as 16 steps per bar: drums as hit strings compose takes, pitched parts as the note on each step"
    )]
    Grid {
        folder: PathBuf,
        name: String,
        #[arg(long, default_value_t = 1, help = "first bar, counting from 1")]
        from_bar: u32,
        #[arg(long, default_value_t = 4, help = "how many bars")]
        bars: u32,
    },

    #[command(
        about = "the note names sounding in the other and bass stems for each bar or span of bars, with a chord name like Am9"
    )]
    Harmony {
        folder: PathBuf,
        #[arg(long, default_value_t = 1, help = "first bar, counting from 1")]
        from_bar: u32,
        #[arg(
            long,
            default_value_t = 9999,
            help = "how many bars, the rest of the song by default"
        )]
        bars: u32,
        #[arg(
            long,
            default_value_t = 0,
            help = "bars per chord, or 0 to start a new chord where the bass note changes"
        )]
        per: u32,
    },

    #[command(
        about = "cut a part into stretches of some bars, letter the ones that play the same, and show the notes each letter's repeats agree on"
    )]
    Parts {
        folder: PathBuf,
        name: String,
        #[arg(
            long,
            default_value_t = 1,
            help = "first bar of the first stretch, counting from 1"
        )]
        from_bar: u32,
        #[arg(
            long,
            default_value_t = 8,
            help = "bars per stretch, usually the song's chord cycle"
        )]
        bars: u32,
        #[arg(long, help = "last bar, the end of the song by default")]
        to_bar: Option<u32>,
        #[arg(long, help = "show the notes of this letter only")]
        letter: Option<String>,
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

    #[command(
        about = "list the version, tempo, channels with their sounds, patterns and mixer effects of an fl studio project"
    )]
    FlpInfo { file: PathBuf },

    #[command(
        about = "write an fl studio project from a loop description in json: tempo, channels, patterns and clips"
    )]
    Compose {
        spec: PathBuf,
        #[arg(
            short,
            long,
            help = "the folder to write the project into, by default next to the json"
        )]
        out: Option<PathBuf>,
        #[arg(long, help = "an fl studio project to use as the template")]
        template: Option<PathBuf>,
    },

    #[command(
        about = "list the notes of one pattern of an fl studio project, by channel, bar and step"
    )]
    Pattern { file: PathBuf, name: String },

    #[command(
        about = "list the playlist of an fl studio project track by track, each clip with its bar, length and repeats"
    )]
    Playlist { file: PathBuf },

    #[command(
        about = "list fl studio's own demo songs and genre templates to learn from, with their tempo, sounds and patterns"
    )]
    References {
        words: Vec<String>,
        #[arg(long, default_value_t = 0.0, help = "lowest tempo")]
        min_bpm: f64,
        #[arg(long, default_value_t = 999.0, help = "highest tempo")]
        max_bpm: f64,
        #[arg(
            long,
            help = "list every channel and pattern instead of the first few sounds"
        )]
        detail: bool,
    },

    #[command(
        about = "find samples in fl studio's own packs by words in their path, like kick or snare amen"
    )]
    Samples {
        #[arg(required = true)]
        words: Vec<String>,
        #[arg(long, default_value_t = 30, help = "how many to list at most")]
        limit: usize,
    },

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
        help = "write the chords as plain three-note triads, without the extra notes heard in them"
    )]
    simple_chords: bool,
    #[arg(
        long,
        help = "keep every melody note heard, without folding it into three octaves, dropping echoes and ghost notes, or cutting overlaps"
    )]
    raw_melody: bool,
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
        raw_melody: project.raw_melody,
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
// prints how each channel fits the song, stretch by stretch, with only the stretches that don't fit spelled out
fn print_fits(fits: &[check::ChannelFit]) {
    println!("Check against the song:");
    for fit in fits {
        println!(
            "  {:<14} {} stretches fit, compared with {}",
            fit.channel, fit.fitting, fit.against
        );
        for problem in &fit.problems {
            println!(
                "    song bar {:>3} (project bar {:>3})  {:>5.2}  {}",
                problem.song_bar, problem.project_bar, problem.fit, problem.verdict
            );
        }
    }
}

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
                    raw_melody: false,
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
                    raw_melody: false,
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
        Command::Overview { folder } => {
            println!(
                "  bar  section     kick snare  hat  bass other   drums dB  top kHz  bass dB  other dB  chord"
            );
            let show = |value: Option<i32>| value.map(|v| v.to_string()).unwrap_or("-".into());
            for line in views::overview(&folder)? {
                println!(
                    "  {:>3}  {:<10} {:>4} {:>5} {:>4} {:>5} {:>5}   {:>8}  {:>7}  {:>7}  {:>8}  {}",
                    line.bar,
                    line.section.as_deref().unwrap_or(""),
                    line.kick,
                    line.snare,
                    line.hat,
                    line.bass,
                    line.other,
                    show(line.drums_db),
                    line.drums_top_khz
                        .map(|v| format!("{:.1}", v))
                        .unwrap_or("-".into()),
                    show(line.bass_db),
                    show(line.other_db),
                    line.chord.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
        Command::SoundsLike {
            folder,
            from_bar,
            bars,
        } => {
            let like = listen::sounds_like(&folder, from_bar, bars)?;
            let level = |db: Option<i32>, top: Option<f64>, other: Option<i32>| {
                format!(
                    "drums {} dB up to {} kHz, other {} dB",
                    db.map(|v| v.to_string()).unwrap_or("-".into()),
                    top.map(|v| format!("{:.1}", v)).unwrap_or("-".into()),
                    other.map(|v| v.to_string()).unwrap_or("-".into())
                )
            };
            println!(
                "Bars {}-{}: {}",
                like.from_bar,
                like.from_bar + like.bars - 1,
                level(like.drums_db, like.drums_top_khz, like.other_db)
            );
            for (title, found) in [("Chords like", &like.chords), ("Drums like", &like.drums)] {
                println!("{}:{}", title, if found.is_empty() { " none" } else { "" });
                for m in found {
                    println!(
                        "  bar {:>3}  {:.2}  {:<10} {}",
                        m.bar,
                        m.likeness,
                        m.section.as_deref().unwrap_or(""),
                        level(m.drums_db, m.drums_top_khz, m.other_db)
                    );
                }
            }
            Ok(())
        }
        Command::Grid {
            folder,
            name,
            from_bar,
            bars,
        } => {
            for bar in views::grid(&folder, &name, from_bar, bars)? {
                for row in &bar.rows {
                    let cells = match (&row.hits, &row.notes) {
                        (Some(hits), _) => hits.clone(),
                        (_, Some(notes)) => notes
                            .iter()
                            .map(|n| format!("{:<4}", n))
                            .collect::<Vec<_>>()
                            .join(""),
                        _ => String::new(),
                    };
                    println!("  bar {:>3}  {:<6} {}", bar.bar, row.part, cells);
                }
            }
            Ok(())
        }
        Command::Parts {
            folder,
            name,
            from_bar,
            bars,
            to_bar,
            letter,
        } => {
            let mut parts = views::parts(&folder, &name, from_bar, bars, to_bar)?;
            if letter.is_some() {
                parts = parts.only_letter(letter.as_deref());
            }
            println!("  bars       part  notes  same  closest other");
            for part in &parts.parts {
                let closest = part
                    .closest_other
                    .as_ref()
                    .map(|(label, l)| format!("{} {:.2}", label, l))
                    .unwrap_or_default();
                println!(
                    "  {:>3}-{:<3}    {:<4} {:>5}  {:.2}  {}",
                    part.from_bar,
                    part.from_bar + part.bars - 1,
                    part.label,
                    part.notes,
                    part.same_as_first,
                    closest
                );
            }
            for group in &parts.groups {
                let bars: Vec<String> = group.bars.iter().map(|b| b.to_string()).collect();
                let source = match &group.variation_of {
                    Some(other) => format!("its own notes, gaps filled from {}", other),
                    None if group.bars.len() > 1 => "the notes its repeats agree on".to_string(),
                    None => "heard once, raw".to_string(),
                };
                println!(
                    "{} (bars {}): {} notes, {}",
                    group.label,
                    bars.join(", "),
                    group.notes.len(),
                    source
                );
                for bar in 1..=parts.bars_per_part {
                    let mut cells = vec![".".to_string(); 16];
                    for note in group.notes.iter().filter(|n| n.bar == bar) {
                        cells[(note.step as usize - 1).min(15)] = note.note.clone();
                    }
                    if cells.iter().any(|c| c != ".") {
                        let row: Vec<String> = cells.iter().map(|c| format!("{:<4}", c)).collect();
                        println!("    bar {} {}", bar, row.join(""));
                    }
                }
            }
            Ok(())
        }
        Command::Harmony {
            folder,
            from_bar,
            bars,
            per,
        } => {
            for span in views::harmony(&folder, from_bar, bars, per)? {
                let tones: Vec<String> = span
                    .tones
                    .iter()
                    .map(|t| format!("{} {}%", t.note, t.percent))
                    .collect();
                println!(
                    "  bars {:>3}-{:<3}  bass {:<3} {:<9} {}",
                    span.from_bar,
                    span.from_bar + span.bars - 1,
                    span.bass.as_deref().unwrap_or("-"),
                    span.chord.as_deref().unwrap_or(""),
                    tones.join(", ")
                );
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
            if let Some(title) = &summary.title {
                println!("Title:       {}", title);
            }
            println!("PPQ:         {}", summary.ppq);
            match summary.tempo {
                Some(bpm) => println!("Tempo:       {} bpm", bpm),
                None => println!("Tempo:       not set"),
            }
            println!("Events:      {}", project.events.len());
            println!("Channels:    {}", summary.channels.len());
            for channel in &summary.channels {
                let sound = match (&channel.plugin, &channel.sample) {
                    _ if channel.automation => "automation",
                    (_, Some(sample)) => sample.rsplit(['\\', '/']).next().unwrap_or(sample),
                    (Some(plugin), None) => plugin.as_str(),
                    (None, None) => "",
                };
                let insert = channel
                    .insert
                    .map(|i| format!("  -> insert {}", i))
                    .unwrap_or_default();
                let volume = match channel.volume {
                    Some(0) => "  volume 0%, probably faded in by automation".to_string(),
                    Some(v) => format!("  volume {}%", v),
                    None => String::new(),
                };
                println!(
                    "  {:>3}  {:<32} {}{}{}",
                    channel.id, channel.name, sound, insert, volume
                );
            }
            println!("Patterns:    {}", summary.patterns.len());
            for pattern in &summary.patterns {
                println!(
                    "  {:>3}  {} ({} notes)",
                    pattern.id, pattern.name, pattern.notes
                );
            }
            println!("Playlist:    {} clips", summary.playlist_items);
            let used: Vec<_> = summary
                .inserts
                .iter()
                .filter(|i| !i.effects.is_empty())
                .collect();
            println!("Mixer:       {} inserts with effects", used.len());
            for insert in used {
                println!(
                    "  {:>3}  {:<24} {}",
                    insert.index,
                    insert.name.as_deref().unwrap_or(""),
                    insert.effects.join(" > ")
                );
            }
            Ok(())
        }
        Command::Compose {
            spec,
            out,
            template,
        } => {
            let text = std::fs::read_to_string(&spec)
                .map_err(|e| format!("can't read {}: {}", spec.display(), e))?;
            let description: compose::Loop = serde_json::from_str(&text)
                .map_err(|e| format!("{} is not a loop description: {}", spec.display(), e))?;
            let folder =
                out.unwrap_or_else(|| spec.parent().map(|p| p.to_path_buf()).unwrap_or_default());
            let written = compose::write(&description, &folder, template.as_deref())?;
            println!("Project:     {}", written.path);
            println!(
                "Wrote:       {} channels, {} patterns, {} clips",
                written.channels, written.patterns, written.clips
            );
            for warning in &written.warnings {
                println!("Warning:     {}", warning);
            }
            if let Some(fits) = &written.check {
                print_fits(fits);
            }
            Ok(())
        }
        Command::Timeline {
            folder,
            from_bar,
            bars,
            approved,
        } => {
            let approved = song_map::parse_approved(&approved)?;
            for row in song_map::timeline(&folder, from_bar, bars, &approved)?.rows {
                println!(
                    "bars {}-{}{}  {}  -> {}",
                    row.from_bar,
                    row.from_bar + row.bars - 1,
                    row.section.map(|s| format!(" ({})", s)).unwrap_or_default(),
                    row.sound,
                    row.work
                );
                println!("    drums:  {}", row.drums);
                println!("    bass:   {}", row.bass);
                println!("    chords: {}", row.chords);
                println!("    lead:   {}", row.lead);
                if let Some(like) = row.sounds_like {
                    println!("    sounds like: {}", like);
                }
            }
            Ok(())
        }
        Command::Check {
            project,
            folder,
            from_bar,
        } => {
            let project = flp::events::read(&compose::local_path(&project.display().to_string()))?;
            print_fits(&check::check(&project, &folder, from_bar)?);
            Ok(())
        }
        Command::Pattern { file, name } => {
            let project = flp::events::read(&file)?;
            let notes = compose::pattern_view(&project, &name)?;
            println!("{}: {} notes", name, notes.len());
            for note in notes {
                println!(
                    "  bar {:>3} step {:>5.2}  {:<5} {:>5.2} steps  velocity {:>3}  {}",
                    note.bar, note.step, note.note, note.steps, note.velocity, note.channel
                );
            }
            Ok(())
        }
        Command::Playlist { file } => {
            let project = flp::events::read(&compose::local_path(&file.display().to_string()))?;
            let playlist = combine::playlist(&project);
            println!("{} bars", playlist.bars);
            for track in playlist.tracks {
                println!(
                    "track {:>3}  {}{}",
                    track.track,
                    track.name.as_deref().unwrap_or(""),
                    if track.muted { "  (muted)" } else { "" }
                );
                for clip in track.clips {
                    let what = clip.pattern.or(clip.audio).unwrap_or_default();
                    let from = clip
                        .from_bar
                        .map(|bar| format!(" from its bar {}", bar))
                        .unwrap_or_default();
                    println!(
                        "    bar {:>6}  {:<28} {} bars x {}{}",
                        clip.bar, what, clip.bars, clip.times, from
                    );
                }
            }
            Ok(())
        }
        Command::References {
            words,
            min_bpm,
            max_bpm,
            detail,
        } => {
            for reference in compose::references(&words.join(" "), min_bpm, max_bpm)? {
                let reference = if detail {
                    reference
                } else {
                    reference.summary()
                };
                println!(
                    "{:>6}  {:<10} {}",
                    reference
                        .tempo
                        .map(|t| format!("{:.0}", t))
                        .unwrap_or_default(),
                    reference.kind,
                    reference.name
                );
                println!("        {}", reference.path);
                let more = reference.channel_count - reference.channels.len();
                let more = if more > 0 {
                    format!(" and {} more", more)
                } else {
                    String::new()
                };
                println!(
                    "        {} channels, {} patterns: {}{}",
                    reference.channel_count,
                    reference.pattern_count,
                    reference.channels.join(", "),
                    more
                );
                if !reference.patterns.is_empty() {
                    println!("        patterns: {}", reference.patterns.join(", "));
                }
            }
            Ok(())
        }
        Command::Samples { words, limit } => {
            for sample in compose::find_samples(&words.join(" "), limit)? {
                println!("{}", sample);
            }
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
