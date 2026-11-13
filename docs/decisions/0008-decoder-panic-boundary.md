# 0008. Panic boundary around the decoder

Status: accepted (M1).

## Context

The project rule is that shipped code does not panic and that files are untrusted input.
Decoding is done by `symphonia` 0.6.1. A property test that damages the header of a
synthesised WAV file found a panic inside that library: a multiplication of header fields
overflows (`symphonia-format-riff`, `wave/chunks.rs`). In a build with overflow checks that is
a panic; in a release build the value wraps silently.

The seed of the failing case is kept in `crates/decode/tests/decode.proptest-regressions`.

## Options

- **Validate every header before handing the file to the library.** That means writing a
  parser for each of the six formats, which is most of a decoder.
- **Patch or fork the library.** The dependency is temporary by decision of the maintainer, and
  a fork is one more thing to keep.
- **Catch the panic at the boundary.** `std::panic::catch_unwind` around the single function
  that calls the library, turning a panic into the error "the file is damaged".

## Choice

The boundary. `platter-decode` wraps its call into the library in `catch_unwind` and returns
`DecodeError::Corrupt`. Decoding runs on a worker thread and shares no state with the caller:
the input is consumed and the partial output is dropped, so nothing inconsistent survives the
unwind.

After a successful decode the samples are also checked: any value that is not a finite number
becomes silence, because the engine relies on finite audio.

## Consequences

- The boundary needs unwinding. Platter must not be built with `panic = "abort"` while this
  library is in use; the workspace does not set it.
- The default panic hook still prints the message of the library to standard error. The
  application of M2 installs its own hook that writes to the local log instead.
- The overflow is worth reporting upstream. Nothing has been sent: opening an issue in
  another project is the maintainer's decision.
- Own decoders, if they ever replace the library, must pass the same property tests without
  the boundary.
