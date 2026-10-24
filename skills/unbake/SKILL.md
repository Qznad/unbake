---
name: unbake
description: Turn a song (mp3, wav or any audio file) into an FL Studio project with unbake, so the user can learn how it is built. Use when the user wants an FL project, a loop or beat like a song, MIDI, stems, the notes, chords, tempo or key of a song. unbake hears the song's facts, the agent builds a loop from them with sounds copied from real producers' projects, and the user listens and steers.
---

# unbake

unbake listens to a song and finds its tempo, key, sections, chord progression, drum groove, bass line and melody candidates. From those facts you build an FL Studio project the user can **learn from**: a loop that plays the song's chords, bass and groove with sounds and effects copied from real producers' FL projects, then the whole arrangement. It is inspired by the song, never an exact copy, so say what came from the song and what didn't.

Every result carries **findings**: a value, a confidence, alternatives that came close, unknowns and limitations. Confidence (0 to 1) says how clearly a choice beat its alternatives, not whether it is right. A note Basic Pitch didn't hear is unknown, not silence. unbake can't hear the result and neither can you: the user's ears are the test, so ask them.

## The user chooses, you show and explain

Don't decide silently when there are choices: the genre, the melody, the reference sounds. Show what differs, give your recommendation and why in plain words, then let them pick. The users already use FL Studio and want to understand how a song is built, so explain the music (why this chord, why the hats double here) more than the software.

## Stop points

These are rules, not suggestions. Stop and wait for the user's answer at each one, even when they asked for everything in one go ("make a loop, then the whole song"): that asks for the whole journey, not for skipping their choices.

1. **After transcribing, before building anything:** tell them what was found, then ask the genre (with your suggestion), the tempo when the tempo finding has a half or double time unknown, the reference project and sounds you propose, and which section the loop comes from (with your suggestion from `song_overview`).
2. **After writing each loop,** the main one and every section loop: tell them which file to open and ask the three listening questions (step 5 of path A). Don't write another version, the next section or the whole song until they say it works.
3. **Before the section loops:** show the section plan made from `song_timeline` (the sections with their bars, and for each part what it reuses, builds or needs the user to confirm, with the evidence) and wait for an OK. Ask the user about every "ask" part here.
4. **After combining the whole song:** ask them to listen and say what's wrong, section by section.

Build from this song's facts and FL's references. Projects you find on disk from earlier unbake runs are not this user's choices: don't reuse their sounds or structure unless the user points you to them.

## First steps

1. **Check the tools when something fails.** Call `doctor`. Its first line says which unbake build is answering (version and commit); a tool refusing a setting you expected to work usually means an older build. Each check says ok, warning or missing, what it is needed for, and the fix command. Show the fix and ask before running installs.

2. **Transcribe.** Call `transcribe` with the song and `out`, the rebuild folder. It takes about a minute with a GPU and a few minutes without, so say so first. A `stems` folder skips the separation.

3. **Tell the user what was found.** Tempo, key, bars and sections, then the **unknowns**:
   - Tempo "half or double time": ask whether the beat feels like the chosen tempo or the alternative.
   - A close key: name both. A relative key uses the same notes, so only the home note changes.
   - A bass or melody over two octaves: probably octave slips or several instruments.

Then take path A (recommended) or B.

## A. Build a loop like the song, then the song

This is the path users liked: real producers' sounds playing the song's own facts. Start with one 8-bar loop of the song's main section, get it right with the user, and only then arrange the whole song.

1. **Agree on the genre (stop point 1).** Suggest one from the tempo and the drums (for example 160 to 180 with busy breaks is drum & bass, breakcore sits around 140 to 180, a kick on every beat at 120 to 130 is house) and ask the user to confirm or correct it.

2. **Pick a reference.** Call `references` (a short summary of each project) with a tempo range around the song's (allow double or half for half-time genres) or with words like `break`. Prefer demo songs, which are full tracks, over templates, which are empty. Call `flp_info` on the best one or two and propose which channels to borrow for drums, sub, bass, pad and lead. Avoid channels marked `volume 0, automated` (they were faded in by automation; if you use one anyway it is reset to 78%) and `not installed`. Use `reference_pattern` to read how the producer wrote a groove, bassline or break pattern.

3. **Read the song's main section.** Call `song_overview` to see, bar by bar, where the drums, bass and other parts play and which chord sounds, and pick the drop or chorus, usually 4 or 8 bars. Then:
   - drums: `part_grid` with `drums` gives kick, snare and hat as hit strings you can put straight into `grids`
   - bass: `part_grid` with `bass` shows the root of each bar and its rhythm (held, 8th pulse, octave bounce); ignore smeared long notes and octave ghosts
   - chords: `harmony` gives the real chords with their extensions (Am9, Gm11, Bbmaj9) and the notes in each; voice the loop's chords from those notes. The name follows the bass, so a walking bass under one held chord shows as several names
   - melody: `part_grid` with `melody` shows the note on each step; keep the notes on the main grid (often 8ths) and turn them into a clean 2 to 4 bar motif that fits the chords
   - `inspect_part` lists exact note lengths and velocities when you need them

