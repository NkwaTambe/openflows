# Releasing OpenFlows

This document describes the release strategy for the two shipped packages —
`openflows` (the main binary) and `openflows-harness` (the typed SharedStore CLI).

Releases are driven by **release-plz** on a **`develop`-as-default** branching
model. There is **no** npm, Homebrew, Docker/GHCR, or crates.io publishing; the
release pulls cross-platform binary tarballs onto a GitHub Release only.

## Branching model

```
feature/* ──PR──▶ develop   (default branch, integration)
develop ──┐
          └─(create)──▶ release/vX.Y.Z ──PR merge──▶ main
                                                    │  push to main
                                                    ▼
                             release-plz → tag vX.Y.Z + GitHub Release
                             release-assets → attach cross-platform tarballs
```

- `develop` is the default branch and the integration point. All feature work
  lands here via PR (CI + review required).
- `main` is release-only. It only ever receives merges of `release/vX.Y.Z`
  branches. A guard workflow rejects any other PR to `main`.
- `release-plz` is the sole publisher; the `release-assets` workflow attaches
  binaries.

## Day-to-day workflow

1. Branch from `develop` (`feature/*`) and open a PR into `develop`.
2. CI and semver checks run on `develop`; merge once green.

## Cutting a release

1. Ensure the changes you want released are merged to `develop`.

2. Create a release branch from `develop`:

   ```
   git checkout develop
   git pull
   git checkout -b release/vX.Y.Z
   ```

3. Bump the versions in lock-step (keep both packages aligned so one tag covers
   both):

   - `binary/Cargo.toml` → `version = "X.Y.Z"`
   - `crates/openflows-harness/Cargo.toml` → `version = "X.Y.Z"`

   Update the lockfile (`cargo build` refreshes `Cargo.lock`), then commit.

4. Open a PR from `release/vX.Y.Z` into `main`.

   - The release-branch guard requires the name to match `release/vX.Y.Z`.
   - `CI` does **not** run on `main` PRs; the release happens on merge.

5. Merge the PR into `main`. On push to `main`, **release-plz**:

   - regenerates the CHANGELOG from Conventional Commits,
   - creates the `vX.Y.Z` tag,
   - opens the GitHub Release.

6. The tag push triggers **release-assets**, which builds and attaches:

   - `openflows-<v>-<target>.tar.gz` + `.sha256` for
     x86_64/aarch64 Linux-GNU (zigbuild), Linux-musl, and macOS,
   - a standalone `openflows-harness-x86_64-unknown-linux-musl.tar.gz`.

   Asset names are fixed/predictable, so re-runs overwrite rather than error.

## Commit message convention

release-plz builds the CHANGELOG from commit subjects between releases, so use
[Conventional Commits](https://www.conventionalcommits.org/) prefixes —
`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `perf:`, `test:`. Freeform
subjects (e.g. "Address review: …", "Rustfmt") produce a noisier CHANGELOG.

## Notes

- Version lock-step: today one `vX.Y.Z` tag covers both packages. If the harness
  is decoupled later, release-plz supports per-package versioning.
- No crates.io publishing is configured (these are application binaries). If you
  ever add a library worth publishing, add a `CARGO_REGISTRY_TOKEN` secret and
  opt that package in.
