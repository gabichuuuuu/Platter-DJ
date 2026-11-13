# 0003. Scope of the privacy check

Status: accepted (M0).

## Context

`AGENTS.md` forbids any trace of the development machine in the repository and its history.
`cargo xtask privacy` enforces what a tool can detect. Three points needed a decision: where the
names of the machine come from, what the check does in CI, and how far image inspection goes
without an external dependency.

## Choices

**Source of the local names.** The user, machine and domain names are read from environment
variables when the check runs (`USERNAME`, `USER`, `LOGNAME`, `COMPUTERNAME`, `HOSTNAME`,
`USERDOMAIN`, `USERDNSDOMAIN`). Nothing about any machine is written in the code or its tests;
the tests use invented names. A name counts when it appears as a whole word, in any letter case.
A finding names the variable and never prints the value, so the output of the check can be
pasted anywhere.

**Behaviour in CI.** On a CI runner those variables describe the runner. Its generic user name
is an ordinary word of the documentation and would fail every run, and the runner is not a
development machine. When `CI=true` the local-name check is skipped and the check says so. The
generic path patterns, the Git identity rules and the image inspection still run there, over
the complete history.

**Path patterns.** A user path is a drive followed by the users folder and the first character
of a name, with single, doubled or forward separators, or a Unix home root followed by the first
character of a name. Requiring that character lets the documentation show placeholders. A root
that continues a longer path or a URL is not a finding.

**Images.** The project writes its own readers instead of adding a dependency. PNG chunks and
JPEG segments are walked with bounded steps; text, EXIF, XMP, IPTC, comment and modification-time
blocks are findings. SVG is searched for a metadata element and editor attributes. Any other
image format is rejected, because a format the check cannot read cannot be shown to be clean.

**Staged files.** The file list comes from the Git index, so running the check after `git add`
and before `git commit` covers what the commit will contain.

## Limits

The check cannot recognise a device name, a hardware model or a network address that it has no
pattern for. Those remain the responsibility of the author and the reviewer.

## How it was measured

Unit tests in `xtask/src/privacy.rs` cover each pattern with invented values, including the
cases that must pass: placeholders, URLs, relative paths and words that merely contain a name.
The check runs on its own source as part of the gate.
