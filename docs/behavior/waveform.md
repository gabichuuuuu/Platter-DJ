# Waveforms

The scrolling waveform of each deck, the overview of the whole track, their colours and the
zoom.

## What the public manual says

Sections of the public manual, read on 13 November 2026:

- *Overview*, section *Track deck*
- *Basic usage tutorials*, section *Working with the waveform and the stripe*

A deck shows two views of its track. The waveform shows the track around the play position,
zoomed in; the stripe shows the entire track. The waveform is colour-coded: lighter colours
stand for high frequencies and darker colours for low frequencies. Two buttons zoom the
waveform in and out. Both views share one colour mode out of four.

## What Platter does

- When a track loads, the worker thread that decoded it also computes its peaks in three
  frequency bands: low below 250 Hz, mid between 250 Hz and 2.5 kHz, high above 2.5 kHz. The
  corners are the ones of the equaliser, so the three bands of the waveform are what the
  three knobs of the mixer act on. The data has 400 columns per second, plus an overview of
  at most 1 024 columns for the whole track. The interface draws from this data and never
  looks at the audio.
- Each column is drawn as three nested layers in the hue of its deck, mirrored around the
  centre line: the low band outermost and darkest, the mid band over it, the high band
  innermost and lightest. This is colour mode 1 of `docs/design/DESIGN.md`, the only one in
  M2.
- The scrolling waveform keeps the play position at its centre. Two keys per deck zoom it
  (`waveform.zoom.in`, `waveform.zoom.out`) in five steps, from 4 to 64 seconds across the
  lane.
- The overview shows the whole track with the part already played dimmed. A click on it
  sends `deck.seek` to the position clicked.
- **Parallel lanes.** The scrolling lanes of the two decks are stacked under the status
  header, each centred on the play position of its deck. At the same zoom they share the time
  scale, so the beats of two synced decks line up vertically (`docs/design/DESIGN.md`,
  *Waveform*, parallel layout).

## Differences

- One colour mode; the other three are M4.
- No key resets the zoom; five steps make it two presses away at most.
- Each lane keeps its own zoom: the time scales are the same only while both lanes are at the
  same step. The design system's parallel layout gives them one scale.
- Dragging the scrolling waveform does nothing in M2. The drag modes of the documented
  application arrive with the beatgrid in M3.

## Edge cases

| Input | Result |
| --- | --- |
| An empty deck or a track that is still loading | Empty lanes |
| A click on the overview of an empty deck | Nothing happens |
| A track shorter than the zoomed window | The lane is empty outside the track |
| Audio above full scale, or samples that are not numbers | Columns stay inside the lane |

## Tests

`crates/engine/src/peaks.rs` for the data; `crates/ui-egui/tests/panels.rs` for the commands
of the zoom keys and of the overview.
