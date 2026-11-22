# 0045. DVS timecode leaves M5

Status: accepted (M5).

## Context

M5 listed DVS with an own timecode and decoder: records and CDs carrying a control signal that
Platter reads from an input to drive a deck, in absolute and relative modes. The timecode signals
of the established systems belong to their vendors and have no public specification, so Platter
would need a signal of its own, and records or CDs made with it, to be of any use. Without a
turntable, records and an input device, nothing beyond synthesised signals could be checked.

## Options

- Design an own signal and its decoder now, tested with synthesised signals only.
- Leave DVS out of M5 until there is a way to make and play records with the signal.

## Choice

DVS leaves M5: the timecode, its modes and its preferences page stay planned with no milestone
(`docs/parity.toml`, `milestone = "later"`). Chosen by the maintainer on 22 November 2026. The
live input of M5, which a DVS would build on, is done (`docs/behavior/live-input.md`).

## How it was measured

Not measured: a decision of the maintainer on what M5 contains.