# 0018. Pull requests open to everyone, merged only by the maintainer

Status: accepted (M3). Supersedes `0001-pull-requests-limited-to-collaborators.md`.

## Context

- The repository became public in M3, with the architecture, the rules and the gate in place.
- There is still a single maintainer, and review time is still the scarcest resource.
- The maintainer wants outside contributions to be possible, provided nothing reaches `main`
  without their own review.

## Options

1. Keep the creation of pull requests limited to collaborators (record 0001).
2. Let anyone open a pull request; only the maintainer merges, after reviewing it.

## Decision

Option 2, by decision of the maintainer. In the repository settings, anyone can create a pull
request. A pull request from outside the project is merged only when the maintainer has reviewed
it and accepted it by hand; nothing merges it automatically.

- `.github/workflows/pr-quality.yml` filters the pull requests of authors who are not owners,
  members or collaborators before the maintainer reads them.
- `.github/workflows/pr-intake.yml` lets an author with no pull request merged into `main` have
  one open at a time and closes the next with a comment; from the first merge on there is no
  limit. The limit that GitHub offers in its settings applies to every author without write
  access, even after a merge, so the project applies its own.
- `CONTRIBUTING.md` no longer carries the notice that external pull requests are not accepted.
- The steps that remain to be applied in the settings are listed in
  `docs/maintainers/github-setup.md`.

## How it was measured

Not measured. The decision rests on the wish of the maintainer to accept contributions and is
revisited if the volume of pull requests exceeds the review time available.
