# Versioning and release policy

## Current state

The active Cargo workspace declares a shared version of `0.2.0` in the root
`Cargo.toml`, and the main crates inherit it with `version.workspace = true`.
The release candidate includes a dated changelog section and automated
release-metadata checks, but the annotated `v0.2.0` tag is intentionally not
created by the preparation commit.

This document defines the contract; a version heading alone does not claim that
the release has already been published or tagged.

## Version model

The active workspace uses one coordinated version for all first-party crates and
binaries.

```text
MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]
```

### Before 1.0

- `0.MINOR.0`: new features and any explicitly documented breaking change.
- `0.MINOR.PATCH`: backwards-compatible fixes, documentation, and packaging
  corrections.
- Pre-release builds use forms such as `0.2.0-alpha.1` or `0.2.0-rc.1`.
- A breaking pre-1.0 release must include a migration section in the changelog.

### From 1.0 onward

- `MAJOR`: incompatible changes to commands, configuration, IPC, persistent
  data, package names, service names, or documented public APIs.
- `MINOR`: backwards-compatible features.
- `PATCH`: backwards-compatible fixes.

## Public compatibility surfaces

Version decisions must account for more than Rust APIs:

- binary names and CLI options;
- configuration schema and default paths;
- environment variables;
- systemd unit names and behavior;
- Unix socket protocol and serialized fields;
- DBus names and desktop application IDs;
- generated i3 configuration;
- plugin/crate APIs;
- persistent cache and state formats.

A compatibility alias is part of the public contract once released.

## Tags and changelog

- Release tags use `vMAJOR.MINOR.PATCH`, for example `v0.2.0`.
- Pre-release tags retain the SemVer suffix, for example `v0.2.0-rc.1`.
- `CHANGELOG.md` follows Keep a Changelog-style sections under `[Unreleased]`:
  `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, and `Security`.
- User-visible changes belong in the changelog; internal refactors without a
  behavior or maintenance impact do not need individual entries.
- On release, move entries from `[Unreleased]` into a dated version heading and
  restore an empty `[Unreleased]` section.

## Release procedure

1. Start from a clean, reviewed worktree.
2. Select the version from compatibility impact, not commit count.
3. Change `[workspace.package].version` in the root `Cargo.toml`, update any
   explicit first-party package versions, and regenerate `Cargo.lock`.
4. Move changelog entries into `## [X.Y.Z] - YYYY-MM-DD` and restore an empty
   `[Unreleased]` section.
5. Run:

   ```sh
   cargo fmt --all -- --check
   python3 scripts/check_release_metadata.py --candidate
   scripts/test_safe.sh
   scripts/test_i3_hotkeys.sh
   CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
   ```

6. Confirm the changelog describes all user-visible changes and migrations.
7. Commit the complete release candidate and verify the worktree is clean:

   ```sh
   python3 scripts/check_release_metadata.py --candidate --require-clean
   ```

8. Create an annotated tag on that exact commit:

   ```sh
   git tag -a vX.Y.Z -m "Release X.Y.Z"
   ```

9. Verify locally when desired:

   ```sh
   python3 scripts/check_release_metadata.py --release --require-clean
   ```

10. Push the annotated tag. `.github/workflows/release.yml` checks that the tag
    object is annotated and points at `HEAD`, reruns all release gates, builds
    the primary and compatibility binaries, and uploads a deterministic Linux
    archive with a SHA-256 checksum.

The release workflow does not publish crates or create a public GitHub release.
Those remain explicit maintainer actions after artifact review.

## Current recommendation

Version `0.2.0` is the first DeskHalloumi-branded release candidate. It includes
the compatibility-first command rename, selective X11 hotkeys, active i3
auditing, cross-process actions, menu/front-end work, and release automation.
Create `v0.2.0` only after the candidate commit is reviewed and the worktree is
clean; the tag itself is intentionally not created by the preparation commit.
