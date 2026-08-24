# Releasing OpenFlows

This document describes the release strategy for the two shipped packages —
`openflows` (the main binary) and `openflows-harness` (the typed SharedStore CLI).

Releases are driven by **release-plz** on a **`develop`-as-default** branching
model, with **versions computed automatically from Conventional Commits**. There
is **no** npm, Homebrew, Docker/GHCR, or crates.io publishing; the release pulls
cross-platform binary tarballs onto a GitHub Release only.

## Branching model

```
feature/* ──PR──▶ develop          (default branch, integration)
develop ──┐
          ├─ release-plz release-pr ──▶ release-plz-* PR (bumps versions + CHANGELOG)
          └─(create)──▶ release/vX.Y.Z ──PR merge──▶ main
                                                    │  push to main
                                                    ▼
                          release-plz release → tag openflows-X.Y.Z
                                              + tag openflows-harness-X.Y.Z
                                              (one GitHub Release per binary, no `v`)
                          release-assets → attach each binary's tarballs to its own release
```

- `develop` is the default branch and the integration point. All feature work
  lands here via PR (CI + review required).
- `main` is release-only. It only ever receives merges of `release/vX.Y.Z`
  branches. A guard workflow rejects any other PR to `main`.
- release-plz is the sole publisher; the `release-assets` workflow attaches
  binaries.

## How versions are provided (automatic)

You never type a version. release-plz derives the next version from the
Conventional Commits merged since the last tag:

| Commit type / footer             | Version bump |
|----------------------------------|--------------|
| `BREAKING CHANGE:` (any type)    | **major**    |
| `feat:`                          | **minor**    |
| `fix:`, `docs:`, `chore:`, …     | **patch**    |

`release-plz` opens a `release-plz-*` pull request to `develop` that bumps each
released package's manifest version and regenerates the CHANGELOG. Merging that
PR "locks in" the new version.

> The very first release (no tags yet in the repo — the old tag history was
> deleted for a clean start) is treated as an **initial release** and ships at
> the current version in the manifests (`1.2.0`). After that, all releases are
> computed from conventional commits.

## Day-to-day workflow

1. Branch from `develop` (`feature/*`) and open a PR into `develop`.
2. CI runs on `develop`; merge once green.
3. Write Conventional Commit messages (`feat:`, `fix:`, `docs:`, …) — a
   **commitlint** guard enforces this on every PR (title and commit messages).

## Cutting a release

1. Merge the feature work you want to release into `develop`.

2. `release-plz` will have opened (or updated) a `release-plz-*` PR to `develop`
   bumping the versions + CHANGELOG. Review and merge it so the new version sits
   on `develop`. (If no release PR appears, run the `Release → release-plz
   (release-pr)` workflow manually.)

3. Cut a release branch from `develop` carrying that version:

   ```
   git checkout develop
   git pull
   git checkout -b release/vX.Y.Z
   ```

   The `X.Y.Z` in the branch name should match the version set by release-plz.

4. Open a PR from `release/vX.Y.Z` into `main`.

   - The release-branch guard requires the name to match `release/vX.Y.Z`.
   - The release happens on merge.

5. Merge the PR into `main`. On push to `main`, **release-plz `release`** creates
   one tag and GitHub Release per binary:

   - `openflows-X.Y.Z` (release **openflows**),
   - `openflows-harness-X.Y.Z` (release **openflows-harness**).

   Tag/release names carry no `v` prefix.

6. Each tag push triggers **release-assets**, which builds and attaches that
   package's own cross-platform tarballs:

   - on `openflows-*` tags: `openflows-<X.Y.Z>-<target>.tar.gz` + `.sha256`
     for x86_64/aarch64 Linux-GNU (zigbuild), Linux-musl, and macOS, containing
     the `openflows` and `openflows-doctor` binaries plus `orchestration/`;
   - on `openflows-harness-*` tags: `openflows-harness-<X.Y.Z>-<target>.tar.gz`
     + `.sha256` containing the `openflows-harness` binary.

   Asset names are fixed/predictable, so re-runs overwrite rather than error.

## Commit message convention

release-plz computes the version and builds the CHANGELOG from commit subjects
between releases, so **every merge message must be a Conventional Commit** —
`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `perf:`, `test:` — with
`BREAKING CHANGE:` in the footer for breaking changes. The commitlint guard
enforces this on every PR.

## Notes

- Independent releases: `openflows` and `openflows-harness` are released
  separately in `release-plz.toml`, each with its own tag
  (`openflows-X.Y.Z` / `openflows-harness-X.Y.Z`) and GitHub Release, so they can
  ship independently. The installer fetches both releases at the same version.
- No crates.io publishing is configured (these are application binaries);
  `release-plz.toml` uses `git_only = true` and `publish = false`.
