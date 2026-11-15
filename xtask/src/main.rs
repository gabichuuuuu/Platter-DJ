//! Repository automation for Platter. Run it as `cargo xtask <command>`.

mod ci;
mod cleanroom;
mod failure;
mod layers;
mod parity;
mod privacy;
mod repo;

use std::process::ExitCode;

use failure::Failure;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci               run the whole gate, stopping at the first failure
  layers           check the layer rules and the dependency registry
  cleanroom        check for third-party material that must not enter the repository
  privacy          check for traces of the development machine; not part of the gate
  parity           validate docs/parity.toml and refresh the summary in ROADMAP.md
  parity --check   validate docs/parity.toml and the summary markers in ROADMAP.md
  parity --site <folder>
                   write the progress page and its badge into a folder";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("xtask: {failure}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result<(), Failure> {
    let root = repo::root()?;
    match args {
        ["ci"] => ci::run(&root),
        ["layers"] => layers::run(&root),
        ["cleanroom"] => cleanroom::run(&root),
        ["privacy"] => privacy::run(&root),
        ["parity"] => parity::write(&root),
        ["parity", "--check"] => parity::check(&root),
        ["parity", "--site", folder] => parity::site::write(&root, std::path::Path::new(folder)),
        _ => Err(Failure::message(USAGE)),
    }
}
