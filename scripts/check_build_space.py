#!/usr/bin/env python3
"""Fail early when a full Rust/Iced release train lacks working space."""

from __future__ import annotations

import argparse
import os
import shutil
import sys
from pathlib import Path


def gibibytes(value: int) -> float:
    return value / (1024**3)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--path",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="filesystem path whose free space should be checked",
    )
    parser.add_argument(
        "--min-free-gib",
        type=float,
        default=float(os.environ.get("DESKHALLOUMI_MIN_FREE_GIB", "16")),
        help="required free GiB (default: 16 or DESKHALLOUMI_MIN_FREE_GIB)",
    )
    args = parser.parse_args()
    if args.min_free_gib < 0:
        parser.error("--min-free-gib must be non-negative")

    path = args.path.resolve()
    usage = shutil.disk_usage(path)
    free_gib = gibibytes(usage.free)
    if free_gib + 1e-9 < args.min_free_gib:
        target = path / "target"
        target_note = ""
        if target.exists():
            target_note = f" The Cargo target directory exists at '{target}'."
        print(
            f"insufficient release build space: {free_gib:.1f} GiB free; "
            f"{args.min_free_gib:.1f} GiB required.{target_note} "
            "Run `cargo clean` or choose a larger CARGO_TARGET_DIR before retrying.",
            file=sys.stderr,
        )
        return 1

    print(
        f"release build space ok: {free_gib:.1f} GiB free "
        f"(minimum {args.min_free_gib:.1f} GiB)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
