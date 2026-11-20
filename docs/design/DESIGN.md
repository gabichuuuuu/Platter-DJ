# Platter design system

Platter is an open-source DJ application written in Rust. This design system defines how its desktop interface looks: one dark theme, four deck identities, a frequency-coloured waveform and controls built for long sessions in low light.

`tokens.toml` in this folder is the single source of colours, sizes and text styles. The UI crate (`platter-ui-egui`) reads colours, sizes and type from that table only; a test fails if a panel uses a colour outside it.

## What is in this folder

| Path | Content |
| --- | --- |
| `DESIGN.md` | This file: principles, rules, icons, reference screens and every component |
| `tokens.toml` | Colours, spacing, radii, strokes, control sizes, the font files and the text styles |
| `icons/icons.toml` | 43 original icons as shape data |
| `logo/platter-mark.toml` | The Platter mark as shape data |
| `fonts/` | Geist in four weights and its licence |
| `screens/` | Eight reference screens as static HTML, one per file |

This folder holds no image files on purpose: icons and the mark are shape data, and the screens are HTML. Nothing here carries embedded metadata.

## How to use it

- Read this file to the end before building any screen. Then open the screens of the milestone you are building.
- The screens fix the layout, the hierarchy and which control lives in which panel. Sample data, exact pixel positions and the schematic icons
  inside the screens are not normative: sizes come from `tokens.toml`, icons from `icons/icons.toml`.
- If a screen and the feature inventory (`docs/parity.toml`) disagree about the milestone of a function, the inventory wins.
- A component section may describe states that arrive in a later milestone; each one marks them with the milestone.
- Do not restyle: no new colours, radii, shadows, gradients or typefaces. A need that the tokens do not cover is raised as a question, not solved locally.

## Principles

1. **Familiar workflows, own identity.** Platter follows the conventions DJs already know from professional DJ software: a status header, the global section, waveforms, decks on each side, the mixer in the centre and the browser below. Colours, icons, control shapes and type belong to Platter. A screenshot of Platter must never be mistaken for another product.
2. **Graphite hardware, lit sparingly.** The interface is a matte graphite faceplate: square panels, tight seams, keys that light an LED strip instead of changing colour. Colour appears only where it tells the DJ something: which deck, how loud, where the loop is. The waveforms are the one loud element.
3. **Readable in a dark booth.** Text contrast is at least 4.5:1 everywhere, control outlines and focus rings at least 3:1. Numbers keep a fixed width per digit so readouts do not jitter while they change.
4. **Keyboard first.** Every control can be reached and operated without a mouse, and the focused control is always visible.
5. **Never colour alone.** Deck colours come with the deck letter, the clip colour with the CLIP label, waveform bands with a legend in settings.

## Colour

All colours live in the `[color]` table of `tokens.toml`. There is no light theme; the table is central so that further themes can be added later.

| Role | Tokens | Rule |
| --- | --- | --- |
| Surfaces | `bg-app`, `surface-panel`, `surface-control`, `surface-hover`, `surface-pressed`, `surface-selected` | Neutral graphite, no colour tint. Panels are square and sit 2px apart on `bg-app`, like the seams of a faceplate. Slots, fields and waveform lanes are cut back to `bg-app`. |
| Lines | `line-divider`, `line-key`, `line-control` | `line-divider` for rules, `line-key` for the quiet outline of keys and pads, `line-control` (3.3:1 or more) for fields, caps and scale ticks. |
| Text | `text-primary`, `text-secondary`, `text-disabled`, `text-on-accent` | Primary for values and titles, secondary for names and units. Disabled is for disabled controls only. |
| Decks | `deck-a` … `deck-d`, each with `-low` and `-high` | One hue per deck. The hue marks the deck's badge, the LED strip of its lit keys and its waveform. Decks C and D arrive in M4 but are fixed now. |
| Waveform | `deck-x-low`, `deck-x`, `deck-x-high`, `playhead` | Colour mode 1: each deck's waveform is drawn in its own hue, and frequency is coded by lightness: lows darkest and outermost, highs lightest and innermost. The bands stay apart in greyscale. |
| Status | `status-ok`, `status-warn`, `status-error` | Health of the system: audio, CPU load, files. Always with a label or the words "Warning:" / "Error:". Never reused as a deck, cue or stem colour. |
| Meter | `meter-ok`, `meter-warn`, `meter-clip` | Aliases of the three status colours, used only by level meters. |
| Hotcues (M3) | `hotcue-1` … `hotcue-8` | One colour per cue slot, always shown with its number. |
| Stems (M4) | `stem-drums`, `stem-bass`, `stem-other`, `stem-vocals` | One colour per stem, always shown with the stem name. |
| Loops and grid (M3) | `loop-region`, `grid-beat`, `grid-downbeat` | Loop overlay with brackets and length label; beat and downbeat lines on the scrolling waveform. |
| Key (M3) | `key-match` | Keys compatible with the master deck in the browser, always with the key name. |
| Track colour tags (M3) | `hotcue-1` … `hotcue-8` | The eight cue colours double as track colour tags. |
| Focus | `focus-ring` | Near-white, distinct from every deck colour. |

The deck, waveform and meter sets were checked with an OKLab colour-vision simulation (protanopia, deuteranopia, tritanopia). Across all pairs, the worst simulated difference is 13.6 ΔE in the deck set, 8.8 ΔE in the status set and 13.1 ΔE in the stem set; for the eight hotcues, which sit side by side, the worst neighbouring pair is 9.0 ΔE. 8 is the target. Deck colours are brighter than a data-chart palette would allow because they mark small UI elements on a very dark ground. Each deck's three waveform steps pass the single-hue lightness check (monotone lightness, visible gaps, lowest step at least 3.3:1 on `bg-app`).

## Type

- **Geist** (`sans`) is the only typeface: labels, titles and numbers. No monospaced face is used anywhere.
- Numbers that change (time, tempo, dB, percent) use the `Readouts` styles and must not change width as their digits change. Where the font's tabular figures (the `tnum` OpenType feature) can be switched on, use them; confirm `tnum` in the bundled Geist file, since the Google Fonts build drops some OpenType features. Where the renderer cannot switch features on (check egui 0.37 before relying on it), the readout widget draws each digit centred in a fixed-width slot measured from the widest digit.

The font files are in `fonts/`: static TrueType files for the four weights in use (400, 500, 600, 700), version 1.3.1, with the licence in `fonts/OFL.txt`. All four carry the `tnum` feature. Geist is published by Vercel under the SIL Open Font License 1.1; the application embeds these files, which need a row in `ATTRIBUTION.md`. Sizes are in px at 100% scale and follow the system text scale. Labels are written in sentence case in the translation files; capitals such as `CUE` are the translated string, not a transformation.

Leave room for translation: Spanish strings often run 20 to 30% longer than English. Buttons size to their label and never truncate it.

## Iconography and logo

