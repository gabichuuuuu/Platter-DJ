# MCP server

An assistant that speaks the Model Context Protocol (MCP) can drive Platter: load and play
tracks, move the mixer, read what the decks do. It sends the same commands as the keyboard, the
interface and MIDI. The choices behind it are in `docs/decisions/0037-mcp-server.md`.

## What the public manual says

Nothing: the documented application has no MCP server. The function follows the specification
of the protocol, revisions 2026-07-28 and 2025-11-25, read on 21 November 2026
(<https://modelcontextprotocol.io/specification/2026-07-28>: *Overview*, *Versioning*, *stdio*
transport, *Discover*, *Tools*; <https://modelcontextprotocol.io/specification/2025-11-25>:
*Lifecycle*, *Ping*, *Tools*).

## What Platter does

- **Starting it.** `platter --mcp` opens Platter as always and serves MCP on its standard input
  and output. An MCP client starts it so, as a local server named in its configuration; Platter
  listens on no port and opens no connection. Without `--mcp` nothing reads the input.
- **Messages.** One JSON-RPC 2.0 message per line, in UTF-8, up to 1 MiB; a longer line is
  answered with a parse error and skipped. The window answers each message between two frames,
  so a message waits for the window to draw; after 10 s without an answer the client gets an
  error for its request, and a notification still gets nothing. The message stays with the
  window: a command the client was told had failed may still run when the window draws again.
  Closing the input closes Platter.
- **Revisions.** A request that carries its version in `_meta` is served by the revision
  2026-07-28, with `server/discover`; a version the server does not speak is refused with the
  error -32022 and the versions it speaks. A client of the revision 2025-11-25 starts with
  `initialize` and gets its version, or 2025-11-25 when it asks for another; `ping` answers.
  Notifications get no answer.
- **Tools.** Their names start with `platter_`, to stay apart from those of other servers, and
  each carries its title and the hints a client uses for consent: the two that read are marked
  read-only and idempotent, the one that runs commands may change or delete things, and none
  reaches outside the computer.
  - `platter_list_commands` gives the commands a page at a time, 50 unless `limit` asks for up
    to 100, from `offset`: each with its identifier, its parameters with their kinds and ranges,
    and its label, as text and as data, with the total and whether more pages follow. `filter`
    keeps the commands whose identifier or label holds every word of it.
  - `platter_run_command` runs one command by its identifier with its parameters by name:
    numbers as numbers, decks as `a` to `d`, choices and paths as text. The command is checked
    as the command line checks it; a refused one comes back as a result marked as an error, with
    the reason and how to find its parameters.
  - `platter_get_state` gives the four decks (track by title, artist and file name, length,
    position, playing, speed, tempo, sync, key lock, fader, and the controls of the stems of a
    stem deck), the crossfader and the mixer mode, and the tempo of the clock and its master, as
    text and as data.
- **Consent.** The client asks the user before each tool call, as the protocol asks of it; a
  command run through MCP can do whatever the same command does from the keyboard, deleting a
  track from the collection included.

- **Evaluation.** `crates/mcp/evaluation.xml` holds ten questions an assistant answers with
  these tools alone, reading the commands, to measure whether the descriptions let it find its
  way. Their answers were read from the commands on 21 November 2026; a model of the Sonnet
  family, through Claude Code as the client, answered the ten right the same day
  (`docs/decisions/0037-mcp-server.md`, *How it was measured*).

## Edge cases

- A message that is not JSON, not JSON-RPC 2.0 or of an unknown method gets the error of the
  protocol for it; a call of an unknown tool gets -32602.
- A command run while the window does not draw, minimised on some systems, waits for it.
- The state gives no folder of a track: a model needs the name, not where the user keeps it.