4. **Write the loop** with `compose_loop`, into `Documents/Image-Line/FL Studio/Projects/unbake/<song> loop` so it opens straight in FL. Make it the way a producer would:
   - short patterns (1 bar drums, 2 bar breaks, 4 bar chords, bass and lead) repeated in the playlist, each part on its own track
   - the reference's break or drum pattern copied as is, with the song's kick and snare layered on top as `grids`
   - a sub holding one long root per bar, plus an audible bass layer an octave or more up, since a pure sub can't be heard on laptop speakers
   - a fill in the last bar (a snare roll rising in velocity) and a crash on the first bar of each phrase
   - copied channels bring their mixer effects; set `effects: false` for a channel whose insert has filters that were automated in the reference (it comes out silent)
   - play a copied sound in the register its producer wrote for it: read their pattern with `reference_pattern` and move your notes by octaves to match (a pluck written around C3 sounds wrong at C5)
   - levels with `volume`: drums 80 to 85, hats 40 to 50, sub about 80, bass layer 50 to 60, pad 60 to 70, lead about 70
   - markers saying where the loop comes from, like "Drop loop (from bars 33-36)"
   - `tracks` naming the playlist tracks after the instruments (Kick, Snare, Hats, Sub, Bass, Keys, Lead)
   - `stems` with the transcribed folder and the loop's source bars (`from_bar`, `bars`), so the song's four stems sit muted under the loop and the user can unmute one to compare each part; keep the project at the song's tempo so they line up

5. **Ask the user to listen (stop point 2).** Tell them which file to open, then ask three things: can every part be heard, how does it feel overall, and which part is weakest. Silent parts usually mean a volume of 0 or an automated filter in the copied channel or its effects.

6. **Improve it.** The description is saved next to the project as json. Change only what the user pointed at and write it under a new name (v2, v3) so they can compare. Explain each change in music terms ("the bass now holds the root and pulses 8ths an octave up, the way the song's bar 36 does").