- **Icons:** 43 original icons on a 24px grid, as shape data in `icons/icons.toml` (see Icons below): transport glyphs are solid, the rest are 1.5px strokes. They take the colour of the text beside them and always come with a label or an accessible name.
- **Logo:** the Platter mark, a record platter seen from above, in a single ink, as shape data in `logo/platter-mark.toml`.
- Every icon and the mark are original work licensed with the project and listed in `ATTRIBUTION.md`.

## Spacing, radius, stroke, size

- Spacing is a six-step scale from 2px to 24px. Panels pad with `space-4`; control groups sit `space-5` apart.
- Corners are tight: `radius-sm` for slots and meters, `radius-md` for keys, fields and caps, `radius-lg` for pads and tooltips. Panels are square.
- Lines are 1px (`stroke-hairline`) except the focus ring and the playhead (2px).
- Control sizes are in the `size` family. Buttons are 28px high: Platter is a mouse-and-keyboard desktop application, not a touch interface.

## Focus and states

Every interactive component shows five states: rest, hover, pressed, focused and disabled.

- **Focus:** a 2px `focus-ring` outline, offset by `space-1`. It draws on top of any other state and never changes the control's size.
- **Lit keys:** a key that is on keeps its graphite body, steps up to `surface-pressed` and lights a 3px LED strip (`stroke-led`) along its top edge: the deck colour for deck functions, `loop-region` for loops, `text-primary` for global toggles. Colour never floods a control, so the waveforms stay the brightest thing on screen.
- **Disabled:** no fill, `line-key` outline, `text-disabled` label.
- **Labels:** sentence case ("Gain", "Key lock", "Set marker"). Only the transport words DJs know as words are capitals: CUE, CUP, SYNC, MASTER.

## Mapping to egui

| Token | egui |
| --- | --- |
| `bg-app` | `Visuals::window_fill`, `Visuals::panel_fill` behind panels |
| `surface-panel` | `Frame::fill` of each panel |
| `surface-control` / `surface-hover` / `surface-pressed` | `WidgetVisuals::bg_fill` of `inactive` / `hovered` / `active` |
| `line-key` | `WidgetVisuals::bg_stroke` of keys; `line-control` for fields |
| LED strip | a filled rect of height `stroke-led` painted over the key's top edge |
| `text-primary` | `WidgetVisuals::fg_stroke`, `Visuals::override_text_color` |
| `focus-ring` + `stroke-focus` | painted by each widget when `Response::has_focus()` |
| `radius-*` | `CornerRadius` |
| Knobs, faders, meters, waveforms | custom widgets drawn with `Painter`, reporting `WidgetInfo` for accessibility |

Gradients, blur and soft shadows are not part of the system: egui draws solid fills and strokes natively, and the system stays within that.

## Components by milestone

The first complete version is milestone M3; every component it needs has its section under Components, below.

| Area | M2 | M3 |
| --- | --- | --- |
| Layout and global | `StatusHeader` | `MasterClock` |
| Deck | `DeckHeader`, `Waveform`, `TransportButton`, `TempoControl` | `SyncControls`, `LoopControls`, `HotcuePads`, `KeyControl`, and the advanced panel pages `CuePanel`, `MovePanel`, `GridEditor` |
| Mixer | `Knob` (with kill buttons and PAN), `Fader`, `Crossfader`, `Meter` | Crossfader assignment and fades, key knob |
| Browser | `TrackList` (in-app file selector) | `BrowserTree`, `SearchField`, `PreviewPlayer`, full `TrackList` columns |
| Settings | `Select` | `ControllerMapping` |
| Feedback | `Notice` | `Tooltip` |


## Still to define

Everything below belongs to M4 and M5, after the first public version:

- **Waveform colour modes 2 to 4 (M4).**
- **Components for M4:** stem lanes, mix and loop recorders. `screens/M4-effects.html` places them; their component sections are written before they are built. The four-deck layout, the effect units, the effect assignment and the mixer effect have theirs.
- **Components for M5:** remix deck grid, pattern sequencer, timecode setup.
- **Icons for M4 and later:** effects, stems, recording and layouts.

## Icons

Forty-three original icons drawn for Platter, stored as shape data in `icons/icons.toml` and licensed with the project (`MIT OR Apache-2.0`). They need one row in `ATTRIBUTION.md` as original work.

- Grid 24 × 24px. `play` and `pause` are solid glyphs, like the legends on hardware transport keys; every other icon is a 1.5px stroke with round caps and joins.
- The shapes carry no colour. In the app, icons are drawn in the colour of the text beside them: `text-primary`, `text-on-accent` on deck-coloured buttons, `text-disabled` when disabled.
- Icons never stand alone: every icon-only button has an accessible label and a tooltip.

| Icon | Use | Milestone |
| --- | --- | --- |
| `play`, `pause` | Transport | M2 |
| `cue` | Cue button and cue marker | M2 |
| `settings` | Settings button in the status header | M2 |
| `folder`, `file-audio` | File selector rows | M2 |
| `load-left`, `load-right` | Load the selected track into the left or right deck | M2 |
| `warning`, `error` | `Notice` and status indicators | M2 |
| `search` | File selector and browser search | M2 |
| `close`, `chevron-down` | Dismiss, drop-downs | M2 |
| `speaker` | Audio indicator in the status header | M2 |
| `clock` | Clock in the status header | M2 |
| `headphones` | Pre-listen on mixer channels (M2) and the preview player (M3) | M2 |
| `sync` | Sync button | M3 |
| `loop`, `beatjump` | Loop controls and beatjump | M3 |
| `flux`, `reverse` | Flux and reverse toggles | M3 |
| `tap`, `metronome` | Master clock | M3 |
| `quantize`, `snap`, `grid` | Quantize, snap and the GRID page | M3 |
| `key`, `lock` | Key control and analysis lock | M3 |
| `star` | Track rating | M3 |
| `plus`, `minus`, `chevron-left`, `chevron-right` | Steppers and navigation | M3 |
| `playlist`, `history`, `bookmark` | Browser tree and favourites | M3 |
| `midi`, `learn` | MIDI indicator and learn mode | M3 |
| `fullscreen`, `info` | Window and tooltips | M3 |
| `trash`, `check`, `chevron-up` | Delete a cue or a mapping row, played mark and checkboxes, ascending sort | M3 |

### Drawing them in the application

The project has no SVG dependency and this folder holds no image files: `icons.toml` has one table per icon with its shapes as SVG shape
elements (`path`, `circle` and `rect`; path commands `M L H V C S Q A Z`, absolute and relative). `platter-ui-egui` either parses that
subset itself at start-up or takes a table of shapes generated from this file by `xtask`; the choice goes in an ADR. Icons are drawn with
the painter in the colour of the text beside them. The names in the table below are the keys of `icons.toml`.

## Reference screens

Eight screens in `screens/`, drawn at 1440 × 900 (windows) or at the size of the dialog, in the order they get built. All data in them is invented.

