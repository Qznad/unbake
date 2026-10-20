---
name: unbake
description: Turn a song (mp3, wav or any audio file) into an editable FL Studio project with unbake, so the user can learn how it is built. Use when the user wants an FL project, MIDI, stems, the notes, chords, tempo or key of a song. The user chooses between unbake's candidates; the agent shows the evidence and explains.
---

# unbake

unbake listens to a song and writes an FL Studio project: tempo, section markers, one pattern each for drums, bass, melody and chords, sounds cut from the song, the four stems as muted audio clips, and a report. The goal is a project people can **learn from**: the right tempo, the right notes and clean parts. It is never an exact copy, so say what is uncertain.

Every result carries **findings**: a value, a confidence, alternatives that came close, unknowns and limitations. Confidence (0 to 1) says how clearly a choice beat its alternatives, not whether it is right. A note Basic Pitch didn't hear is unknown, not silence.

## The user chooses, you show and explain

Don't decide silently when unbake gives choices. Show the user what differs, with a piano roll when it helps. Give your recommendation and why in plain words, then let them pick. Many users are learning music production, so explain terms like "half time" or "relative key" in a sentence when they come up.

## Workflow

1. **Check the tools when something fails.** Call `doctor`. Each check says ok, warning or missing, what it is needed for, and the fix command. Show the fix and ask before running installs. A missing basic-pitch still works but with weaker notes; a missing demucs works only with `stems` given.

2. **Transcribe.** Call `transcribe` with the song (and `out`, the folder that becomes the FL project folder). It takes about a minute with a GPU and a few minutes without, so tell the user before you start. If the user gives a `stems` folder, it skips the separation.

3. **Tell the user what was found.** Summarise tempo, key and bars, then go through the **unknowns**:
   - Tempo "half or double time": ask whether the beat feels like the chosen tempo or the alternative, for example by counting along. unbake can't change the tempo yet, so if the user hears the other one, say the patterns will look stretched or squashed in FL and that the notes are still in time.
   - Key with a close alternative: name both, and say a relative key uses the same notes, so it only changes which note feels like home.
   - Bass spanning more than two octaves: some notes may be octave slips.

4. **Pick the melody with the user.** Call `melody_candidates`.
   - If there is only one, or the others share more than 90% of their notes with it, keep the chosen one and say so.
   - Otherwise call `piano_roll` with the candidates (up to four, first in orange, then cyan, pink, green) over 4 to 8 bars where the song is busy, often the first drop or chorus. Look at it and show it to the user. Describe the difference: range, how jumpy it is, how dense, how many notes it shares with the first.
   - Recommend one. A `vocals` candidate inside about two octaves is usually the real melody of a sung song. A line spanning more than two octaves is probably jumping between instruments. Then ask the user which one sounds like the lead.
   - Call `choose_melody` with their answer. Everything else from the other stem becomes the chords part.
   - Use `inspect_part` to answer questions about specific bars ("what notes does the bass play in bar 9?"). Don't list whole songs of notes.

5. **Write the project.** Call `write_flp` with the folder. Options to offer when they fit what the user wants:
   - `simple_chords`: clean triads that follow the key instead of every note heard. Easier to read and edit, less exact.
   - `loops`: up to 4 reused 4-bar loops per part instead of one pattern copying the whole song. Tidier, but changes in the middle of the song get averaged away.
   - `sounds`: a folder with the user's own sounds (kick, snare, hat, bass, lead, pad).
   - `template`: the user's own FL template with channels named Kick, Snare, Hat, Bass, Lead and Pad.

6. **Explain the song.** After writing, tell the user how the song is built, as a short lesson:
   - tempo and key, and what the key's notes are
   - the chord progression (the chords finding), in plain names
   - the sections from the markers
   - what the melody and bass do (stepwise or jumping, arpeggios, held notes)
   - which parts to check by ear in FL, from the unknowns
   Then say where the project is (the `.flp` in the folder). Tell them to open it from that folder, or copy the **whole** folder, because the sounds and stems sit next to the `.flp`.

## Other tools

- `rebuild` does steps 2 and 5 in one go with the default melody. Use it only when the user wants a project without choosing anything.
- `analyze` gives tempo and key from the mix in seconds, without stems.
- `split` only separates stems. `flp_info` lists what is inside any `.flp`.

## Limits to keep in mind

- One tempo for the whole song, notes on a straight 1/16 grid in 4/4, so swing, triplets and other meters are lost.
- Drums are kick, snare and hat only. A hat that lands on a snare is often missed.
- The other stem can hold several instruments, so the melody can jump between them.
- Reverb, delay and other effects aren't recreated yet. Dry notes sound thinner than the song.

## Without MCP

The same steps exist on the command line: `unbake doctor`, `unbake transcribe song.mp3 -o folder`, `unbake candidates folder`, `unbake piano-roll folder loudest vocals --from-bar 9 --bars 8`, `unbake inspect folder bass --from-bar 9`, `unbake choose folder vocals`, `unbake write folder`.
