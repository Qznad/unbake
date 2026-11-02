# unbake

Turn a song into an FL Studio project you can learn from.

unbake listens to a song for its tempo, key, chords, drums, bass and melody. Then an AI agent such as Claude Code builds an editable FL Studio project from what it heard, using sounds from FL Studio's own demo projects, with the song's stems underneath so you can compare.

> **Early builds (version 0.1).** Windows only. The lead melody is often wrong, and you will run into bugs.

## Install

Paste this into Claude Code:

> Install unbake: run `irm https://qznad.github.io/unbake/install.ps1 | iex` in PowerShell, show me the setup plan it prints, and run it once I say yes.

Then reload Claude Code and ask:

> Use unbake on `C:/Music/my song.mp3`. Start with a loop of the drop.

## More

The guide, an example and the details are on the website: **https://qznad.github.io/unbake**

Projects hold the song's own stems, so only share what you have the rights to. MIT license, AI-assisted, built on [Demucs](https://github.com/facebookresearch/demucs) and [Basic Pitch](https://github.com/spotify/basic-pitch).