Each screen is one static HTML file with no scripts: open it in a browser at 100% zoom to see it, or read its markup, where every colour,
size and text style is written as a token (`var(--surface-panel)`, `var(--radius-md)`) and every control has its label or accessible name.
The files load the fonts from `fonts/`. The waveforms in these files are drawn with coarse bars to keep them small; the real ones follow
the `Waveform` component.

| Milestone | File | What it fixes |
| --- | --- | --- |
| M2 | `M2-main.html` | Main window: status header, two parallel scrolling waveforms with zoom keys, deck A, mixer, deck B, in-app file selector |
| M2 | `Startup-states.html` | No audio device (header state and notice with its action), an empty deck, a deck that is loading a track |
| M2 | `Preferences-audio.html` | Audio page: device, sample rate, buffer size, live status, output routing, test tone |
| M2 | `Preferences-general.html` | General page: language, crossfader curve, read-only shortcut list with access to the keymap file |
| M3 | `First-version.html` | Main window of the first complete version: master clock, sync, key, loops, beatjump, hotcues, advanced panel, full mixer, browser |
| M3 | `Preferences-controllers.html` | Controller manager: mapping, ports, assignment table, learn mode, import and export |
| M3 | `Track-details.html` | Track metadata editor |
| M4 | `M4-effects.html` | Four-deck layout: effect units, layout selector, mix recorder, compact decks with embedded waveforms, a stems deck, four-channel mixer |

### Rules for reading them

- The screens fix the zones, their order, and which control lives in which panel. They do not fix pixel positions: sizes, colours and text
  styles come from `tokens.toml`.
- Where a screen and a component section differ, the screen wins for placement and panel proportions, the component section wins for states, behaviour,
  keyboard and commands.
- The icons inside the screens are schematic. The application uses the shapes in `icons/icons.toml`.
- If a screen and the feature inventory (`docs/parity.toml`) disagree about the milestone of a function, the inventory wins. Do not build in
  one milestone a control that appears only in the screens of a later one.
- Every label in the screens is a translatable string. English is shown; Spanish strings run 20 to 30% longer and must fit.
- Tempo figures are consistent on purpose: the header shows the current tempo, the pitch offset and the base tempo (124.00 is -0.8% of
  125.00). The track list shows base tempo.

### Layout of the main window

Top to bottom, separated by 2px seams of `bg-app`:

1. Status header, 40px.
2. Effect units, 72px (M4 only).
3. Two scrolling waveform lanes, one per deck, playhead fixed at the centre, with the deck badge and zoom keys in a 32px gutter. In the
   four-deck layout the lanes move inside each deck.
4. Deck A, mixer, deck B. Decks are 520px wide at 1440; the mixer takes the rest. Decks C and D sit under A and B in M4
   (*Four-deck layout (M4)* below).
5. Browser: tree on the left (260px), list on the right, to the bottom of the window.

When the window is wider than 1440 the mixer keeps its width and the two columns of decks share the extra width, so the decks reach the
edges of the window; the browser list and the waveform lanes take the whole width. When it is taller or shorter than 900 the browser gains
or loses rows. Windows narrower than 1440 are not designed yet: ask before choosing how the
decks shrink.

### M2: what each zone contains

- **Status header:** name, audio state with device kind, sample rate, latency, audio-thread load, dropout count, master meter, clock, settings.
  No master tempo, no full-screen key, no MIDI indicator (M3).
- **Deck:** badge, title, artist, album, current tempo, pitch offset and base tempo, remaining, elapsed and total time; overview waveform
  (click to jump); CUE, CUP, play or pause; tempo range, tempo fader, pitch readout, nudge keys. The base tempo is read from the file tag
  when there is one, otherwise the readout shows `--.--`: there is no tempo or key analysis before M3, so no key is shown.
- **Mixer:** per channel gain, three EQ knobs each with its kill key, filter, pan, pre-listen key, fader and meter; master meter with scale
  and level; headphone mix and volume; crossfader. With a two-output device the pre-listen keys and headphone knobs are disabled, and the
  audio preferences page says why.
- **Browser strip:** folder tree, breadcrumb, file count, list with title, artist, album, tempo from the tag, duration and format, deck
  badges on loaded files, Load A and Load B. Drag and drop onto a deck also loads.
- **Preferences:** two pages only, Audio and General.

### M3: what `First-version.html` adds

- **Status header:** master tempo with metronome, Auto, Tap, down and up; Snap and Quantize; MIDI indicator; full screen.
- **Waveform lanes:** beatgrid, cue markers with number and name, loop region with its length.
- **Deck:** cover; key readout; SYNC, MASTER and phase meter; key lock and transposition by semitone; loop size with halve and double,
  In, Out, Active; beatjump; eight hotcue pads; advanced panel with the Move, Cue and Grid pages (the screen shows Cue on deck A and Grid on
  deck B; the `MovePanel`, `CuePanel` and `GridEditor` components describe each page in full); Flux and Reverse.
- **Mixer:** autogain key, key knob, crossfader side per channel (L or R), one-step and automatic fades.
- **Browser:** collection, playlists, preparation list, favourites, history, explorer; live search; preview player; cover, colour tag,
  played mark, key in musical notation with compatible keys highlighted, rating; status bar with background analysis.

### M3 functions without a screen

They follow the components below and the pattern of the nearest screen. Ask before inventing a new layout for any of them.

| Function | Build it as |
| --- | --- |
| Compact deck size | The compact deck of `M4-effects.html`, without the embedded waveform when the parallel lanes are visible |
| Tooltips | `Tooltip` component |
| External mixer mode | A setting in Preferences, Mixer page; the internal mixer panel collapses to gain, key and pre-listen per channel |
| Preferences pages Decks, Mixer, Library, Keyboard | Same form pattern as `Preferences-audio.html`. Sync mode (tempo or beat sync), loading rules and key options go in Decks; crossfader curve moves from General to Mixer |
| Playlist management, M3U import and export | Context menu on `BrowserTree` rows and `TrackList` rows; file choices use the in-app file selector |
| Collection maintenance (consistency check, relocate, delete) | A report dialog in the pattern of `Track-details.html`, opened from the Library preferences page |
| Configurable columns | Context menu on the `TrackList` header |
| Loops stored in hotcues, cue types | `HotcuePads` and `CuePanel` components |
| Master clock bend and MASTER | `MasterClock` component |

### M4 and later

`M4-effects.html` covers effect units 1 and 2 in group mode, channel sends, the mixer effect knob per channel, the stems deck, four decks,
the layout selector and the mix recorder. Not drawn: effect units 3 and 4, single mode, the loop recorder, smart playlists, the remaining
layouts, and everything in M5 (remix decks, pattern sequencer, DVS, live input, broadcasting).

### Smart playlist rules (M4)

Approved by the maintainer on 20 November 2026. A dialog in the pattern of `Track-details.html`, 560px wide and as tall as its
rows, opened by *Edit rules* in the menu of a smart playlist of `BrowserTree`.

- Top: *Rules of* and the name of the smart playlist, `title` style; under it the line "A track must pass every rule.", `body`
  in `text-secondary`.