7. **Then the whole song, one section at a time,** once the user says the loop works. The loop came out well because the user listened to it and steered, so give every section the same care instead of writing the whole song in one go:
   - **Section plan (stop point 3).** Read the song's repeating cycle (often 4 or 8 bars) from the chord of each bar in `song_overview`, then call `song_timeline` with the cycle's first bar and length, and the song bars of the approved loop as `approved` (like "33-40"). It gives every stretch of the song, part by part (drums, bass, chords, lead): "same as bar X" with its likeness, "close to bar X", "new" (with its song_parts letter, and whether it was heard several times or once) or "silent", how clearly unbake hears the stretch, and the work left: reuse, build, ask, done. Plan from it, part by part, never by section alone:
     - **reuse** a part only where its own cell says "same as" a bar that is already built or approved. Matching chords say nothing about the lead or the drums.
     - **build** the parts it lists, from that stretch's own facts (`song_parts` letters, `part_grid`, `harmony`).
     - **ask** the user about the parts it lists before building them: a lead heard once is often another instrument, and in a filtered or quiet stretch nothing is certain. Tell them what unbake heard and offer choices.
     Group the stretches into sections (intro, verse, build, drop, break, outro). In the plan you show, every part of every section names its evidence, like "lead: same as bar 33 (0.85)", "drums: new, letter C" or "lead: ask, heard once". Show the plan and wait for an OK.
   - **Quiet, filtered or sparse sections** (intros, breakdowns, builds) are often a drop played softer or behind a filter, and they transcribe badly: few drum hits, wrong notes from the filter, no bass. Spot them in `song_overview` by their levels (`drums_db`, `other_db` well below 0) and `drums_top_khz` (around 0.3 means the drums are behind a low-pass filter, rising values mean it opens). `song_timeline` marks them unclear and adds what they sound like; `sounds_like` gives the full list of loud bars a stretch matches in chords and in drum rhythm. A clear chord match (0.6 or more) lets you reuse that section's chords, and a clear drum match its drums; it says nothing about the lead, so the lead stays an "ask" for the user. Show how the song brings it in: velocities rising bar by bar for a fade, and for a filter sweep tell the user how to draw it in FL (route the parts to one insert, add Fruity Filter, right-click Cutoff, Create automation clip), since unbake can't write automation yet.
   - **One loop per distinct section,** each its own `compose_loop` project in the same folder, named like "<song> intro". First call `flp_info` and `playlist` on the user's saved main loop to see what they changed in FL (muted, moved or deleted clips, new effects), then build on it: copy its channels with `copy` {project: that .flp, channel}, so their sound edits carry over. Write the section's own parts: `from_parts` for its melody and bass letters (a letter heard several times is voted and clean; a melody letter heard once that is a `variation_of` another has its gaps filled from it; one heard once with no relative is often another instrument, so put it on a track listed in `mute`, named like "Lead (unsure, check against the Other stem)", and tell the user; bass comes back on each bar's root and fifth), `grids` from `part_grid` for its drums, and chords voiced from `harmony` over its bars. Make it one cycle of the section (4 or 8 bars), put the fill in its own pattern in the last bar when a different section follows, and give it `stems` cut to its bars.
   - **Read the check before the user listens.** Every `compose_loop` with `stems` answers with `check`: each channel compared with what the song plays in the same bars, 4 bars at a time. A part that "doesn't fit" is probably copied from other bars or wrong: fix it first. "Weak" and "unclear" parts go to the user by name ("the lead in bars 17-20 is only partly heard in the song, listen to it against the Other stem"). Use `check_project` the same way on a project the user edited.
   - **Listen to each section (stop point 2).** Same three questions, fix what they point at as v2, and only then start the next section. If they changed a section in FL, read it with `playlist` and `flp_info` before going on.
   - **Combine** once every section works: `compose_loop` with `sections`, one entry per section with its project, the bars it starts at (`at`, several for a section that comes back), its length (`bars`; the loop repeats to fill it and is cut there), `first_only` for a crash that only opens the section and `last_only` for the fill that only closes it. List the main loop first, so its tracks come first and its master effects are used. Each section brings its sounds, effects, patterns and clips as the user last saved them (what they muted stays silent, and a stem they switched on comes along as an audio clip), and gets a marker with its name. Add `stems` from bar 1, so project bar 1 is song bar 1, read its `check` the same way, then stop point 4. To fix one section, change its loop and combine again.

8. **Explain the song** as a short lesson: tempo and key and its notes, the chord progression in plain names, what the bass, drums and melody do, which parts came from the song and which from the reference, and what to try changing.

## B. Copy the song note for note

A faithful draft with every note heard, the stems as muted audio clips and sounds cut from the stems. Useful for checking what unbake heard, but it often sounds rough.

1. **Pick the melody with the user.** Call `melody_candidates`. If the others share more than 90% of their notes with the chosen one, keep it. Otherwise call `piano_roll` with up to four candidates (orange, cyan, pink, green, white where they share a note) over 4 to 8 busy bars, show it, describe the differences, recommend one (a `vocals` candidate inside two octaves is usually the real melody of a sung song) and **stop until the user picks**. Then call `choose_melody`.

2. **Write it** with `write_flp`. Options: `simple_chords` (plain triads), `raw_melody` (every note heard, no tidying), `loops` (up to 4 reused 4-bar loops per part), `sounds` (the user's own kick, snare, hat, bass, lead, pad) and `template`.

3. Tell them to open the `.flp` from its folder, or copy the **whole** folder, because the sounds and stems sit next to it.

## Other tools

- `inspect_part` answers questions about specific bars ("what does the bass play in bar 9?"). Don't list whole songs of notes.
- `find_samples` finds FL's own kicks, snares, hats, crashes and loops by words.
- `playlist` lists any project's tracks and clips in compose_loop's playlist form: what the user changed in a loop, or how a producer arranged a reference song.
- `sounds_like` finds the bars a stretch matches in chords and drum rhythm, whatever its level: where a quiet intro or break comes from.
- `song_timeline` maps the whole song part by part with the work left; `check_project` compares any project with the song bar by bar.
- `rebuild` does transcribe and write in one go. `analyze` gives tempo and key in seconds. `split` only separates stems.

## Limits to keep in mind

- One tempo for the whole song, notes on a straight 1/16 grid in 4/4, so swing, triplets and other meters are lost.
- Drums are kick, snare and hat only, and a hat on a snare is often missed.
- The other stem can hold several instruments, so the melody candidates can jump between them. That's why path A rewrites the motif cleanly.
- unbake can copy plugins and effects but can't set their knobs, so the sound comes from the reference's presets.
- unbake can't write automation clips yet, so volume fades and filter sweeps are velocity ramps or steps the user draws in FL.
- Quiet or filtered bars transcribe badly. Trust `sounds_like` and the levels there more than their notes.

## Without MCP

The same tools exist on the command line: `unbake doctor`, `unbake transcribe song.mp3 -o folder`, `unbake inspect folder bass --from-bar 33`, `unbake references --min-bpm 160`, `unbake flp-info project.flp`, `unbake pattern project.flp "Main Break"`, `unbake playlist project.flp`, `unbake sounds-like folder --from-bar 1 --bars 4`, `unbake timeline folder --from-bar 9 --bars 4 --approved 33-40`, `unbake check project.flp folder --from-bar 33`, `unbake samples snare power`, `unbake compose loop.json -o folder`, and for path B `unbake candidates`, `unbake piano-roll`, `unbake choose` and `unbake write`.
