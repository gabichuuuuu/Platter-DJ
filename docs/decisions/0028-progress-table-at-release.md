# 0028. The progress table of the roadmap is refreshed by the release pull request

Status: accepted (M3).

## Context

- `ROADMAP.md` holds a table generated from `docs/parity.toml`: features of each milestone by
  status. The gate failed when it was out of date, so every pull request that changed a status
  regenerated it.
- On 15 November 2026 nine pull requests were merged one after another. After each merge the
  others conflicted on the same lines of that table, and each conflict meant merging `main` into
  the branch and running CI again, about ten minutes each.
- The progress page, published from `main` on each push (`pages.yml`), already shows the same
  count.

## Options

1. Keep it as it was.
2. Remove the table from the repository and link the progress page.
3. Keep the table, refresh it only in the release pull request, and stop asking the gate for it
   to be up to date.

## Choice

Option 3, chosen by the maintainer. The job `release-pr` of `.github/workflows/release.yml`
runs `cargo xtask parity` on the branch of the release pull request and pushes the change. A pull
request that changes a status leaves the table as it is. `cargo xtask parity --check` validates
the inventory and the markers of the table, not its counts.

Between two releases the table shows the last release; the progress page shows `main`.

## How it was measured

The cost of option 1 was observed on the merges of 15 November 2026: the table conflicted in six
of the nine pull requests. The step of the workflow has not run yet; its first run is the next
push to `main` after this change.