- One row per rule, 32px high, 8px apart, up to 16, which fit in the window at 1440 × 900. Each row: a `Select` of the field,
  140px wide (Title, Artist, Album, Genre, Tempo, Length, Rating, Plays, Imported, Last played, Colour, Key, Missing,
  Analysed); then its value; then a key of 28 × 28 with the `close` icon that removes the row.
- The value: a form field for words; two form fields, *From* and *to*, with the unit after them (BPM, s, `YYYY-MM-DD`) for a
  range; a `Select` of the eight colours or of the 24 keys in the key notation of the preferences; the segmented control of
  the preferences with *Yes* and *No* for the missing file and the analysis.
- A row whose value means nothing is outlined with a hairline in `status-warn`.
- Under the rows, *Add rule*, a text key, disabled at 16 rules.
- Foot: *Cancel* and *Save*, as in `Track-details.html`. *Save* is disabled until every row holds a rule.
- A smart playlist has the `playlist` glyph in the tree; no icon of its own.

### Four-deck layout (M4)

Measured on `M4-effects.html` at 1440 × 900. Zones top to bottom, separated by 2px seams of `bg-app`:

1. Status header, 40px, as in M3.
2. Effect units row, 72px: FX 1 on the left (520px, over decks A and C), FX 2 on the right (520px, over decks B and D) and, in the
   396px between them over the mixer, the mix recorder (143px: Rec, elapsed time, Cut). The screen also draws the layout selector
   there; it lives in Preferences instead (*Decided on 18 November 2026* below). Until the effect units exist the row is left out
   and the browser takes its height.
3. Decks band, 580px: left column 520px with deck A on top and deck C below, the mixer 396px in the middle, right column 520px with
   deck B on top and deck D below. Each deck is 289px tall.
4. Browser, from the bottom of the decks band to the bottom of the window (202px at 900), as in M3.

There are no parallel waveform lanes in this layout: each deck carries its own waveform.

**Deck in the four-deck layout.** 10px padding. Header 40px: badge, cover, title with its type tag (`Track`, `Stems`), artist and
album on one line, tempo with pitch offset and key, remaining time over total time. Body 221px: a 442px main column and the tempo
column at the right (range, fader, pitch readout, nudge). The main column holds, top to bottom: the waveform with the stripe under it
(88px; a stems deck shows its four stem lanes there instead), the SYNC, MASTER, phase and key row (24px), the transport and loop row
(30px: play, CUE, CUP, Flux, reverse, loop size, In, Out, Active) and the eight hotcue pads (26px). No advanced panel and no beatjump
row in this size.

**Mixer in the four-deck layout.** Four channels of 75px on a 79px step, in the order A, B, C, D, then the master column (60px: master
meter and level, limiter, phones mix and volume). Each channel keeps the M3 controls (gain with Auto, three bands with Kill, filter,
key, pan, pre-listen, fader, L and R) and adds the M4 effect controls (sends 1 and 2, the mixer effect knob). The crossfader row
(30px) spans the four channels under them, with the step keys and Auto at each end.

**Decided on 18 November 2026:**

- The layout is chosen in Preferences, General page, Layout section: `Decks`, a segmented choice of `Two` and `Four`, above the deck
  size. It is kept in the settings file. Four is the default (decided later the same day, with the channel order A, B, C, D in
  place of the C, A, B, D the screen draws). The two-deck layout keeps the advanced panel and the parallel lanes; the four-deck layout
  has neither.
- Loading: in the four-deck layout the browser draws Load A, Load B, Load C and Load D, left to right; dragging a track onto any of
  the four decks also loads it.
- The crossfader starts with deck C on the left and deck D on the right.
- Windows wider than 1440 and full screen: the mixer keeps its 396px, the two deck columns share the extra width, and the browser
  takes the extra height.

**Effects, decided on 18 November 2026:**

- The effect row is in both layouts, 72px, under the status header. Each unit panel takes the width of the deck column under it and
  the middle panel keeps the 396px of the mixer, so a wider window widens the units, by the rule of the decks above.
- Single mode, which the screen does not draw, takes the same panel (`EffectUnit` below).
- Units 3 and 4: a preference of two or four units on the Mixer page. With four, the header of each panel switches between its two
  units (`1 | 3` on the left, `2 | 4` on the right).
- The mixer effect knob is one-sided, 0 to 1, off at 0. Its effect is chosen by clicking its name. The catalogue has no filter,
  since every channel has one: delay, reverb, gater and bitcrusher; a channel starts on the reverb. The *Filter* the screen shows on
  channel C stands for that channel's choice.
- The effect that repeats slices of a bar is named `Slicer`.
- No send routing in M4: units are insert or post fader. The send through an external mixer waits for input routing (M5).

## Components

One section per component: anatomy, states, behaviour, keyboard and the commands it emits. Grouped by zone.

### Global

#### StatusHeader

The strip across the top of the window: Platter's name, live status indicators, the master level and quick access to settings.

##### Anatomy (left to right)

- Product name in the `title` style, `text-primary`.
- Indicators, each an icon-free label plus a value in the `readout` style:
  - **Audio:** device state. `status-ok` dot with "Audio" when running, `status-error` dot with "No audio" when the device is missing.
  - **CPU:** audio thread load in percent; the value turns `status-warn` above 70%.
  - **Dropouts:** count since start; `status-warn` when above zero. Clicking it resets the count.
  - **MIDI** (M3) and **Rec** (M4) indicators join the same row later.
- Master level: a horizontal `Meter`, 120px long.
- Clock: system time, `readout` style.
- The settings button at the right end. The layout of the decks is chosen in Preferences, not here (*Four-deck layout (M4)*).

Height 32px, `surface-panel` fill, `line-divider` bottom border, `space-4` side padding, indicators `space-5` apart.

##### Behaviour

- Status dots never carry meaning alone: every dot has its label.
- The settings button emits `app.settings.open`; the dropout counter emits `audio.dropouts.reset`.
- Values come from the state snapshot and refresh at the UI frame rate.

#### MasterClock

The global tempo and timing panel at the top centre, above the mixer (milestone M3).

##### Anatomy

- Tempo readout in the `tempo` style with the word "Master" in the `label` style; double-click to type a tempo.
- **Auto / Master** segmented control: automatic tempo master, or the clock as master.
- **Tap**, tempo down and up, and bend slower and faster keys.
- **Metronome** toggle with the `metronome` icon.
- **Quantize** and **Snap** toggles, lit with a `text-primary` LED strip when on.
- Main volume uses `Knob`; the limiter toggle sits beside it with the label "Limiter".

##### Behaviour

- Quantize keeps jumps (cues, loops, beatjumps) on the beat; Snap places new cues and loops on the nearest beat.
- Commands: `clock.mode.set`, `clock.tap`, `clock.tempo.step`, `clock.bend`, `clock.metronome.toggle`, `global.quantize.toggle`, `global.snap.toggle`, `master.limiter.toggle`.

#### Notice

