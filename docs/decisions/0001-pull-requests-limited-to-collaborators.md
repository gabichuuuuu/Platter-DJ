# 0001. Pull requests limited to collaborators

Status: superseded by `0018-pull-requests-open-to-everyone.md` (M3).

## Context

- The project is in its first milestones. The architecture, the rules and the gate are still
  being laid down, and a contribution written before they settle is likely to be rewritten.
- There is a single maintainer. Review time is the scarcest resource.
- Public repositories receive a growing volume of pull requests generated with AI tools and
  submitted without review by their authors. Triaging them costs more than they contribute.
- The repository starts private and becomes public later. Whatever policy is chosen must hold on
  the day the visibility changes, without anything left to configure in a hurry.

## Options

1. Accept pull requests from anyone from the first public day, filtered by an automatic quality
   check.
2. Limit the creation of pull requests to collaborators, keep issues and Discussions open, and
   prepare everything needed to open later.
3. Disable pull requests entirely.

## Decision

Option 2. In the repository settings, the creation of pull requests is limited to collaborators
(<https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/disabling-pull-requests>).
Issues and Discussions stay open, so bugs, ideas and controller reports can still arrive.

Everything needed for opening is written now and stays inactive:

- `CONTRIBUTING.md` holds the complete guide, including the policy on AI-assisted contributions
  and the licence of contributions. A notice at the top, between the `external-prs-notice`
  markers, states that external pull requests are not accepted yet.
- `.github/workflows/pr-quality.yml` runs the quality filter only for authors who are not
  owners, members or collaborators. While pull requests are limited it never runs.
- `docs/maintainers/github-setup.md` lists the steps to open, in order.

Option 1 was rejected because it spends review time the project does not have yet. Option 3 was
rejected because collaborators need pull requests: every change to `main` goes through one.

## How to revert

Follow "Opening to external pull requests" in `docs/maintainers/github-setup.md`: enable the
limit of open pull requests per user without write access, change the setting to allow everyone,
remove the notice block from `CONTRIBUTING.md`, review the rule of `main`, and supersede this
record with a new one that registers the opening.

## How it was measured

Not measured. The decision rests on the size of the team and is revisited when the project has a
working first version or a second maintainer.
