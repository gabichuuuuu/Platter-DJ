# Audio formats

## What the public manual says

Sections of the reference manual:

- *Managing your track collection*, section *Compatible audio file formats*

The reference product plays six formats: MP3, M4A (AAC), WAV, AIFF, FLAC and Ogg Vorbis. Files
protected by DRM cannot be played, and WMA is not supported.

## What Platter does

`platter-decode` reads the same six formats and decodes a whole file into `f32` PCM in memory.

- The content decides the format; the extension is only a hint. A WAV file named `.mp3` is
  read as WAV.
- A mono file feeds both sides. A file with more than two channels plays its first two.
- The sample rate of the file is kept. The engine reads the track at the ratio of the two
  rates while it plays (`docs/decisions/0004-interpolation.md`); nothing is resampled on load.
- Title, artist, album and tempo are read when the file carries them. The tempo comes from
  the tag, with its decimals; values outside 20 to 999 beats per minute are ignored. Platter
  does not measure tempo before M3.
- `probe_file` reads the header and the tags without decoding the audio, for the file list of
  the interface. A file that passes it may still fail to decode.

## Differences

- AAC decoding is a cargo feature of `platter-decode` (`aac`, on by default), so a build can
  leave it out. See `docs/legal-notes.md`.
- M4A files that hold another codec (for example lossless audio) are reported as unsupported.

## Edge cases

| Input | Result |
| --- | --- |
| Not one of the six formats, or protected by DRM | Error "unsupported file", naming the six formats |
| Header that breaks the rules of its format | Error "the file is damaged" |
| File that ends before the audio its header declares | Error "the file is cut short" |
| Declares more than six hours of audio | Error; no memory is reserved for it |
| Sample rate outside 8 kHz to 384 kHz, or more than eight channels | Error "unsupported file" |
| No audio frames at all | Error "the file holds no audio" |
| One damaged packet among good ones | The packet is skipped and the rest plays |
| The decoder library panics on the content | Error "the file is damaged" (`docs/decisions/0008-decoder-panic-boundary.md`) |

No input makes decoding panic or return samples that are not finite numbers.

## Tests

`crates/decode/tests/decode.rs`. WAV, AIFF and FLAC files are synthesised in the test and
compared with the signal they were made from. MP3, AAC and Ogg Vorbis need an encoder to
produce a valid file, and the project has none: for those three the tests cover damaged input
only, and their rows in `docs/parity.toml` stay `in_progress` until a real file has been
checked by hand or an encoder exists.