Inline message for states the DJ must know about: no audio device, a track still loading, a file that cannot be read.

##### Anatomy

- Box: `surface-control` fill, `radius-md`, `space-3` padding, no outline.
- Marker: the `error` or `warning` icon, 16px, in its status colour, or three small level bars in `text-secondary` for loading.
- Text: what happened in bold, then how to fix it, in the `body` style ("No audio output. Platter can't find a device to play through.").
- Optional action: a `TransportButton`-styled button on the right ("Open settings", "Retry").

##### Behaviour

- Shown where the problem is: inside the deck for a loading or unreadable track, under the status header for audio problems.
- Errors stay until fixed or dismissed; loading notices disappear when the track is ready.
- Screen readers announce errors and warnings when they appear (`Role::Alert`).
- Loading never blocks the interface: the deck shows the notice while the rest of the app keeps working.

#### Tooltip

Short help on hover or focus (milestone M3), enabled by default and switchable in Settings.

##### Anatomy

- `surface-pressed` box, `radius-md`, `space-2` × `space-3` padding, max 240px wide.
- Text in the `body` style; the keyboard shortcut after it as a small key cap on `bg-app`.
- No arrow; it appears 8px below the control, or above when there is no room.

##### Behaviour

- Appears after 600 ms of hover or immediately on keyboard focus; disappears on Escape or when the pointer leaves.
- Every icon-only button has a tooltip with its name; disabled controls explain why.

#### Select

Drop-down choice used in Settings: audio device, sample rate, buffer size, language and crossfader curve when space is short.

##### Anatomy

- Field: 28px high, `surface-control`, 1px `line-control` outline, `radius-md`; value in `body` style `text-primary`; a small chevron drawn as a 1.5px stroke on the right.
- Label above in the `label` style, `text-secondary`, `space-2` gap.
- Open list: `surface-control` panel with `line-control` outline, rows 28px, hover `surface-hover`, current value `surface-selected`.
- Help or error text below in `readout` style: `text-secondary` for help, `status-error` with "Error:" prefix for errors.
- Focus: 2px `focus-ring` around the field.

##### Behaviour

- Space, Enter or Alt+Down opens; Up/Down move; Enter picks; Escape closes without change.
- Changing the audio device, sample rate or buffer emits `audio.device.set`, `audio.sample_rate.set`, `audio.buffer.set` and is saved to the settings TOML.
- Unavailable options (a sample rate the device does not support) show in `text-disabled` and cannot be picked.

### Deck

#### DeckHeader

Top of each deck: deck letter, track title and artist, tempo and the elapsed and remaining time.

##### Anatomy

- Deck letter: 18 × 18px badge filled with the deck colour, letter in `text-on-accent`.
- Title: `title` style, `text-primary`, one line, truncated with an ellipsis (the full title shows on hover and to screen readers).
- Artist: `body` style, `text-secondary`, one line.
- Tempo: `tempo` style in `text-primary`; below it the pitch offset in `readout` style (`+2.5%`).
- Time: remaining time in the `time` style (`-03:41.2`), elapsed time below in `readout` style. In the last 30 seconds the remaining time turns `status-warn`.
- Empty deck: title "No track loaded" in `text-secondary`, times shown as `--:--`.

Layout: `surface-panel`, `space-4` padding, letter and text on the left, numbers right-aligned on the right.

##### Behaviour

- Clicking the time toggles remaining/elapsed as the large value (`deck.time_display.toggle`).
- Mirrored for the right-hand deck: letter on the outer edge, numbers on the inner edge, so both decks read from the mixer outward.

##### Additions

- **Album** (M2): third line in the `body` style, `text-secondary`; hidden in the compact deck.
- **Cover** (M3): 40 × 40px image with `radius-sm` to the left of the title; a `file-audio` icon on `surface-control` when the file has none.
- **Key** (M3): the resulting key from `KeyControl` under the tempo.
- **Compact deck** (M3): one line with letter, title, tempo and remaining time; the full deck adds artist, album, cover and elapsed time.

#### Waveform

Frequency-coloured waveform in two forms: the scrolling view centred on the playhead (top of the screen, one lane per deck) and the overview of the whole track inside each deck.

##### Anatomy

- Ground: a lane cut back to `bg-app` inside the `surface-panel`, `radius-sm` corners.
- Three nested layers per column in the deck's own hue, drawn back to front: `deck-x-low` (bass, outermost), `deck-x` (mids), `deck-x-high` (treble, innermost). Each layer's height is that band's peak in the column, mirrored around the centre line.
- Playhead: a 2px `playhead` line, fixed at the centre of the scrolling view and moving across the overview.
- Lane marker: the deck's letter badge at the left of each scrolling lane. The lane's hue already says which deck it is; the badge confirms it.
- Playhead ends: small `playhead` triangles at the top and bottom edge.
- Overview: the played part is dimmed with a 55% `bg-app` veil; cues and the loop keep their colours.

##### Behaviour

- Peaks come from the three-band peak data computed when the track loads; the UI never analyses audio itself.
- Clicking the overview emits `deck.seek` with the clicked position.
- Colour mode 1 (deck hue, lightness by band) is the only mode in M2. Modes 2 to 4 (M4) swap the three tokens, not the drawing code.
- Settings shows a legend that names each band, so the colours are never the only key.

##### Markers (M3)

- Beatgrid: beats as 1px `grid-beat` lines behind the waveform, downbeats and grid markers as 1px `grid-downbeat` lines over it, on the scrolling view only.
- Cues: a 2px line in the slot's `hotcue-N` colour with a 12px flag at the top carrying the slot number; the floating cue is a `playhead` triangle.
- Active loop: a `loop-region` overlay at 22% opacity with bracket lines at both ends and the length label.
- The overview shows cues and the loop with the same colours, without grid lines.
- Parallel layout (M3): the two scrolling lanes stack with the same time scale so beats line up vertically.

#### TransportButton

The PLAY, CUE and CUP keys of each deck, and the model for every key (button) in Platter.

##### Anatomy

- Key: 64 × 36px (`size-button-transport` × `size-transport-h`); other keys are 28px high. `surface-control` fill, 1px `line-key` outline, `radius-md`.
- PLAY shows the solid `play` glyph, or `pause` while playing; CUE and CUP are words in the `button` style.
- Hover: `surface-hover`. Pressed: `surface-pressed`.
- **Lit** (playing, cue held): `surface-pressed` body and a 3px LED strip along the top edge in the deck colour. The glyph stays `text-primary`.
- Focus: 2px `focus-ring`, offset 2px, drawn over every other state.
- Disabled (no track): no fill, `text-disabled` glyph.

##### Behaviour

- Emits a command, never changes state itself: `deck.play_pause`, `deck.cue`, `deck.cup`.
- Space and Enter activate the focused key. Default shortcuts come from the keymap table.
- Width grows with a longer translated label; it never truncates.
- The glyph-only PLAY key has the accessible name "Play" or "Pause" and a tooltip.

##### egui

