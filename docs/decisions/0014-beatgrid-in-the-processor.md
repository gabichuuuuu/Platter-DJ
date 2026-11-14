# 0014. The beatgrid lives in the processor

Status: accepted (M3).

## Context

The grid commands of `docs/behavior/beatgrid.md` act at the play position: set a marker, move
or delete the marker on or before it, jump to the previous or the next one. The transport
already promises that commands sent in a row apply in order, so a seek followed by a marker set
must put the marker where the seek landed. Loops, beatjump, quantize and sync, which come next
in M3, read the grid inside the audio callback.

## Options

1. The control side holds the grid and uses the position the processor publishes once per
   block. Simple, but the published position lags the commands still in the queue: a seek and
   a marker set in a row would put the marker at the old position.
2. The control side holds the grid and tracks the seeks it sends to predict the position. It
   duplicates the transport logic of the processor, cue preview and end of track included.
3. The processor holds the grid. Edits arrive as small commands and are applied at the
   position the deck has once the commands before are applied, as the cue point is. The grid
   goes back to the control side for the snapshot.

## Choice

Option 3. `BeatGrid` is a fixed-size value of `platter-core` (a tempo, an array of 128 markers
with a count, a lock), so the processor edits and copies it without allocating. After a change
the processor sends the whole grid through a third queue, `GridReport`, at most once per deck
and block; when that queue is full it keeps the report for the next block, so the control
side always ends with the latest grid. `Engine::poll` takes the reports in and the snapshot
shows the last one.

The tap button is timed on the control side, where the clock may be read, and sends a tempo
like any other.

Since the loops (`0015-jumps-that-keep-the-time.md`) the report is `DeckReport` and carries
the loop of the deck as well; since the hotcues, its stored cues and the selected one too,
for the same reason: a store and the change of type after it need no round trip.

## How it was checked

- `crates/engine/tests/beatgrid.rs`: a seek and a marker set in a row put the marker where the
  seek landed; edits sent after a load belong to the track that arrives; an empty deck changes
  nothing.
- `crates/engine/tests/realtime_memory.rs`: every grid command is processed without
  allocating or freeing memory.
- `crates/engine/tests/command_sweep.rs`: after any sequence of commands the markers are in
  order, finite and at most 128, and the tempo is inside its range.
