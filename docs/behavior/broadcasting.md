# Broadcasting

Broadcasting sends the mix, as it plays, to a streaming server the user runs or rents, from
which listeners hear it over the internet. This note covers what the stream holds; sending it
to a server comes with the next part of the work.

## What the public manual says

*Preferences*, section *Preferences – Broadcasting Page*, and *Advanced Operation*, section
*Broadcasting Your Mix* with its part *Streaming Protocol*, read on 22 November 2026:

- The mix is streamed to a server of the kind of Icecast, which the user sets up first. The
  preferences hold a proxy (none, the one of the system or a custom one), the address, port,
  mount path and password of the server, the format, which sets the quality and so the
  bandwidth, and the metadata listeners see: the address, name, description and genre of the
  stream. Most servers take port 8000.
- The source of the mix recorder must be the internal mix. A Broadcast key starts and stops the
  stream; lit, it is connected, and flashing, it could not connect.
- The manual lists no formats or bitrates and does not say what happens when the connection is
  lost.

## What Platter does

- **The stream.** The mix is coded as Opus in Ogg, after RFC 6716 (the codec,
  <https://www.rfc-editor.org/rfc/rfc6716>), RFC 3533 (the pages,
  <https://www.rfc-editor.org/rfc/rfc3533>) and RFC 7845 (Opus in Ogg,
  <https://www.rfc-editor.org/rfc/rfc7845>): an identification header and a comment header,
  each on a page of its own, then the audio in packets of 20 ms, ten to a page. The comment
  header names Platter as the vendor and holds no other comments.
- **The rate.** Opus codes at 48 kHz. The mix, at the rate of the audio device, is resampled to
  48 kHz with a windowed sinc that keeps its state from one block to the next. The pre-skip of
  the stream counts the delay of the resampler and of the encoder, and its last granule position
  ends it where the mix ended, so a player plays neither the delay nor the padding.
- **The bitrate.** 64, 96, 128, 192 or 256 kilobits per second, stereo.

## Differences

- **Opus only.** The manual leaves the formats open; Platter codes Opus, a royalty-free codec
  that the servers and the browsers of today play. MP3 is left out: its free encoder is under the
  LGPL, which Platter does not link (`docs/decisions/0046-broadcast-encoding.md`).
- **Not yet:** the connection to the server, its settings, the Broadcast key and the metadata.

## Edge cases

- A device rate that is not 48 kHz, such as 44.1 or 96 kHz, is resampled; at 48 kHz the mix
  passes as it is.
- A tone above 24 kHz of a mix at 96 kHz is removed, not folded back.
- The end of the stream is padded with silence to a whole packet; the last granule position
  leaves the padding out.

## Tests

`cargo test -p platter-broadcast`: the pages are read back with a CRC computed apart from the
writer, the headers are checked field by field, and the packets are decoded with libopus to a
sine of the right frequency, level, phase between the channels and length, from 44.1, 48 and
96 kHz. Synthesised signals only; nothing was listened to.