A custom widget: `allocate_exact_size(size, Sense::click())`, paint with `Painter`, then `response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label))`.

##### More transport keys

- **CUP** (M2): plays from the cue while held.
- **Flux** and **Reverse** (M3): toggles with the `flux` and `reverse` icons, lit with a `text-primary` LED. With flux on, a dimmed `playhead` line shows where the track would be.
- Commands: `deck.cup`, `deck.flux.toggle`, `deck.reverse.toggle`.

#### TempoControl

Tempo fader with its range, a centre detent and the two nudge buttons, on the outer side of each deck.

##### Anatomy

- Fader: vertical, same track and cap as `Fader`, 120px. Centre tick in `line-control` marks 0%.
- Range label above: `±8%` in `readout` style; clicking it cycles the range (±8, ±16, ±50%).
- Nudge buttons below: two 28 × 28px buttons labelled `−` and `+`, styled like `TransportButton`. While held they fill with the deck colour.
- Readout: the current offset (`+2.5%`) under the fader in `readout` style.
- Focus: 2px `focus-ring` around the cap or the focused button.

##### Behaviour

- Direction: up is faster by default. A setting inverts it for DJs used to the turntable convention, where moving the slider down raises the tempo.
- Keyboard: Up/Down in 0.05% steps, Shift for 0.01%, Home resets to 0%.
- Nudge is temporary: it bends the tempo only while held (`deck.nudge`, with direction).
- Emits `deck.tempo.set`, `deck.tempo.range.cycle`, `deck.nudge`.

#### SyncControls

SYNC and MASTER buttons and the phase meter of each deck (milestone M3).

##### Anatomy

- **SYNC**: a key. On: lit with the deck-colour LED strip.
- **MASTER**: a key, lit the same way. Only one of the two is lit on a deck at a time, and the caption beside the phase meter says "Tempo master" for the master deck.
- **Phase meter**: a 120 × 6px slot in `bg-app` with a centre tick in `line-control`; a bar in the deck colour grows left (behind) or right (ahead) from the centre. In sync the bar disappears.
- Order: SYNC, MASTER, then the phase meter on the inner side of the deck.

##### Behaviour

- SYNC aligns tempo (Tempo Sync) or tempo and phase (Beat Sync), as set in Preferences.
- MASTER makes the deck the tempo master; with AUTO in the master clock, the master is chosen automatically and the button only shows the state.
- Commands: `deck.sync.toggle`, `deck.master.set`.

#### LoopControls

Loop size, loop in and out, the Active key and the beatjump keys in each deck (milestone M3).

##### Anatomy

- Size display: the loop length in beats (`1/32` … `32`) in the `tempo` style, with **‹** and **›** to step through sizes. Out-of-range steps flash the arrow in `status-warn` once.
- **In** and **Out** keys; Out sets the end and activates the loop.
- **Active**: lit with a `loop-region` LED strip while a loop plays.
- Beatjump: **«** and **»** with the jump size shown between them; the size follows the MOVE page.
- On the waveforms the active loop is a `loop-region` overlay at 22% opacity with bracket lines at both ends and the length label.

##### Behaviour

- With Quantize on, In, Out and jumps land on the beat.
- Keyboard: L toggles the loop, `[` and `]` halve and double the size, comma and full stop jump back and forward.
- Commands: `deck.loop.size.step`, `deck.loop.in`, `deck.loop.out`, `deck.loop.toggle`, `deck.beatjump`.

#### HotcuePads

Eight hotcue buttons per deck (milestone M3), the on-screen counterpart of a controller's pads.

##### Anatomy

- 2 rows × 4 pads, each 76 × 44px, `radius-lg`.
- **Set**: graphite body (`surface-control`) with a 5px strip of the slot's `hotcue-N` colour along the top, the slot number in that colour and the cue name below in `text-primary`. The type shows in the name's default ("Fade in", "Loop 8") until the DJ renames it.
- **Empty**: no fill, 1px `line-key` outline, slot number in `text-secondary`.
- **MAP mode**: every pad shows a dashed `focus-ring` outline until one is chosen.

##### Behaviour

- Press an empty pad to store a cue (or the active loop) at the playhead; press a set pad to jump; Shift+press deletes.
- Keyboard: 1–8 trigger the pads of the focused deck.
- Commands: `deck.hotcue.trigger`, `deck.hotcue.delete`, `deck.hotcue.map`.

#### KeyControl

Key lock and transposition per deck (milestone M3), and the key readout in the deck header.

##### Anatomy

- **Key lock** toggle: on state is `surface-pressed` with a `text-primary` outline and the `lock` icon.
- Transpose: **−** and **+** with the offset in semitones (`+2 st`) in the `readout` style; **Reset** returns to the original key.
- Key readout: original key and resulting key in musical notation, `A min → B min`, in `text-primary`; the resulting key is shown only when it differs.
- The mixer channel's key knob is a `Knob` with the same readout.

##### Behaviour

- Without key lock, tempo changes move the pitch, as on a turntable.
- Notation is plain musical notation; no third-party trademarked key notations.
- Commands: `deck.keylock.toggle`, `deck.key.transpose`, `deck.key.reset`.

#### CuePanel

The CUE page of the deck's advanced panel (milestone M3): edit the stored cue points of the loaded track.

##### Anatomy

- Previous and next cue buttons (**‹** **›**).
- Cue name field (`body` style) with the slot colour as a 12px square before it.
- Type `Select`: Cue, Fade in, Fade out, Load, Grid, Loop.
- Position readout in the `readout` style (`01:12.480`).
- **STORE**, **MAP** and **Delete** buttons; Delete uses the `error` icon and asks for no confirmation because Undo is available.

##### Behaviour

- Selecting a cue in the list or with **‹ ›** moves the playhead to it.
- STORE saves the floating cue to the next free hotcue; MAP enters the mapping mode of `HotcuePads`.
- Commands: `deck.cue.select`, `deck.cue.rename`, `deck.cue.type.set`, `deck.cue.store`, `deck.cue.map`, `deck.cue.delete`.

#### MovePanel

The MOVE page of the advanced panel (milestone M3): move the playhead, the loop or its ends by a number of beats.

##### Anatomy

- Mode segmented control: **Beatjump**, **Loop**, **Loop in**, **Loop out**.
- Size: `‹ size ›` in beats (`1/32` … `32`), or **Loop size** to follow the current loop length.
- **«** and **»** to move backward and forward; **Fine** toggle for small steps.

##### Behaviour

- Commands: `deck.move.mode.set`, `deck.move.size.step`, `deck.move` (with direction), `deck.move.fine.toggle`.

#### GridEditor

The GRID page of the advanced panel (milestone M3): correct the tempo and the beatgrid of a track.

##### Anatomy

- BPM field in the `tempo` style, editable by typing or dragging; `Lock` toggle with the `lock` icon beside it.
- **TAP**, **×2**, **÷2**, **−** and **+** (tempo of the current zone).
- Marker row: previous marker, **Set marker**, next marker, **Delete marker**.
- **Reset grid**, **Reset BPM** and **Beat tick** (audible click) toggles.
- On the waveforms, beats are `grid-beat` lines and downbeats and markers `grid-downbeat` lines.

