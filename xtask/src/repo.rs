//! Access to the repository: its root, its tracked files and external commands.

use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::failure::Failure;

/// Root of the workspace.
///
/// Read at run time from the variable cargo sets for `cargo run`, so the binary carries no
/// path of the machine that built it.
pub fn root() -> Result<PathBuf, Failure> {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
        Failure::message("CARGO_MANIFEST_DIR is not set; run this tool as `cargo xtask <command>`")
    })?;
    Path::new(&manifest_dir)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Failure::message("the xtask manifest directory has no parent"))
}

/// The cargo that started this tool, so nested calls use the same toolchain.
pub fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn describe(program: &OsStr, args: &[&str]) -> String {
    format!("{} {}", program.to_string_lossy(), args.join(" "))
}

/// Runs a command with inherited output and fails unless it exits with success.
pub fn run_visible(root: &Path, program: &OsStr, args: &[&str]) -> Result<(), Failure> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|error| {
            Failure::message(format!("cannot run `{}`: {error}", describe(program, args)))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Failure::message(format!(
            "`{}` failed ({status})",
            describe(program, args)
        )))
    }
}

/// Runs a command and returns its standard output.
pub fn capture(root: &Path, program: &OsStr, args: &[&str]) -> Result<Vec<u8>, Failure> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| {
            Failure::message(format!("cannot run `{}`: {error}", describe(program, args)))
        })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(Failure::message(format!(
            "`{}` failed ({})",
            describe(program, args),
            output.status
        )))
    }
}

/// Whether a command exists and exits with success, without showing its output.
pub fn succeeds(root: &Path, program: &OsStr, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Paths of the files in the Git index, relative to the root, with forward slashes.
///
/// The index includes staged files, so a check run before a commit covers what the commit
/// will add.
pub fn tracked_files(root: &Path) -> Result<Vec<String>, Failure> {
    let listing = capture(root, OsStr::new("git"), &["ls-files", "-z"])?;
    Ok(listing
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect())
}

/// Content of a tracked file, or `None` when it is deleted in the working tree.
pub fn read_tracked(root: &Path, path: &str) -> Result<Option<Vec<u8>>, Failure> {
    match std::fs::read(root.join(path)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Failure::message(format!("cannot read `{path}`: {error}"))),
    }
}

/// Content of a file the checks need, as text.
pub fn read_text(root: &Path, path: &str) -> Result<String, Failure> {
    std::fs::read_to_string(root.join(path))
        .map_err(|error| Failure::message(format!("cannot read `{path}`: {error}")))
}
