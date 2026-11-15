# 0011. Icons: read from the shape data at start-up

Status: accepted (M2).

## Context

`docs/design/icons/icons.toml` stores 43 icons as SVG shape elements, and
`docs/design/logo/platter-mark.toml` stores the mark the same way. The project has no SVG
dependency. `docs/design/DESIGN.md` leaves two ways open: the interface crate reads the subset
of shapes itself, or `xtask` generates a table of shapes from the file.

## Options

1. **Read at start-up.** `platter-ui-egui` compiles the two files in, reads them when the
   theme loads and turns every shape into straight segments once.
2. **Generate with `xtask`.** A command writes a Rust table of points; the interface crate
   includes it. A gate step fails when the table is out of date.

## Choice

Option 1. There is one source and nothing generated to keep in step with it: the design files
are the maintainer's, and a change there reaches the application with no command to remember.
The reader is about 900 lines with its tests. Option 2 would save the reading at start-up,
which was not measured because nothing suggested it matters, and cost a generated file in the
repository, a new gate step and a second place where an icon can be wrong.

The reader accepts what the files use and nothing else: `path`, `circle` and `rect`; the path
commands `M L H V C S Q A Z`, absolute and relative; `stroke-dasharray`, `stroke-width` and
`fill` on a shape. Anything else is an error that names the icon, so a new icon outside the
subset fails at start-up and in the tests, not silently on screen. Curves become twelve
segments and a full circle thirty-two, which at the sizes icons are drawn (12 to 24 pixels)
cannot be told from the curve. Arcs follow the conversion from end points to a centre in the
SVG specification.

## Limits

- egui strokes have butt caps and mitred joins; the files ask for round ones. At a stroke of
  1.5 units on a 24-unit grid the difference is under a pixel.
- A filled shape is drawn as a convex polygon. The two solid icons, play and pause, and the
  disc of the mark are convex. A concave solid icon would need a tessellator.

## How it was checked

`crates/ui-egui/src/icons.rs`: all 43 icons are read and every point stays on the 24-unit
grid; only play and pause are solid; numbers without separators, relative commands, arcs with
both sweep flags and with flags glued to numbers, cubic, smooth and quadratic curves; shapes
outside the subset are errors; the mark has a ring, two arcs and a filled centre. The
captures of the interface show them drawn.