##### Behaviour

- While Lock is on, analysis and edits cannot change BPM, grid or cues; the controls show as disabled.
- Commands: `deck.grid.bpm.set`, `deck.grid.tap`, `deck.grid.bpm.scale`, `deck.grid.bpm.step`, `deck.grid.marker.*`, `deck.grid.reset`, `deck.grid.lock.toggle`, `deck.grid.tick.toggle`.

### Mixer

#### Knob

Rotary control for gain, the three EQ bands, pan and the filter: a 34px cap inside a value arc, with its name and value below.

##### Anatomy

- Cap: 34px disc, `surface-control` fill, 1px `line-key` outline, with a 2px `text-primary` pointer line near its edge.
- Track: a 270° arc, 2px, `line-key`, from 7:30 to 4:30 o'clock, 7px outside the cap.
- Value arc: 2px `text-primary`, from the centre detent (EQ, pan, filter) or from the minimum (gain). Centre-detent knobs carry a small `line-control` tick at 12 o'clock.
- Name in the `label` style, `text-secondary`; value in the `readout` style, `text-primary`, always visible.
- Focus: 2px `focus-ring` circle around the cap.

##### Behaviour

- Vertical drag changes the value; Shift for fine steps; double-click returns to the default.
- Keyboard: Up/Right increase, Down/Left decrease, Shift for fine steps, Home for the default.
- EQ kill: at the minimum the value arc disappears and the readout shows `KILL`.
- Filter: left of centre is low-pass, right is high-pass; the centre detent is off.
- Emits `mixer.<channel>.eq.<band>.set`, `mixer.<channel>.filter.set`, `mixer.<channel>.gain.set`.

##### Mixer additions

- **Kill keys** (M2): an 18px-high key labelled "Kill" under each EQ knob, lit with a `text-primary` LED while the band is cut. Emits `mixer.<channel>.eq.<band>.kill.toggle`.
- **PAN** (M2): a centre-detent knob like the filter, labelled `PAN`.
- **Key** (M3): a centre-detent knob in steps of one semitone, with the `KeyControl` readout.

#### Fader

Vertical channel fader: a 120px track with a cap, the channel meter beside it and the deck letter on top.

##### Anatomy

- Slot: 4px wide, cut to `bg-app` with a `line-key` outline; 1px `line-key` scale ticks every 12px beside it and a longer `line-control` tick at unity.
- Cap: 26 × 16px, `surface-pressed` fill, 1px `line-control` outline, `radius-md`, and a 2px centre line in `text-primary`.
- The deck's letter badge above the slot; the level in dB below, in the `readout` style.
- Meter: the `Meter` component, `space-3` to the right of the track.
- Focus: 2px `focus-ring` around the cap.

##### Behaviour

- Drag the cap or click the track to jump; double-click returns to unity (0 dB at 75% travel).
- Keyboard: Up/Down in 1% steps, Page Up/Page Down in 10% steps.
- Emits `mixer.<channel>.fader.set`.

#### Crossfader

Horizontal fader that blends deck A (left) into deck B (right): a 160px track, a cap and the two deck letters at its ends.

##### Anatomy

- Track: 4px high, `surface-control`, `radius-sm`, with a centre tick in `line-control`.
- Cap: 14 × 28px, same fill, outline and centre line as the channel fader cap.
- Deck letters at each end in their deck colour.
- Curve selector below: a segmented control with the curve names from the translation files.
- Focus: 2px `focus-ring` around the cap.

##### Behaviour

- Drag or click the track; double-click returns to the centre.
- Keyboard: Left/Right in 1% steps, Page Up/Page Down in 10% steps, Home to the centre.
- Emits `mixer.crossfader.set` and `mixer.crossfader.curve.set`.

##### Assignment and fades (M3)

- Under each channel fader, a three-way `seg` control: **A** (left), **Thru**, **B** (right) assigns the channel to a side of the crossfader.
- Beside the crossfader: **‹** and **›** move it one step; **Auto fade** moves it to the other side over a set number of beats.
- Commands: `mixer.<channel>.xfader_assign.set`, `mixer.crossfader.step`, `mixer.crossfader.auto_fade`.

#### Meter

Stereo peak meter for each channel and the master: two 4px columns of 30 fine segments with a clip indicator on top.

##### Anatomy

- Segments: 3px high with 1px gaps, left and right columns 2px apart; unlit segments in `surface-control`. A scale (0, 6, 12, 24, 48 dB below full scale) sits beside the master meter.
- Lit colours by level: `meter-ok` below -6 dBFS, `meter-warn` from -6 to 0 dBFS, `meter-clip` above 0 dBFS.
- Clip indicator: a `meter-clip` cell above the column plus the `CLIP` label in `readout` style; it holds for 2 seconds or until clicked.
- Peak hold: one segment stays lit for 1 second at the last peak.

##### Behaviour

- Read-only; the clip indicator is the only interactive part (`mixer.<channel>.clip.reset`).
- The UI reads levels from the state snapshot; the meter never touches the audio thread.
- Colour is never the only signal: the `CLIP` label appears with the red.

### Browser

#### TrackList

Rows of tracks used by the in-app file selector (M2), the session list in the browser strip (M2) and the full browser (M3).

##### Anatomy

- Header row: column names in the `label` style, `text-secondary`, on `surface-panel`, 1px `line-divider` below.
- Rows: 28px high, `body` style, `space-3` side padding. Columns in M2: title, artist, duration (`readout` style, right-aligned), format.
- States: rest on `surface-panel`; hover `surface-hover`; selected `surface-selected`; focused row adds the 2px `focus-ring` inset.
- Loaded marker: a 16px deck-letter badge (deck colour, `text-on-accent`) in the first column for tracks loaded in a deck.
- Folders in the file selector: same row, name followed by `/`, no duration.
- Unreadable file: name in `text-secondary`, a `status-error` dot and the label "Can't read" in the format column.

##### Behaviour

- Up/Down move the selection, Enter opens a folder, Backspace goes up a level.
- Load to deck: Shift+Left loads the selected track to deck A, Shift+Right to deck B; drag and drop onto a deck does the same (`deck.load`).
- Long names truncate with an ellipsis; the full name shows on hover and to screen readers.

##### Browser columns (M3)

- Default columns: colour tag, title, artist, BPM, key, rating, time, comment. Columns can be shown, hidden, resized and reordered; sorting by any column.
- **Colour tag:** an 8px square in one of the eight `hotcue-N` colours.
- **Key:** musical notation; when compatible with the master deck the key text turns `key-match` and a tooltip says "Compatible key".
- **Rating:** five `star` icons, filled for the rating, in `text-secondary`.
- **Played:** tracks played in this session show their title in `text-secondary` with a small check after it.
- **Analysing:** a thin progress bar in `text-secondary` under the row while analysis runs.

