# Security policy

## Supported versions

Platter has no release yet. Once releases exist, only the latest one receives security fixes
while the project is at 0.x.

## Reporting a vulnerability

Do not report security problems in public issues or discussions.

Use GitHub's private vulnerability reporting on this repository (Security > Report a
vulnerability). It is enabled when the repository becomes public. Include the affected version or
commit, the steps to reproduce the problem and, if a file triggers it, a minimal file that does.

Files, controller mappings, MIDI messages and command parameters are untrusted input for Platter:
a crash, hang or memory exhaustion caused by any of them is a security-relevant bug.

## Scope

Platter opens no network connection without an explicit user action and collects no telemetry.
Reports about third-party dependencies are welcome here too; they are tracked with
`cargo deny check` in CI.
