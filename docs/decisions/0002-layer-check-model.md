# 0002. Model of the layer check

Status: accepted (M0).

## Context

`docs/architecture.md` states the layer rules and `cargo xtask layers` enforces them. The rules
leave four points open: how far "depends on" reaches for the interface and device crates, what
the test kit itself may depend on, which dependency kinds the layer order covers, and how a
rule about a module (the real-time module of the engine) is checked by a tool that sees crates.

## Options and choices

**Reach of the ban on interface and device crates.** Checking only declared dependencies would
let one of those crates arrive through an innocent-looking intermediate. The check follows the
resolved graph of `cargo metadata`: from the direct dependencies of any kind, through normal and
build dependencies. The graph covers all targets and all features, the strictest reading.

**Dependencies of the test kit.** The test kit is a dev-dependency of the crates it serves. If it
could build on L1 or above, a crate and its test kit would depend on each other. It sits beside
L1 and builds on L0 only. If M1 shows that it needs more, a new record changes this.

**Dependency kinds.** The layer order applies to normal, build and dev dependencies alike. A test
that reaches into a higher layer couples the layers as much as shipped code does. The only
kind-specific rule is that the test kit must be a dev-dependency.

**The automation tool.** `xtask` depends on no workspace crate and nothing depends on it, so the
gate can never be broken by the code it checks.

**The real-time module.** The rule "the real-time module of the engine uses only the core and DSP
crates" concerns a module. Cargo metadata cannot see it, and the module does not exist yet. The
check is written together with the engine in M1, when the module path is known. Until then the
rule is documented and not enforced.

**Dependency registry.** Besides requiring a row in `docs/dependencies.md` for each direct
external dependency, the check rejects a row no crate uses, so the document cannot drift.

## How it was measured

The rules run against a plain model of a workspace, so the tests in `xtask/src/layers.rs` build
models that break each rule and assert the finding: an upward dependency, a dependency between
crates of the same layer, an interface crate declared below L3, a device crate reached through
another dependency, the test kit as a normal or build dependency, a crate missing from the
table, and a missing or stale registry row.
