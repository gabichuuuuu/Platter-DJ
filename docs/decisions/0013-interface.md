# 0013. Shape of the interface: a view in, commands out, and how it is tested

Status: accepted (M2).

## Context

`AGENTS.md` fixes that the interface reads a snapshot of the state, acts through commands and
holds no engine state, and that egui is a thin, replaceable layer. `docs/design/` fixes how it
looks. This record covers what those leave open: the boundary between the interface crate and
the application, how the design system reaches the screen, and how a graphical interface is
tested in a CI without a display.

## Choices

**A view in, actions out.** Each frame the application builds a `View`: the snapshot of the
engine, the state of the audio output, the settings, the file list, the shortcuts and the
notices of the decks. `Shell::show` draws it and returns a list of `Action`s. An action is a
command of the engine or a command of the application (`AppCommand`: open the preferences,
choose a device, move the selection). The application carries each one out in one function,
`dispatch`. Controls, shortcuts and files dropped on the window all end there; nothing changes
state by another path.

**What the interface remembers.** `ViewState` holds what is only a way of showing things: the
open page of the preferences, the zoom of each waveform, which time is large, the held peak
and clip indicator of each meter, scroll positions. Commands that change only this
(`waveform.zoom.in`, `deck.time_display.toggle`) still go out as actions and come back through
`Shell::apply`, so a shortcut and a click take the same route.

**Own widgets, own layout.** Every control is drawn with the painter from the design tokens
and placed in a rectangle the layout computes from the size of the window. No stock egui
widget is used. That keeps the look to the design system, makes the layout match the reference
screens to the pixel where it matters, and leaves little of egui to replace: input, text
layout and the painter.

**Tokens.** `docs/design/tokens.toml` is compiled into the binary and read when the theme
loads. A token the interface uses and the file lacks is an error at start-up and in the tests.
The four font files of the design system are compiled in the same way.

**Tabular figures.** egui 0.37 cannot switch on OpenType features, so the tabular figures of
the font are out of reach. Text in a tabular style is laid out digit by digit, each digit
centred in a slot as wide as the widest one, as `docs/design/DESIGN.md` prescribes for this
case.

**Keyboard.** Each control handles its own keys while it has the focus and says which keys it
uses; the shortcut table is consulted last, for the keys nobody claimed.

**The preferences are a dialog inside the window.** While it is open, what lies behind is
drawn without taking input and under a veil of the background colour at 55 %, the same veil
the overview of a waveform uses for its played part.

**Capture mode.** `platter --capture <state> <file.png>` draws a state from invented data, at
one pixel per point and 1440 by 900, and saves the frame egui hands back through its
screenshot command. The PNG is written by the project without compression. The invented data
lives in the interface crate (`sample.rs`) and is the same the tests draw.

## Testing without a display

`crates/ui-egui/tests/panels.rs` runs egui without a window: it feeds a frame with pointer and
key events and reads back the actions and the shapes painted.

- Every control registers the command it emits and where it is. The test operates each one
  with the pointer the way its kind is operated, in a fresh interface, and expects that
  command. Disabled controls are operated too and must emit nothing.
- The same list proves that every command of the interface has a shortcut or a control that
  takes the focus.
- The shapes of a frame give every text and every colour painted. Texts are checked against
  marked translations, colours against the token table; a colour with alpha must be a token
  behind its alpha. A shape of a kind the check does not know fails the test, so a new way of
  painting cannot go unchecked.

The registration of controls is the one piece of the interface that exists for the tests. It
costs a vector per frame.

## Limits

- The tests prove that a control emits its command, not that it looks right. That is what the
  capture mode and the comparison with the reference screens are for.
- A narrow window scales the whole interface down; there is no narrow layout.
- The screen-reader bridge of egui is off; see `docs/behavior/interface.md`.

## How it was checked

The eleven tests of `crates/ui-egui/tests/panels.rs` over five states of the interface: the
four of the reference screens and one with a clipped master and a deck that failed to load.
216 controls are operated. The four captures in `target/screenshots/` were compared with
the reference screens by eye.
