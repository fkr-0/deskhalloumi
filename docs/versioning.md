# Versioning and release policy

## Current state

The active Cargo workspace declares a shared version of `0.2.0` in the root
`Cargo.toml`, and the main crates inherit it with `version.workspace = true`.
The released commit includes a dated changelog section, automated
release-metadata checks, and the annotated `v0.2.0` tag. The canonical remote is
`https://github.com/fkr-0/deskhalloumi`.

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

10. Push the reviewed branch and annotated tag explicitly:

    ```sh
    git push origin main
    git push origin vX.Y.Z
    ```

11. Verify the tag-triggered workflow. `.github/workflows/release.yml` checks
    that the tag object is annotated and points at the checked-out commit,
    reruns all release gates, builds the primary and compatibility binaries,
    and uploads a deterministic Linux archive with a SHA-256 checksum.

If GitHub did not register the original tag event, or a runner-only failure was
fixed after publication, rerun packaging without moving the tag:

```sh
gh workflow run release.yml --ref main -f release_ref=vX.Y.Z
```

The manual workflow checks out the requested annotated tag before validating,
building, and packaging it. The workflow definition comes from `main`, while
the source and release metadata remain pinned to the immutable tag.

Release retries may carry narrowly documented runner-compatibility exclusions
for obsolete hardware-presence smoke assertions in the tagged tree. Such an
exclusion must name the exact test and may not skip parser, engine, integration,
or release-metadata coverage.

The release workflow does not publish crates or create a public GitHub release.
Those remain explicit maintainer actions after artifact review.

## Post-release maintenance

- Treat a pushed annotated release tag as immutable. Never force-move or replace
  it to include later documentation or packaging corrections.
- Put post-tag changes under `[Unreleased]` and commit them on the development
  branch.
- Use a patch release when a correction must be represented by a new immutable
  source tag or redistributed artifact.
- Documentation may link to the stable tag tree and to the comparison from that
  tag to `HEAD`.

## Current release

Version `0.2.0` is the first DeskHalloumi-branded release. It includes the
compatibility-first command rename, selective X11 hotkeys, active i3 auditing,
cross-process actions, menu/front-end work, dynamic evdev keyboard hot-plug,
sxhkd migration improvements, and release automation. New work must return to
the `[Unreleased]` changelog section and select a later SemVer version before
the next annotated tag is created.