#### BrowserTree

The left column of the browser (milestone M3): sources of tracks, with favourites on top.

##### Anatomy

- Favourites row: up to 8 chips (`radius-md`, `surface-control`) with a playlist name each.
- Tree rows: 24px high, `body` style; 16px indent per level; disclosure chevron for folders; `playlist`, `history`, `folder` and `bookmark` icons.
- Fixed nodes, in order: **Collection**, **Playlists**, **Preparation**, **History**, **Explorer** (file system); **Recordings** joins in M4.
- Selected node `surface-selected`; focus ring as in `TrackList`.
- Track counts right-aligned in the `readout` style.

##### Behaviour

- Up/Down move, Right/Left expand and collapse, Enter shows the node in the track list.
- Dropping tracks on a playlist adds them; dropping on Preparation queues them.
- Commands: `browser.node.select`, `browser.playlist.create`, `browser.playlist.rename`, `browser.playlist.delete`, `browser.favourite.set`.

#### SearchField

Live search above the track list (milestone M3).

##### Anatomy

- 28px field, `bg-app` fill, 1px `line-control` outline, `radius-md`; `search` icon on the left; `close` button on the right when there is text.
- Placeholder "Search" in `text-secondary`; query in `text-primary`.
- Focus ring as every control.

##### Behaviour

- Filters the current node as you type, across title, artist, album, comment, BPM and key.
- Ctrl+F focuses it; Escape clears it and returns focus to the track list.
- Command: `browser.search.set`.

#### PreviewPlayer

Pre-listen of the selected track without loading it into a deck (milestone M3).

##### Anatomy

- Play/pause button with the `headphones` icon beside it, to make clear it plays in the headphones.
- A one-colour overview strip (`text-secondary` peaks on `bg-app`), 200 × 20px, with the `playhead` line; click to seek.
- Elapsed time in the `readout` style.

##### Behaviour

- Plays through the cue (headphone) output; with a stereo-only device it is disabled and its tooltip explains why.
- Does not change the track's played state or history.
- Commands: `preview.play_pause`, `preview.seek`, `preview.load_selected`.

### Effects

#### EffectUnit

One effect unit: FX 1 over the left deck column and FX 2 over the right one, in the effect row (milestone M4).

##### Anatomy (group mode, as `M4-effects.html` draws it)

- Panel `surface-panel`, 72px tall, the width of its deck column (520 at 1440), padding 0 12px, items centred vertically, gap 8px.
- Header, 64 × 40: the unit title (`FX 1`, 600 12/16, `text-primary`) over a segmented choice of `Group` and `Single` (78 × 20,
  segments of 600 10/12, the chosen one on `surface-pressed`). With four units the title is a two-key switch, `1 | 3` or `2 | 4`.
- Dry / wet: a 36px `Knob` in a 52px column, its label `Dry / wet` under it (500 11/14, `text-secondary`).
- Three slots, 116 × 42 on a 124px step. Each has a 36px amount `Knob` at the left and, 6px to its right, the effect selector
  (74 × 20, `surface-control`, the effect name 500 11/14 and the chevron of `Select`) over the `On` key (74 × 18, 600 10/12), 4px
  apart. A lit `On` key has the 3px `text-primary` strip along its top edge, as the keys of M3.
- An empty slot shows `None` in its selector; its knob and its `On` key are disabled.

##### Anatomy (single mode)

- Header and Dry / wet as in group mode.
- In the place of the three slots: the effect selector (74 × 20) over the `On` key (74 × 18); three 36px knobs in 52px columns, each
  with the name of its parameter under it; two keys of 52 × 20 stacked 2px apart, with the names of their parameters; `Reset`
  (40 × 20) at the right end of the panel.
- A parameter the effect does not use leaves its knob or key disabled, with no name.

##### Behaviour

- Knobs follow `Knob`: drag, keyboard, double click back to the default.
- A selector opens a `Select` of the eight effects and `None`.
- Commands: `fx.unit.mode.set`, `fx.unit.dry_wet.set`, `fx.slot.effect.set`, `fx.slot.amount.set`, `fx.slot.toggle`,
  `fx.single.effect.set`, `fx.single.knob.set`, `fx.single.key.toggle`, `fx.single.toggle`, `fx.unit.reset`, and
  `fx.unit.snapshot.save` in the context menu of the unit title.
- A unit no channel is assigned to keeps its controls enabled; the tooltip of its title says it is not assigned.

#### EffectAssign

The keys that put a channel through effect units 1 and 2.

##### Anatomy

- In each channel of the four-deck mixer: two keys of 20 × 18, stacked 3px apart, at the right of the mixer effect knob, labelled `1`
  and `2` (600 10/12). Lit: the `text-primary` strip along the top edge.
- Name for screen readers and tooltip: "Send deck C to effect unit 1".
- The two-deck mixer places the same keys side by side, left of the `L` and `R` keys of the crossfader assign, in their row; it has no
  mixer effect knob (approved on its capture on 18 November 2026).

##### Behaviour

- A click emits `mixer.fx_assign.toggle` with the deck and the unit.

#### MixerEffect

The one-knob effect of each channel, under its filter knob.

##### Anatomy

- A 30px `Knob` in a 40px column; under it the name of its effect (500 11/14, `text-secondary`), drawn as a key with the look of a
  label (text only, `surface-hover` on hover) that opens a `Select` of the catalogue.
- The arc fills from the left end; at 0 it is empty.

##### Behaviour

- Commands: `mixer.effect.set` (amount 0 to 1) and `mixer.effect.select`.
- In external mixer mode the knob and its name collapse with the rest of the channel, as the filter does.

#### Effect row

- Left unit panel over the left deck column, the middle panel over the mixer (396px), the right unit panel over the right deck
  column, with 2px seams of `bg-app`.
- The middle panel holds the mix recorder when it arrives: the label `Mix recorder` over a 28px row of `Rec` (48 × 28, its strip
  `status-error` while recording), the elapsed time (tabular figures, 47px) and `Cut` (39 × 28), right-aligned with 12px padding.
  Until then it is an empty panel.
- No new icon: selectors use the chevron of `Select`; the keys use text.

### Controllers

#### ControllerMapping

The controller manager in Settings (milestone M3): devices, their assignment table and learn mode.

##### Anatomy

- Device `Select` and **Add**, **Import**, **Export** buttons above the table.
- Table columns: Control (as received, `readout` style), Command (`body`), Modifier, Type (Button, Fader, Encoder), Deck.
- Rows 28px, same states as `TrackList`.
- **Learn**: the row waiting for input has a dashed 2px `focus-ring` outline and the text "Move a control…" in `text-secondary`; the MIDI indicator in the status header blinks on every message.

##### Behaviour

- Mappings are TOML files; Import and Export read and write them.
- Learn fills the Control column with the next message received; Escape cancels.
- Commands: `controller.device.select`, `controller.mapping.add`, `controller.mapping.learn`, `controller.mapping.import`, `controller.mapping.export`.
