# 0027. Versions computed from the pull request titles; a release ships when the maintainer merges its pull request

Status: accepted (M3).

## Context

- The roadmap lists the release workflow among the deliveries of M3. Until now there was no
  release, no tag and the workspace version was 0.0.0.
- The pull request titles already follow Conventional Commits and are squash-merged, so the
  history of `main` says whether each change is a feature, a fix or neither.
- The rules require SemVer at 0.x until M4 closes, binaries built only in CI with the paths
  remapped, and the maintainer as the only one who merges.
- All crates inherit the version of `[workspace.package]` and set `publish = false`; Platter is
  released as one product, not as crates on a registry.
- The default token of GitHub Actions cannot start other workflows: a tag or a release created
  with it does not trigger `on: push: tags` or `on: release`, and the checks of `main` do not
  start on a pull request it opens.

## Options

1. By hand: the maintainer chooses the number, edits `Cargo.toml` and the changelog, and creates
   the tag.
2. release-plz (Apache-2.0), a tool for Rust workspaces: it keeps a release pull request open,
   computes the next version from Conventional Commits, writes the changelog and, once that pull
   request is merged, creates the tag and the GitHub release.
3. release-please (Apache-2.0), a generic tool with the same model. Only its licence and its
   latest version (v5.0.0 of its action) were checked; its handling of a Cargo workspace with
   unpublished crates was not studied.
4. An own `cargo xtask release` that computes the version and opens the pull request with `gh`.

## Choice

Option 2, approved by the maintainer as a CI tool: it runs only on GitHub Actions, is not a
dependency of any crate and is not part of the binaries. The action is pinned by SHA
(`release-plz/action` v0.5.148) and the binary by version (release-plz 0.4.3).

How it is configured (`release-plz.toml`):

- **One version, one tag.** Versions come from the Git tags `vX.Y.Z` (`git_only`), never from a
  registry. Every crate reads the same tag; only `platter` creates the tag, the GitHub release
  and `CHANGELOG.md`, which includes the commits of the crates it ships. `xtask` and
  `platter-testkit` never release Platter.
- **What raises the number.** `feat` raises the minor number and `fix` or `perf` the patch number.
  A breaking change (`!` after the type, or a `BREAKING CHANGE:` footer) raises the minor number
  while Platter is at 0.x and the major number from 1.0 on. Pull requests of type `docs`,
  `chore`, `test`, `ci`, `refactor` or `style` alone do not lead to a version, and stay out of
  the changelog. Platter is an application, so `features_always_increment_minor` is on; the
  release-plz documentation warns that this departs from the Cargo rule for libraries, which does
  not apply here.
- **When a version ships.** Each merge into `main` only updates the release pull request
  (`release_always = false`). The version ships when the maintainer merges that pull request.
- **First version.** With no tag, release-plz releases the version written in `Cargo.toml`. It
  is set to 0.2.0, the last closed milestone; the following numbers are computed.

How it runs (`.github/workflows/release.yml`), on each push to `main`:

1. `release` does nothing unless the last commit is a merged release pull request. Then it pushes
   the tag and creates the GitHub release as a draft.
2. `release-pr` opens or updates the release pull request. It waits for `release`, so the tag of a
   version just merged already exists when the next version is computed.
3. `binaries` builds `platter` and `platter-cli` from the tag on Linux, Windows and macOS, without
   a dependency cache and with `--remap-path-prefix` for the workspace and the Cargo home, and
   attaches one archive per system with its SHA-256. On Windows the Cargo home is converted with
   `cygpath -w`, because the shell gives it in a form rustc does not match, and the linker gets
   `/PDBALTPATH:%_PDB%` so the executable names its PDB without a path.
4. `publish` publishes the draft and marks it as the latest release.

The binaries are built by jobs of the same run because a separate workflow would never start
(see the context). The tags are created only by this workflow; nobody creates one by hand.

The release pull request is not checked by CI on its own: the maintainer closes and reopens it,
which runs the required `gate` checks. A personal access token or a GitHub App would avoid that
step, but it would be a secret to keep; it can be added later without changing the model.

The release pull request has its own short body instead of the template of
`.github/pull_request_template.md`: it holds no code change, only the version and the changelog.

Requires a setting the maintainer applies by hand: "Allow GitHub Actions to create and approve
pull requests" (`docs/maintainers/github-setup.md`).

## How it was measured

Read, not run. The behaviour was read in the documentation and in the source and tests of
release-plz at the tag `release-plz-v0.4.3`:

- `git_only` with a tag shared by several packages (`git_only_update_handles_packages_sharing_a_release_tag`);
- the first release with no tag uses the version of `Cargo.toml`
  (`git_only_no_tags_creates_initial_release`);
- `features_always_increment_minor` and `release_commits` (`git_only_feat_commit_minor_bump`,
  `git_only_respects_release_commits_regex`);
- a package that inherits the workspace version moves with it even when it is not released
  (`crates/release_plz_core/src/command/update/updater.rs`).

The remapping was measured on Windows x64 with the commands of the build step, building
`platter-cli` in release in Git Bash: with the Cargo home in the form the shell gives
(`/c/...`), the executable held the local user name 222 times; with `cygpath -w` and
`/PDBALTPATH:%_PDB%`, it held no user name, no local path and only the file name of its PDB.
`platter` and the Linux and macOS builds were not measured; they use the same flags.

The workflow has not run yet: its first run is the first push to `main` after it is merged. To
check then: that the release pull request proposes 0.2.0, that its changelog lists only features
and fixes, and, at the first release, that the three archives are attached and the release is
published.
