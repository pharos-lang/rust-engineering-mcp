#!/usr/bin/env python3
"""Reject missing or structurally empty SonarCloud coverage inputs."""

from __future__ import annotations

from pathlib import Path
import xml.etree.ElementTree as ET

RUST_LCOV = Path("coverage/rust.lcov")
PYTHON_COBERTURA = Path("coverage/python.xml")


def validate_lcov(path: Path) -> tuple[int, int]:
    sources = 0
    lines_found = 0
    lines_hit = 0
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        if raw_line.startswith("SF:"):
            sources += 1
        elif raw_line.startswith("LF:"):
            lines_found += int(raw_line.removeprefix("LF:"))
        elif raw_line.startswith("LH:"):
            lines_hit += int(raw_line.removeprefix("LH:"))
    if sources == 0 or lines_found <= 0:
        raise RuntimeError(f"Rust LCOV report is empty: {path}")
    if lines_hit < 0 or lines_hit > lines_found:
        raise RuntimeError(f"Rust LCOV line counters are inconsistent: {path}")
    return lines_hit, lines_found


def validate_cobertura(path: Path) -> tuple[int, int]:
    root = ET.parse(path).getroot()
    if root.tag != "coverage":
        raise RuntimeError(f"unexpected Cobertura root element in {path}: {root.tag}")
    lines_valid = int(root.attrib.get("lines-valid", "0"))
    lines_covered = int(root.attrib.get("lines-covered", "0"))
    if lines_valid <= 0:
        raise RuntimeError(f"Python Cobertura report is empty: {path}")
    if lines_covered < 0 or lines_covered > lines_valid:
        raise RuntimeError(f"Python Cobertura line counters are inconsistent: {path}")
    return lines_covered, lines_valid


def main() -> None:
    rust_hit, rust_found = validate_lcov(RUST_LCOV)
    python_hit, python_found = validate_cobertura(PYTHON_COBERTURA)
    print(
        "coverage inputs valid: "
        f"Rust {rust_hit}/{rust_found} lines; "
        f"Python {python_hit}/{python_found} lines"
    )


if __name__ == "__main__":
    main()
