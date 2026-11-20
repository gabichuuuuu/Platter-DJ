# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/gabichuuuuu/Platter-DJ/compare/v0.6.0...v0.7.0) - 2026-11-20

### Added

- smart playlists with a dialog of rules ([#50](https://github.com/gabichuuuuu/Platter-DJ/pull/50))

## [0.6.0](https://github.com/gabichuuuuu/Platter-DJ/compare/v0.5.0...v0.6.0) - 2026-11-20

### Added

- *(engine)* recordings node, keep values, fade-cue mixing and tempo zones ([#48](https://github.com/gabichuuuuu/Platter-DJ/pull/48))

## [0.5.0](https://github.com/gabichuuuuu/Platter-DJ/compare/v0.4.0...v0.5.0) - 2026-11-19

### Added

- M4 effect units, mixer effects, maximiser and mix recorder ([#46](https://github.com/gabichuuuuu/Platter-DJ/pull/46))

## [0.4.0](https://github.com/gabichuuuuu/Platter-DJ/compare/v0.3.1...v0.4.0) - 2026-11-18

### Added

- four-deck layout chosen in the preferences, and wider decks in full screen ([#42](https://github.com/gabichuuuuu/Platter-DJ/pull/42))

## [0.3.1](https://github.com/gabichuuuuu/Platter-DJ/compare/v0.3.0...v0.3.1) - 2026-11-18

### Added

- M3 engine, mixer, MIDI input and the collection in the browser ([#30](https://github.com/gabichuuuuu/Platter-DJ/pull/30))

## [0.3.0](https://github.com/gabichuuuuu/Platter-DJ/compare/v0.2.0...v0.3.0) - 2026-11-17

### Added

- M3 playlists, covers, tag editing, preferences and deck controls ([#37](https://github.com/gabichuuuuu/Platter-DJ/pull/37))
- *(ui-egui)* search and sort the browser list, and show the MIDI controllers ([#31](https://github.com/gabichuuuuu/Platter-DJ/pull/31))
- M3 engine, mixer, MIDI input and the collection in the browser ([#30](https://github.com/gabichuuuuu/Platter-DJ/pull/30))
- *(engine)* add decks C and D ([#39](https://github.com/gabichuuuuu/Platter-DJ/pull/39))
- *(decode)* read covers and write tags and covers into track files ([#36](https://github.com/gabichuuuuu/Platter-DJ/pull/36))
- *(ui-egui)* redraw the Platter mark for the header size ([#33](https://github.com/gabichuuuuu/Platter-DJ/pull/33))

## [0.2.0](https://github.com/gabichuuuuu/Platter-DJ/releases/tag/v0.2.0) - 2026-11-15

### Added

- *(control)* turn the messages of MIDI controllers into commands ([#22](https://github.com/gabichuuuuu/Platter-DJ/pull/22))
- *(library)* import a folder and the folders inside it ([#23](https://github.com/gabichuuuuu/Platter-DJ/pull/23))
- *(library)* keep the collection, playlists and history in one SQLite file ([#21](https://github.com/gabichuuuuu/Platter-DJ/pull/21))
- *(engine)* keep the key of a deck while its tempo changes ([#20](https://github.com/gabichuuuuu/Platter-DJ/pull/20))
- *(engine)* analyse each loaded track and reset the grid to what it found ([#19](https://github.com/gabichuuuuu/Platter-DJ/pull/19))
- *(analysis)* detect tempo, first beat and key with Platter's own code ([#18](https://github.com/gabichuuuuu/Platter-DJ/pull/18))
- *(engine)* add the metronome of the master clock ([#17](https://github.com/gabichuuuuu/Platter-DJ/pull/17))
- *(ui-egui)* add SYNC, MASTER, the phase meter and the master clock ([#16](https://github.com/gabichuuuuu/Platter-DJ/pull/16))
- *(engine)* add Snap and Quantize ([#12](https://github.com/gabichuuuuu/Platter-DJ/pull/12))
- *(platter)* call the product Platter DJ where its users see the name ([#11](https://github.com/gabichuuuuu/Platter-DJ/pull/11))
- *(engine)* add sync, the tempo master and the master clock ([#10](https://github.com/gabichuuuuu/Platter-DJ/pull/10))
- *(ui-egui)* add the hotcue pads, the CUE page and the cues in the waveforms ([#9](https://github.com/gabichuuuuu/Platter-DJ/pull/9))
- *(engine)* add stored cues and the eight hotcues ([#8](https://github.com/gabichuuuuu/Platter-DJ/pull/8))
- *(ui-egui)* add the loop controls, the MOVE page and the loop in the waveforms ([#7](https://github.com/gabichuuuuu/Platter-DJ/pull/7))
- *(engine)* add loops, beatjumps and the moves of a loop ([#5](https://github.com/gabichuuuuu/Platter-DJ/pull/5))
- *(ui-egui)* add the GRID page and the beat lines of the waveform ([#4](https://github.com/gabichuuuuu/Platter-DJ/pull/4))
- *(engine)* add the beatgrid with tempo, tap, markers and lock ([#3](https://github.com/gabichuuuuu/Platter-DJ/pull/3))
- *(platter)* add the desktop application: two decks, the mixer, audio settings, a file selector, editable shortcuts, English and Spanish
- *(audio-io)* add the output stream, with a deviceless backend for tests
- *(engine)* add decks, mixer, commands, the processor and the deviceless render
- *(decode)* decode MP3, M4A (AAC), WAV, AIFF, FLAC and Ogg Vorbis; read the tempo tag
- *(dsp)* add interpolation, equaliser, filter, smoothing, meter, balance law and limiter
- *(cli)* add platter-cli with info and render

### Fixed

- *(audio-io)* let the silent output catch up when its timer wakes late ([#14](https://github.com/gabichuuuuu/Platter-DJ/pull/14))
