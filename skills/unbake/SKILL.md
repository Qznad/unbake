---
name: unbake
description: Turn a song (mp3, wav or any audio file) into an FL Studio project with unbake, so the user can learn how it is built. Use when the user wants an FL project, a loop or beat like a song, MIDI, stems, the notes, chords, tempo or key of a song. unbake hears the song's facts, the agent builds a loop from them with sounds copied from real producers' projects, and the user listens and steers.
---

# unbake

unbake listens to a song and finds its tempo, key, sections, chord progression, drum groove, bass line and melody candidates. From those facts you build an FL Studio project the user can **learn from**: a loop that plays the song's chords, bass and groove with sounds and effects copied from real producers' FL projects, then the whole arrangement. It is inspired by the song, never an exact copy, so say what came from the song and what didn't.

Every result carries **findings**: a value, a confidence, alternatives that came close, unknowns and limitations. Confidence (0 to 1) says how clearly a choice beat its alternatives, not whether it is right. A note Basic Pitch didn't hear is unknown, not silence. unbake can't hear the result and neither can you: the user's ears are the test, so ask them.

## The user chooses, you show and explain

Don't decide silently when there are choices: the genre, the melody, the reference sounds. Show what differs, give your recommendation and why in plain words, then let them pick. The users already use FL Studio and want to understand how a song is built, so explain the music (why this chord, why the hats double here) more than the software.

## First steps

1. **Check the tools when something fails.** Call `doctor`. Each check says ok, warning or missing, what it is needed for, and the fix command. Show the fix and ask before running installs.

2. **Transcribe.** Call `transcribe` with the song and `out`, the rebuild folder. It takes about a minute with a GPU and a few minutes without, so say so first. A `stems` folder skips the separation.

3. **Tell the user what was found.** Tempo, key, bars and sections, then the **unknowns**:
   - Tempo "half or double time": ask whether the beat feels like the chosen tempo or the alternative.
   - A close key: name both. A relative key uses the same notes, so only the home note changes.
   - A bass or melody over two octaves: probably octave slips or several instruments.

Then take path A (recommended) or B.

## A. Build a loop like the song, then the song

This is the path users liked: real producers' sounds playing the song's own facts. Start with one 8-bar loop of the song's main section, get it right with the user, and only then arrange the whole song.

1. **Agree on the genre.** Suggest one from the tempo and the drums (for example 160 to 180 with busy breaks is drum & bass, breakcore sits around 140 to 180, a kick on every beat at 120 to 130 is house) and ask the user to confirm or correct it.

2. **Pick a reference.** Call `references` with a tempo range around the song's (allow double or half for half-time genres) or with words like `break`. Prefer demo songs, which are full tracks, over templates, which are empty. Call `flp_info` on the best one or two and propose which channels to borrow for drums, sub, bass, pad and lead. Avoid channels marked `volume 0, automated` (they were faded in by automation; if you use one anyway it is reset to 78%) and `not installed`. Use `reference_pattern` to read how the producer wrote a groove, bassline or break pattern.

3. **Read the song's main section.** The drop or chorus from the sections, usually 4 bars. Call `inspect_part` for `drums`, `bass`, `chords` and `melody` over those bars. Look for:
   - drums: where the kicks and snares land, as a 16-step grid per bar
   - bass: the root of each bar and its rhythm (held, 8th pulse, octave bounce); ignore smeared long notes and octave ghosts
   - chords: the held chords and their extra notes (a Cm with a D)
   - melody: the notes on the main grid (often 8ths), turned into a clean 2 to 4 bar motif that fits the chords

4. **Write the loop** with `compose_loop`, into `Documents/Image-Line/FL Studio/Projects/unbake/<song> loop` so it opens straight in FL. Make it the way a producer would:
   - short patterns (1 bar drums, 2 bar breaks, 4 bar chords, bass and lead) repeated in the playlist, each part on its own track
   - the reference's break or drum pattern copied as is, with the song's kick and snare layered on top as `grids`
   - a sub holding one long root per bar, plus an audible bass layer an octave or more up, since a pure sub can't be heard on laptop speakers
   - a fill in the last bar (a snare roll rising in velocity) and a crash on the first bar of each phrase
   - copied channels bring their mixer effects; set `effects: false` for a channel whose insert has filters that were automated in the reference (it comes out silent)
   - levels with `volume`: drums 80 to 85, hats 40 to 50, sub about 80, bass layer 50 to 60, pad 60 to 70, lead about 70
   - markers saying where the loop comes from, like "Drop loop (from bars 33-36)"

5. **Ask the user to listen.** Tell them which file to open, then ask three things: can every part be heard, how does it feel overall, and which part is weakest. Silent parts usually mean a volume of 0 or an automated filter in the copied channel or its effects.

6. **Improve it.** The description is saved next to the project as json. Change only what the user pointed at and write it under a new name (v2, v3) so they can compare. Explain each change in music terms ("the bass now holds the root and pulses 8ths an octave up, the way the song's bar 36 does").

7. **Then the whole song,** once the loop works: an intro, the song's sections from its markers, breaks without drums, and an outro, with the loop's patterns placed and varied in the playlist.

8. **Explain the song** as a short lesson: tempo and key and its notes, the chord progression in plain names, what the bass, drums and melody do, which parts came from the song and which from the reference, and what to try changing.

## B. Copy the song note for note

A faithful draft with every note heard, the stems as muted audio clips and sounds cut from the stems. Useful for checking what unbake heard, but it often sounds rough.

1. **Pick the melody with the user.** Call `melody_candidates`. If the others share more than 90% of their notes with the chosen one, keep it. Otherwise call `piano_roll` with up to four candidates (orange, cyan, pink, green, white where they share a note) over 4 to 8 busy bars, show it, describe the differences, recommend one (a `vocals` candidate inside two octaves is usually the real melody of a sung song) and **stop until the user picks**. Then call `choose_melody`.

2. **Write it** with `write_flp`. Options: `simple_chords` (plain triads), `raw_melody` (every note heard, no tidying), `loops` (up to 4 reused 4-bar loops per part), `sounds` (the user's own kick, snare, hat, bass, lead, pad) and `template`.

3. Tell them to open the `.flp` from its folder, or copy the **whole** folder, because the sounds and stems sit next to it.

## Other tools

- `inspect_part` answers questions about specific bars ("what does the bass play in bar 9?"). Don't list whole songs of notes.
- `find_samples` finds FL's own kicks, snares, hats, crashes and loops by words.
- `rebuild` does transcribe and write in one go. `analyze` gives tempo and key in seconds. `split` only separates stems.

## Limits to keep in mind

- One tempo for the whole song, notes on a straight 1/16 grid in 4/4, so swing, triplets and other meters are lost.
- Drums are kick, snare and hat only, and a hat on a snare is often missed.
- The other stem can hold several instruments, so the melody candidates can jump between them. That's why path A rewrites the motif cleanly.
- unbake can copy plugins and effects but can't set their knobs, so the sound comes from the reference's presets.

## Without MCP

The same tools exist on the command line: `unbake doctor`, `unbake transcribe song.mp3 -o folder`, `unbake inspect folder bass --from-bar 33`, `unbake references --min-bpm 160`, `unbake flp-info project.flp`, `unbake pattern project.flp "Main Break"`, `unbake samples snare power`, `unbake compose loop.json -o folder`, and for path B `unbake candidates`, `unbake piano-roll`, `unbake choose` and `unbake write`.
