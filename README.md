# unbake

Turn a song into an FL Studio project you can learn from.

Give unbake an mp3 or wav. It splits the song into stems and listens for the tempo, key, sections, chords, drum groove, bass line and melody. Then, with an AI agent such as Claude Code, it builds an FL Studio project from what it heard: first a loop of the song's main section, then the whole song, one section at a time. The sounds and effects are copied from real producers' projects that come with FL Studio. The song's own stems sit underneath as muted audio clips, so you can solo them and compare part by part.

The result is not a copy of the song. It plays the song's chords, bass and groove in a clean, editable project, and it tells you what came from the song and what didn't. Your ears decide whether it's close.

> **Status:** early. The FL Studio side runs on Windows only, and it has been tested on electronic instrumentals. It does well on some songs and badly on others: see [What works and what doesn't](#what-works-and-what-doesnt).

## What a project contains

- the song's tempo, and markers for its sections (intro, drop, break…)
- patterns for each part and section (kick, snare, hats, bass, chords, lead), with names you can read
- channels copied from FL Studio's demo projects, with their plugin, preset and mixer effects
- the song's drums, bass, other and vocals stems as muted audio clips on their own tracks
- the agent's notes on each choice: the chord names, where a part repeats, what it wasn't sure about

## How it works

1. **Split.** [Demucs](https://github.com/facebookresearch/demucs) separates the song into drums, bass, other and vocals.
2. **Listen.** unbake finds the tempo and first downbeat, the key, the sections and the drum hits. [Basic Pitch](https://github.com/spotify/basic-pitch) turns the bass and other stems into notes. From those, unbake names the chords (Am9, Bbmaj7, Cm/Eb…), finds which bars repeat, and offers several melody candidates. Every result is a *finding*: a value with a confidence, the alternatives that came close, and what unbake couldn't tell.
3. **Build.** The agent picks a reference project from FL Studio's demo songs in the same genre, copies its sounds and effect chains, and writes patterns from the song's facts. unbake writes the `.flp` file itself, so FL Studio doesn't need to be open.
4. **Check.** unbake compares every channel's notes with what the song plays in the same bars. It also renders the project through FL Studio's command line and compares each part's sound with its stem: rhythm, notes, tone, brightness, stereo width, and how loud it sits in the mix.
5. **You listen and steer.** By default the agent stops to ask you at set points: the genre and reference, after each loop, and before the full song. You can also ask it to work on its own from start to finish and report every choice it made.

## Install (Windows)

You need:

- Windows 10 or 11, and FL Studio 2024 or later (tested with FL Studio 25.1)
- Rust, ffmpeg, uv, Demucs and Basic Pitch
- optionally an NVIDIA GPU, which makes Demucs about 4 times faster

In PowerShell:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Rustlang.Rustup
winget install Gyan.FFmpeg
winget install astral-sh.uv
uv tool install --python 3.11 demucs --with numpy --with soundfile
uv tool install --python 3.10 basic-pitch --with "setuptools<81"
uv tool update-shell
```

Open a new terminal so the tools are on your PATH, then build unbake:

```powershell
git clone https://github.com/Qznad/unbake
cd unbake
cargo build --release
.\target\release\unbake.exe doctor
```

`doctor` checks each tool and says what it's needed for. It also says how to fix anything that's missing, including the command for a GPU build of Demucs.

**On Linux and macOS:** unbake started on Linux, and the listening side and `rebuild` run there too. Reference sounds, sample search and rendering look for FL Studio's Windows install, so they don't work there yet. macOS is untested.

## Use it with an agent

unbake runs as an [MCP](https://modelcontextprotocol.io) server, so an agent such as [Claude Code](https://claude.com/claude-code) can call its tools. Add it to a `.mcp.json` file in the folder you work from:

```json
{
  "mcpServers": {
    "unbake": {
      "command": "C:/path/to/unbake/target/release/unbake.exe",
      "args": ["mcp"]
    }
  }
}
```

Use forward slashes in the path. You can also run `claude mcp add unbake -- C:/path/to/unbake.exe mcp`. When the agent connects, the server sends it the whole workflow ([skills/unbake/SKILL.md](skills/unbake/SKILL.md)), so there's nothing else to install. Then ask, for example:

> Use unbake on `C:/Music/my song.mp3`, in `C:/Users/me/Documents/Image-Line/FL Studio/Projects/unbake/my song`. Start with a loop of the drop.

Save projects under `Documents/Image-Line/FL Studio/Projects/unbake/`, where FL Studio looks for their samples.

To watch the agent build the whole song without stopping you, ask for it in so many words, for example "work on your own from scratch without stopping me". Keep FL Studio closed during the run, because the agent renders through it.

## Use it on its own

```powershell
.\target\release\unbake.exe rebuild "C:\Music\my song.mp3" -o "$HOME\Documents\Image-Line\FL Studio\Projects\unbake\my song"
```

`rebuild` needs no agent. It writes a note-for-note project from what it heard: one pattern per part covering the whole song, sounds cut from the stems, MIDI files and a `report.html`. It's more literal and messier than an agent's project. To go step by step instead, run `transcribe`, then `write`.

| | Commands |
|---|---|
| Listen | `transcribe`, `analyze`, `split`, `info`, `overview`, `timeline`, `harmony`, `grid`, `parts`, `inspect`, `candidates`, `choose`, `piano-roll`, `sounds-like`, `clues` |
| Build | `compose` (a project from a JSON description), `write`, `rebuild` |
| Check | `check`, `render`, `compare-render`, `eval` |
| Read FL Studio projects | `flp-info`, `pattern`, `playlist`, `references`, `samples` |
| Setup | `doctor`, `mcp` |

`unbake <command> --help` explains each one.

## What works and what doesn't

Works well:

- tempo, key and sections on most songs. Half or double tempo is sometimes a coin flip, and unbake says so.
- chord progressions, drum grooves and bass lines
- projects that open cleanly in FL Studio, with every sample found

Hard:

- **The lead melody.** The "other" stem mixes several instruments, and Basic Pitch hears maybe half of a busy lead. unbake offers candidates and the agent cleans them up, but the lead is often wrong.
- **Effects.** Reverb, delay and sidechain are read as clues ("these notes come back a dotted 8th later and quieter: a delay?"), not copied. Filter sweeps and fades need automation clips, which unbake can't write yet.
- **The range of music.** It assumes 4/4 on a straight 16th-note grid, and it has been tuned on a handful of electronic instrumentals. Vocals are separated but barely used.
- **It can't hear.** Its numbers say whether a part got closer to its stem, not whether it sounds good. You decide that.

## Contributing

Ideas and help are welcome, especially on:

- melody transcription from a mixed stem
- automation clips (volume fades, filter sweeps) in `compose`
- the FL Studio side on macOS, where FL's folders differ
- songs in other genres, or with vocals, swing or other meters

`cargo test`, `cargo clippy` (with no warnings) and `cargo fmt --check` should pass. Each function has one short lowercase comment above it, and there are no other comments. The test songs aren't in the repo because they're copyrighted, so the tests build their own audio and projects.

While an agent runs the MCP server, Windows locks its exe and `cargo build` fails. To avoid that, point `.mcp.json` at a copy of the exe.

## A note on copyright

unbake is for studying music. The projects it writes hold the song's own stems, so only share or release what you have the rights to. The reference sounds come from your own FL Studio install, and nothing from Image-Line ships with unbake.

## Credits

- [Demucs](https://github.com/facebookresearch/demucs) (MIT), for the stems
- [Basic Pitch](https://github.com/spotify/basic-pitch) (Apache-2.0), for the notes
- [PyFLP](https://github.com/demberto/PyFLP), whose sample projects helped check the FLP reader and writer
- [REA](https://github.com/morluto/rea), whose design inspired unbake's findings and its agent skill
- [FL Studio](https://www.image-line.com/) by Image-Line, whose demo projects by real producers are the reference sounds

unbake is made by [Qznad](https://github.com/Qznad), AI-assisted.
