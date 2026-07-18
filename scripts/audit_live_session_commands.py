#!/usr/bin/env python3
"""Audit tests for live-session command execution hazards.

The bar runtime supports real i3/sway/network/audio/session commands in normal
usage. Unit tests must not execute those commands against a developer's live
session. This audit flags the patterns that are risky in tests while allowing
non-executing command-render assertions, docs, examples, and explicit guard
classifier tests.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
import re
import sys

REPO_ROOT = Path(__file__).resolve().parents[1]
SKIP_DIRS = {".git", "target"}
RUST_SUFFIX = ".rs"
SHELL_SCRIPT_SUFFIXES = {".sh", ".bash", ".zsh", ".fish"}
EXECUTABLE_CONFIG_NAMES = {"bridge.yml", "bridge.yaml", "Makefile", "justfile", ".envrc"}
ALLOW_REFERENCE_MARKER = "unilii-audit: allow-live-session-command-reference"
ALLOW_EXECUTION_MARKER = "unilii-audit: allow-live-session-command-execution"

# Build sensitive command names in pieces so this audit file itself does not
# become an easy source of false positives for naive greps.
MUTATING_TOOLS = [
    "i3" + "-msg",
    "sway" + "msg",
    "nm" + "cli",
    "pa" + "ctl",
    "wp" + "ctl",
    "system" + "ctl",
    "login" + "ctl",
    "notify" + "-send",
]

SAFE_TEST_NAME_PARTS = (
    "side_effect_guard",
    "guarded_command",
    "backend_presets_resolve",
    "without_executing",
)


EXECUTION_HINTS = (
    "dispatch_workspace_switch",
    "run_script_command",
    "command =",
    "ip_command =",
    "ssid_command =",
    "on_click",
    ".into()",
)

HARMLESS_OVERRIDE_HINTS = (
    'switch_command_template = "printf ',
    'switch_command_template = r"printf ',
)

TEST_ATTR_RE = re.compile(r"^\s*#\s*\[\s*test\s*\]")
FN_RE = re.compile(r"^\s*fn\s+([A-Za-z0-9_]+)\s*\(")
BACKEND_RE = re.compile(r"backend\s*=\s*\\?\"(i3|sway)\\?\"")


class OutputMode(str, Enum):
    QUIET = "quiet"
    NORMAL = "normal"
    VERBOSE = "verbose"


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    test_name: str
    message: str
    snippet: str

    def format(self) -> str:
        rel = self.path.relative_to(REPO_ROOT)
        return f"{rel}:{self.line}: {self.test_name}: {self.message}: {self.snippet.strip()}"



def iter_executable_command_files(root: Path):
    for path in root.rglob("*"):
        if path.is_dir() or any(part in SKIP_DIRS for part in path.parts):
            continue
        if path.name in EXECUTABLE_CONFIG_NAMES:
            yield path
            continue
        if path.suffix in SHELL_SCRIPT_SUFFIXES:
            yield path
            continue
        if ".github" in path.parts and "workflows" in path.parts and path.suffix in {".yml", ".yaml"}:
            yield path
            continue


def audit_executable_command_file(path: Path) -> list[Finding]:
    text = path.read_text(errors="ignore")
    if ALLOW_EXECUTION_MARKER in text:
        return []
    findings: list[Finding] = []
    for line_no, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if any(tool in line for tool in MUTATING_TOOLS):
            findings.append(
                Finding(
                    path=path,
                    line=line_no,
                    test_name="executable-command-file",
                    message="executable file references live-session command without opt-in marker",
                    snippet=line,
                )
            )
    return findings

def iter_rust_files(root: Path):
    for path in root.rglob(f"*{RUST_SUFFIX}"):
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        yield path


def extract_test_blocks(text: str):
    lines = text.splitlines()
    pending_test = False
    i = 0
    while i < len(lines):
        line = lines[i]
        if TEST_ATTR_RE.match(line):
            pending_test = True
            i += 1
            continue
        if pending_test:
            match = FN_RE.match(line)
            if not match:
                i += 1
                continue
            name = match.group(1)
            start = i
            brace_depth = line.count("{") - line.count("}")
            i += 1
            while i < len(lines) and brace_depth > 0:
                brace_depth += lines[i].count("{") - lines[i].count("}")
                i += 1
            end = i
            yield name, start + 1, lines[start:end]
            pending_test = False
            continue
        i += 1


def is_safe_test_name(name: str) -> bool:
    return any(part in name for part in SAFE_TEST_NAME_PARTS)


def audit_test_block(path: Path, name: str, start_line: int, block_lines: list[str]) -> list[Finding]:
    findings: list[Finding] = []
    text = "\n".join(block_lines)

    # The exact dangerous regression: a test config uses a real WM backend and
    # then dispatches a workspace switch. Rendering the command is safe;
    # dispatching it is not.
    has_backend_dispatch = BACKEND_RE.search(text) and "dispatch_workspace_switch" in text
    has_harmless_override = any(hint in text for hint in HARMLESS_OVERRIDE_HINTS)
    if has_backend_dispatch and not has_harmless_override and not is_safe_test_name(name):
        dispatch_line = next(
            (idx for idx, line in enumerate(block_lines, start_line) if "dispatch_workspace_switch" in line),
            start_line,
        )
        findings.append(
            Finding(
                path=path,
                line=dispatch_line,
                test_name=name,
                message="real WM backend test dispatches workspace switch",
                snippet=block_lines[dispatch_line - start_line],
            )
        )

    # Direct references to mutating tools are allowed only in guard/classifier
    # tests. Command rendering assertions should prefer split constants or
    # non-executing helpers; command dispatch tests must use harmless mocks.
    has_explicit_reference_allow = ALLOW_REFERENCE_MARKER in text
    if not is_safe_test_name(name) and not has_explicit_reference_allow:
        for offset, line in enumerate(block_lines):
            if not any(tool in line for tool in MUTATING_TOOLS):
                continue
            if not any(hint in line for hint in EXECUTION_HINTS):
                continue
            # Rendered-command assertions are safe when they use the non-executing helper.
            if "assert_eq!(command" in line or "workspace_switch_command" in text:
                continue
            findings.append(
                Finding(
                    path=path,
                    line=start_line + offset,
                    test_name=name,
                    message="test contains potentially executed live-session command reference",
                    snippet=line,
                )
            )
    return findings


@dataclass(frozen=True)
class AuditSummary:
    rust_files: int = 0
    rust_tests: int = 0
    executable_files: int = 0


def audit(root: Path) -> tuple[list[Finding], AuditSummary]:
    findings: list[Finding] = []
    rust_files = 0
    rust_tests = 0
    executable_files = 0
    for path in iter_rust_files(root):
        rust_files += 1
        text = path.read_text(errors="ignore")
        for name, start_line, block_lines in extract_test_blocks(text):
            rust_tests += 1
            findings.extend(audit_test_block(path, name, start_line, block_lines))
    for path in iter_executable_command_files(root):
        executable_files += 1
        findings.extend(audit_executable_command_file(path))
    return findings, AuditSummary(
        rust_files=rust_files,
        rust_tests=rust_tests,
        executable_files=executable_files,
    )


def parse_output_mode(args: argparse.Namespace) -> OutputMode:
    if args.quiet and args.verbose:
        raise ValueError("--quiet and --verbose are mutually exclusive")
    if args.quiet:
        return OutputMode.QUIET
    if args.verbose:
        return OutputMode.VERBOSE
    return OutputMode.NORMAL


def print_success(mode: OutputMode, summary: AuditSummary) -> None:
    if mode == OutputMode.QUIET:
        return
    print("live-session command audit passed")
    if mode == OutputMode.VERBOSE:
        print(
            "scanned "
            f"{summary.rust_files} Rust files, "
            f"{summary.rust_tests} Rust tests, "
            f"{summary.executable_files} executable command files"
        )


def print_failure(findings: list[Finding], summary: AuditSummary, mode: OutputMode) -> None:
    if mode == OutputMode.QUIET:
        print(f"live-session command audit failed: {len(findings)} finding(s)", file=sys.stderr)
        return
    print("live-session command audit failed:", file=sys.stderr)
    if mode == OutputMode.VERBOSE:
        print(
            "scanned "
            f"{summary.rust_files} Rust files, "
            f"{summary.rust_tests} Rust tests, "
            f"{summary.executable_files} executable command files",
            file=sys.stderr,
        )
    for finding in findings:
        print(finding.format(), file=sys.stderr)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=REPO_ROOT)
    parser.add_argument("--quiet", action="store_true", help="Only print audit failures and suppress success output")
    parser.add_argument("--verbose", action="store_true", help="Print scanned-file/test counts in addition to normal output")
    args = parser.parse_args(argv)
    try:
        output_mode = parse_output_mode(args)
    except ValueError as error:
        parser.error(str(error))
    root = args.root.resolve()
    findings, summary = audit(root)
    if findings:
        print_failure(findings, summary, output_mode)
        return 1
    print_success(output_mode, summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
