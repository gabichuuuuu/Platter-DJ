# 0044. HID devices: async-hid, read on a thread of their own

Status: accepted (M5).

## Context

M5 adds HID controllers with a public or community-documented protocol (`docs/parity.toml`).
Reading a HID device needs the system API of each platform (hidraw, IOHIDManager, Win32), which
the project cannot call without `unsafe`, so a dependency is needed. `docs/dependencies.md`
approved `hidapi` 2.6.7 for M5.

## Options

- **hidapi** 2.6.7 (MIT): mature and much used, with a blocking interface. It compiles the C
  library of the same name, which on Linux needs the headers of libudev, and the build script of
  its repository starts with a header naming the GPL, version 3, which contradicts the MIT licence
  of the crate and was not resolved.
- **async-hid** 0.5.3 (MIT): written in Rust over the system APIs, no C compiler and no libudev.
  Younger and less used. Its interface is asynchronous: listing the devices gives a stream, and
  reading a report a future, so something has to run them.
- Writing the access to the system APIs in the project, which needs `unsafe` in a crate of its own.

## Choice

`async-hid`, chosen by the maintainer on 22 November 2026, with `futures-lite` 2.6.1
(Apache-2.0 OR MIT) to run its futures with `block_on` and to walk the stream of devices, also
approved that day. `async-hid` already brings `futures-lite`. Both are confined to the module
`hid` of `platter-device-io`; no type of theirs is public.

Each open device is read by a thread of its own, `platter-hid`, which waits for the next report
and puts it in a bounded queue, as a MIDI port does. A read that waits cannot be cut short, so a
device that is let go stops being read at its next report or error.

The mappings do not read the report descriptor: a binding names the report, the byte and the bit
(`docs/behavior/hid-controllers.md`), so no descriptor parser is written for now.

## How it was measured

`cargo deny check` passes with both crates. On Windows x64, listing the HID devices of the
system found eight interfaces. Opening a device and reading its reports was not tried with a
controller: no HID DJ controller was at hand. The bindings were tested with synthesised reports.