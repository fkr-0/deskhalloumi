#!/usr/bin/env python3
"""Validate workspace Semantic Versioning and changelog release metadata."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(
    r"^(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)
DATE = r"\d{4}-\d{2}-\d{2}"


def fail(message: str) -> None:
    print(f"release metadata error: {message}", file=sys.stderr)
    raise SystemExit(1)


def workspace_version() -> str:
    manifest_path = ROOT / "Cargo.toml"
    try:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        version = manifest["workspace"]["package"]["version"]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot read [workspace.package].version from {manifest_path}: {error}")
    if not isinstance(version, str) or not SEMVER.fullmatch(version):
        fail(f"workspace version {version!r} is not valid Semantic Versioning")
    return version


def validate_inherited_versions(version: str) -> set[str]:
    errors: list[str] = []
    package_names: set[str] = set()
    for manifest_path in sorted(ROOT.glob("unilii/**/Cargo.toml")):
        try:
            manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as error:
            errors.append(f"{manifest_path.relative_to(ROOT)}: cannot parse: {error}")
            continue
        package = manifest.get("package")
        if not isinstance(package, dict):
            continue
        package_name = package.get("name")
        if isinstance(package_name, str):
            package_names.add(package_name)
        package_version = package.get("version")
        if package_version is None:
            errors.append(f"{manifest_path.relative_to(ROOT)}: package has no version")
        elif isinstance(package_version, dict) and package_version.get("workspace") is True:
            continue
        elif package_version == version:
            continue
        else:
            errors.append(
                f"{manifest_path.relative_to(ROOT)}: version {package_version!r} "
                f"does not inherit or match workspace version {version}"
            )
    if errors:
        fail("workspace version mismatch:\n  " + "\n  ".join(errors))
    if not package_names:
        fail("no first-party package manifests were discovered")
    return package_names


def validate_lock_versions(version: str, package_names: set[str]) -> None:
    lock_path = ROOT / "Cargo.lock"
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse {lock_path}: {error}")

    locked_versions = {
        package.get("name"): package.get("version")
        for package in lock.get("package", [])
        if isinstance(package, dict) and package.get("name") in package_names
    }
    missing = sorted(package_names - locked_versions.keys())
    mismatched = sorted(
        (name, locked_versions[name])
        for name in package_names & locked_versions.keys()
        if locked_versions[name] != version
    )
    if missing or mismatched:
        details = [*(f"{name}: missing from Cargo.lock" for name in missing)]
        details.extend(
            f"{name}: Cargo.lock has {locked!r}, expected {version!r}"
            for name, locked in mismatched
        )
        fail("Cargo.lock first-party version mismatch:\n  " + "\n  ".join(details))


def changelog_text() -> str:
    path = ROOT / "CHANGELOG.md"
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        fail(f"cannot read {path}: {error}")
    if not re.search(r"^## \[Unreleased\]\s*$", text, flags=re.MULTILINE):
        fail("CHANGELOG.md must contain an exact '## [Unreleased]' heading")
    return text


def git_output(*args: str) -> str:
    try:
        return subprocess.run(
            ["git", *args],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        fail(f"cannot run git {' '.join(args)}: {error}")


def git_tags() -> set[str]:
    return {line for line in git_output("tag", "--list").splitlines() if line}


def validate_release_heading(version: str, changelog: str) -> None:
    heading = re.compile(
        rf"^## \[{re.escape(version)}\] - {DATE}\s*$", flags=re.MULTILINE
    )
    if not heading.search(changelog):
        fail(
            f"release mode requires a dated '## [{version}] - YYYY-MM-DD' "
            "heading in CHANGELOG.md"
        )


def validate_release_tag(version: str) -> None:
    expected_tag = f"v{version}"
    if expected_tag not in git_tags():
        fail(f"release mode requires git tag {expected_tag}")
    object_type = git_output("cat-file", "-t", expected_tag)
    if object_type != "tag":
        fail(f"release mode requires {expected_tag} to be an annotated tag")
    tagged_commit = git_output("rev-list", "-n", "1", expected_tag)
    head_commit = git_output("rev-parse", "HEAD")
    if tagged_commit != head_commit:
        fail(
            f"release tag {expected_tag} points to {tagged_commit[:12]}, "
            f"but HEAD is {head_commit[:12]}"
        )


def validate_clean_worktree() -> None:
    if status := git_output("status", "--porcelain=v1", "--untracked-files=all"):
        preview = "\n  ".join(status.splitlines()[:20])
        fail(f"clean worktree required; found:\n  {preview}")


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--candidate",
        action="store_true",
        help="require a dated version heading without requiring the release tag",
    )
    mode.add_argument(
        "--release",
        action="store_true",
        help="require a dated version heading and matching annotated vVERSION tag at HEAD",
    )
    parser.add_argument(
        "--require-clean",
        action="store_true",
        help="fail when tracked or untracked worktree changes exist",
    )
    args = parser.parse_args()

    version = workspace_version()
    package_names = validate_inherited_versions(version)
    validate_lock_versions(version, package_names)
    changelog = changelog_text()
    if args.candidate or args.release:
        validate_release_heading(version, changelog)
    if args.release:
        validate_release_tag(version)
    if args.require_clean:
        validate_clean_worktree()

    selected_mode = "release" if args.release else "candidate" if args.candidate else "development"
    print(f"release metadata ok: version={version} mode={selected_mode}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
