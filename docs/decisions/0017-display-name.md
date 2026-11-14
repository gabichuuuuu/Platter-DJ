# 0017. The product is called Platter DJ; the code keeps the name Platter

Status: accepted (M3).

## Context

The product was named Platter. On its own the word names many other things, a serving dish
and the disc of a hard drive among them, so a search for it rarely finds a DJ application. The
maintainer asked for a name that says what the product is.

## Options

1. Keep Platter.
2. Call the product Platter DJ where its users see the name, and keep Platter in the code: the
   crates, the binaries, the repository and the folder of the settings.
3. Rename everything to Platter DJ, crates and binaries included.

## Choice

Option 2, by decision of the maintainer. Its users see the new name in the title of the
window, in the header, in the texts of the interface and in the README; prose may still say
Platter for short. The crates stay `platter-*`, the binaries `platter` and `platter-cli`, and
the identifier of the application `platter`.

The folder of the settings keeps the name Platter, so the settings and shortcuts saved by an
earlier version are still found. Option 3 would touch every crate and every script that runs
a binary and give its users nothing they can see.

The design system in `docs/design/` writes Platter; it is the maintainer's to update.

## How it was checked

A web search on 14 November 2026 found no DJ application called Platter DJ. Registered
trademarks were not searched.
