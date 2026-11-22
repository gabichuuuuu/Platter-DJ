# HID controllers

Many DJ controllers talk to the computer as USB HID devices instead of, or beside, MIDI. This note
covers how a mapping names the controls of a HID device, how Platter reads its reports and how they
become commands. Mappings of MIDI controllers, whose file this extends, are in
`docs/behavior/controllers.md`.

## What the public sources say

- **The public manual**, chapter on configuring a MIDI controller, sections *Device Setup Section*
  and *Device Mapping Section*, and *Preferences*, *Controller Manager page*, read on
  22 November 2026: the controller manager holds mappings for the computer keyboard, MIDI
  controllers and some HID devices, and learn mode works with HID controls such as the keyboard and
  the mouse. It offers no generic HID mapping: the HID devices it maps are those of its own vendor,
  with mappings it ships.
- **Device Class Definition for HID 1.11**, USB Implementers Forum,
  <https://www.usb.org/sites/default/files/hid1_11.pdf>, read on 22 November 2026: a device sends
  its data in input reports (section 5.6, *Reports*); a device that sends several kinds of report
  on one endpoint puts a one-byte report ID before the data of each (section 5.6); multibyte values
  are little-endian (section 5.8, *Format of Multibyte Numeric Values*).

## What Platter does

A HID device is mapped by the same TOML file as a MIDI controller, its bindings naming a part of a
report with `hid` in the place of `midi`. Platter reads the raw input reports of the device; it does
not read the report descriptor, so the mapping says where each control is.

```toml
[[binding]]
hid = "bit 1 0 3"
command = "deck.play_pause"
deck = "a"

[[binding]]
hid = "word 1 4"
command = "mixer.fader.set"
deck = "a"
param = "position"

[[binding]]
hid = "byte 1 6"
control = "encoder"
encoding = "counter"
command = "browser.move"
param = "delta"
```

- `hid` names a control by its report, its byte and, for a key, its bit:
  - `bit R B N`: bit `N` (0 the lowest, to 7) of byte `B` of report `R`. A button: set is down.
  - `byte R B`: byte `B`, 0 to 255. A fader, or an encoder with `control = "encoder"`.
  - `word R B`: bytes `B` and `B + 1`, little-endian, 0 to 65 535. A fader.
- `R` is the report ID, 1 to 255, and `B` counts from 0 at the first byte after it. `R` = 0 is a
  device that numbers no report: `B` counts from the first byte of the report. A report whose ID is
  not that of a binding does nothing for it.
- A control acts only when its part of the report changes from the last report with the same ID. The
  first report of a device after it opens counts as a change from all zeros, except for an encoder
  counter, which takes it as its starting value.
- **Buttons** follow the MIDI buttons: `hold`, `release`, modifiers and `when` mean the same.
- **Faders**: the value over its maximum (255 or 65 535) becomes a number from 0 to 1, or from the
  bottom to the top of `range`; `invert` turns it upside down.
- **Encoders** on a byte: `encoding = "7f-01"` (the default) reads 1 to 127 as steps up and 255 down
  to 129 as steps down, 255 being one; `"3f-41"` reads 129 and above as steps up, 129 being one, and
  127 and below as steps down, 127 being one (0 and 128 are no step); `"counter"` reads a position
  that counts up and down and goes round, so a step is the difference from the previous report,
  from -128 to 127. `param`, `step`, `up` and `down` mean the same as for MIDI encoders, with at most
  16 commands for one report.
- A mapping may hold MIDI and HID bindings; the MIDI ones act on MIDI messages and the HID ones on
  reports.

### Devices

- Platter lists the HID devices of the system with their name, vendor and product numbers and
  the usage page and usage of the interface: a controller often shows several interfaces. It
  opens the first one that matches all five.
- The reports of an open device wait in a queue of their own, in the order they arrived; when the
  queue is full a report is counted and lost. A report longer than 1 024 bytes is cut there.
- Reading runs on a thread of its own, never on the audio thread or the interface thread.
- A device that cannot be found or opened is an error naming it. A device that is unplugged while
  open stops sending, and its queue says it is gone.
- A device that is let go stops being read at its next report or error: a read that waits for the
  device cannot be cut short (`docs/decisions/0044-hid-devices.md`).

## Differences

- **No shipped mappings.** Platter ships no mapping for any HID device: the layout of the reports of
  a controller is the vendor's, and Platter only maps a device from a public description or from what
  its user finds. Mappings are written by hand.
- **No learn mode for HID yet**, no HID output (lights and screens), and no reading of the report
  descriptor.
- **The application does not open HID devices yet.** This version reads devices and turns reports
  into commands; choosing a HID device in the controller manager comes with its page.

## Edge cases

- A report shorter than a binding needs leaves that binding as it was.
- A binding with a bit above 7, or `word` on a report ID of 0 at byte 1 023, is a mistake of the
  file and refused with its number.
- An encoder counter that jumps by more than 127 in one report counts the short way round.
- A fader binding on a `bit` control is a mistake; `encoding` on a `word` is a mistake.

## Tests

`cargo test -p platter-control --test hid` (the bindings and reports) and
`cargo test -p platter-device-io` (the queue of reports), with synthesised reports only. Listing
the devices was tried on Windows x64; opening and reading a real controller was not.
