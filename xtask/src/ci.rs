//! The gate: one command, the same locally and in CI.

use std::path::Path;

use crate::failure::Failure;
use crate::{cleanroom, layers, parity, privacy, repo};

const DENY_VERSION: &str = "0.20.2";

enum Step {
    Cargo(&'static [&'static str]),
    Deny,
    Check(fn(&Path) -> Result<(), Failure>),
}

/// In order; the gate stops at the first step that fails.
const STEPS: &[(&str, Step)] = &[
    (
        "cargo fmt --all -- --check",
        Step::Cargo(&["fmt", "--all", "--", "--check"]),
    ),
    (
        "cargo clippy --workspace --all-targets -- -D warnings",
        Step::Cargo(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
    ),
    (
        "cargo test --workspace",
        Step::Cargo(&["test", "--workspace"]),
    ),
    ("cargo xtask layers", Step::Check(layers::run)),
    ("cargo deny check", Step::Deny),
    ("cargo xtask cleanroom", Step::Check(cleanroom::run)),
    ("cargo xtask privacy", Step::Check(privacy::run)),
    ("cargo xtask parity --check", Step::Check(parity::check)),
];

pub fn run(root: &Path) -> Result<(), Failure> {
    for (index, (label, step)) in STEPS.iter().enumerate() {
        println!("==> [{}/{}] {label}", index + 1, STEPS.len());
        match step {
            Step::Cargo(args) => repo::run_visible(root, &repo::cargo(), args)?,
            Step::Deny => deny(root)?,
            Step::Check(check) => check(root)?,
        }
    }
    println!("==> gate passed");
    Ok(())
}

fn deny(root: &Path) -> Result<(), Failure> {
    let cargo = repo::cargo();
    if !repo::succeeds(root, &cargo, &["deny", "--version"]) {
        return Err(Failure::message(format!(
            "cargo-deny is not installed. Install it with: \
             cargo install cargo-deny --version {DENY_VERSION} --locked"
        )));
    }
    repo::run_visible(root, &cargo, &["deny", "check"])
}
