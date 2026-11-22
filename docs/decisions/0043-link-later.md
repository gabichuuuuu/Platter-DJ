# 0043. Link compatibility leaves M5

Status: accepted (M5).

## Context

M5 listed compatibility with Ableton Link, written by the project: tempo, beat and phase shared
with other applications on the local network. The only detailed description of the protocol known
to the project is the code of the official library, published under the GPL. `AGENTS.md` forbids
copying GPL code and using it as a guide, so that code cannot be the source of an own
implementation, and the licence of Platter (`MIT OR Apache-2.0`) rules out linking it. The name is
also a trademark of its owner.

## Options

- Write the protocol from a public description that is not that code, if one exists.
- A clean-room process: someone who has read the library writes a specification without code, and
  the implementation is written only from that text.
- Leave Link out of M5 and keep the MIDI clock as the external sync of M5
  (`docs/decisions/0041-midi-clock.md`).

## Choice

Link leaves M5 and stays planned with no milestone (`docs/parity.toml`, `milestone = "later"`).
It comes back when a public description that is not GPL code exists or a clean-room
specification is written; before shipping, the use of the name is checked with its owner. The
Sync page keeps the MIDI clock only.

## How it was measured

Not measured: a decision of the maintainer on what M5 contains.
