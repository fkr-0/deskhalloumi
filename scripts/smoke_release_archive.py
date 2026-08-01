#!/usr/bin/env python3
"""Verify a DeskHalloumi release archive without trusting the source tree.

The smoke test verifies an optional SHA-256 sidecar, rejects unsafe archive
members, extracts the archive, installs it into a temporary prefix, exercises
all primary and compatibility binaries, and proves that the installed prefix
can be removed cleanly.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import tarfile
import tempfile

EXPECTED_BINARIES = (
    "deskhalloumi",
    "deskhalloumi-bar",
    "deskhalloumi-copyq",
    "deskhalloumi-filter-tab",
    "deskhalloumi-i3-vis",
    "deskhalloumi-hotkeyd",
    "unilii",
    "unilii-bar",
    "unilii-copyq",
    "unilii-filter-tab",
    "unilii-i3-vis",
    "unilii-hotkeyd",
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_checksum(archive: Path, checksum: Path) -> None:
    fields = checksum.read_text(encoding="utf-8").strip().split()
    if len(fields) != 2:
        raise ValueError(f"invalid checksum file {checksum}: expected '<sha256> <basename>'")
    expected_hash, recorded_name = fields
    recorded_name = recorded_name.removeprefix("*")
    if recorded_name != archive.name:
        raise ValueError(
            f"checksum names {recorded_name!r}, expected archive basename {archive.name!r}"
        )
    actual_hash = sha256_file(archive)
    if actual_hash != expected_hash:
        raise ValueError(
            f"checksum mismatch for {archive.name}: expected {expected_hash}, got {actual_hash}"
        )


def validate_member(member: tarfile.TarInfo, expected_root: str) -> PurePosixPath:
    path = PurePosixPath(member.name)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError(f"unsafe archive member path: {member.name!r}")
    if not path.parts or path.parts[0] != expected_root:
        raise ValueError(
            f"archive member {member.name!r} is outside expected root {expected_root!r}"
        )
    if member.issym() or member.islnk() or member.isdev() or member.isfifo():
        raise ValueError(f"unsupported archive member type: {member.name!r}")
    if not (member.isdir() or member.isfile()):
        raise ValueError(f"unexpected archive member type: {member.name!r}")
    return path


def extract_safely(archive: Path, destination: Path, expected_root: str) -> Path:
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        if not members:
            raise ValueError(f"archive {archive} is empty")
        validated = [(member, validate_member(member, expected_root)) for member in members]
        for member, relative in validated:
            target = destination.joinpath(*relative.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                target.chmod(stat.S_IMODE(member.mode))
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            source = tar.extractfile(member)
            if source is None:
                raise ValueError(f"cannot read archive member {member.name!r}")
            with source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
            target.chmod(stat.S_IMODE(member.mode))
    return destination / expected_root


def install_tree(extracted_root: Path, prefix: Path) -> None:
    for directory in ("bin", "share"):
        source = extracted_root / directory
        if source.exists():
            shutil.copytree(source, prefix / directory, copy_function=shutil.copy2)
    if not (prefix / "bin").is_dir():
        raise ValueError("release archive does not contain bin/")


def run_command(command: list[str], *, env: dict[str, str]) -> None:
    completed = subprocess.run(
        command,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=15,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )


def smoke_installed_prefix(prefix: Path) -> None:
    bin_dir = prefix / "bin"
    env = os.environ.copy()
    env["PATH"] = f"{bin_dir}:{env.get('PATH', '')}"
    env.setdefault("XDG_RUNTIME_DIR", str(prefix / "runtime"))
    Path(env["XDG_RUNTIME_DIR"]).mkdir(parents=True, exist_ok=True)

    missing = [name for name in EXPECTED_BINARIES if not (bin_dir / name).is_file()]
    if missing:
        raise ValueError(f"release archive is missing binaries: {', '.join(missing)}")

    for name in EXPECTED_BINARIES:
        executable = bin_dir / name
        if not os.access(executable, os.X_OK):
            raise ValueError(f"installed binary is not executable: {executable}")
        run_command([str(executable), "--help"], env=env)

    run_command([str(bin_dir / "deskhalloumi-bar"), "--runtime-contract"], env=env)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help="release .tar.gz archive")
    parser.add_argument(
        "--checksum",
        type=Path,
        help="optional sha256 sidecar; defaults to <archive>.sha256 when present",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    archive = args.archive.resolve()
    if not archive.is_file():
        raise SystemExit(f"archive not found: {archive}")
    if not archive.name.endswith(".tar.gz"):
        raise SystemExit(f"expected a .tar.gz archive: {archive}")

    checksum = args.checksum
    if checksum is None:
        candidate = Path(f"{archive}.sha256")
        checksum = candidate if candidate.is_file() else None
    if checksum is not None:
        verify_checksum(archive, checksum.resolve())

    expected_root = archive.name.removesuffix(".tar.gz")
    with tempfile.TemporaryDirectory(prefix="deskhalloumi-release-smoke-") as temp:
        work = Path(temp)
        extracted_root = extract_safely(archive, work / "extract", expected_root)
        prefix = work / "prefix"
        install_tree(extracted_root, prefix)
        smoke_installed_prefix(prefix)
        shutil.rmtree(prefix)
        if prefix.exists():
            raise RuntimeError(f"temporary installation prefix still exists: {prefix}")

    checksum_note = f" and {checksum}" if checksum is not None else ""
    print(f"release archive smoke passed: {archive}{checksum_note}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
