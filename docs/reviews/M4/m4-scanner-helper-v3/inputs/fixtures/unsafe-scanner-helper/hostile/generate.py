#!/usr/bin/env python3
"""Generate ADR-069 adversarial parser inputs for guest-only qualification.

This script is intentionally outside Cargo's test discovery. Do not run the
generated sources through the helper on the host.
"""

from argparse import ArgumentParser
from pathlib import Path


def cases(depth: int) -> dict[str, str]:
    return {
        "nested-delimiters.rs": "fn f() { let _ = " + "(" * depth + "0" + ")" * depth + "; }\n",
        "unary-not.rs": "fn f() { let _ = " + "!" * depth + "true; }\n",
        "unary-reference.rs": "fn f(x: bool) { let _ = " + "&" * depth + "x; }\n",
        "right-associative.rs": (
            "fn f() { let (" + ",".join(f"v{i}" for i in range(depth)) + ") = "
            + "(" + ",".join("false" for _ in range(depth)) + "); "
            + " = ".join(f"v{i}" for i in range(depth)) + " = false; }\n"
        ),
        "nested-types.rs": "type T = " + "Option<" * depth + "u8" + ">" * depth + ";\n",
        "deep-macro-tree.rs": "macro_rules! m { () => { " + "(" * depth + "unsafe {}" + ")" * depth + " } }\n",
        "many-flat-tokens.rs": "fn f() { " + "let _ = 0; " * depth + "}\n",
    }


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--depth", type=int, default=50_000)
    arguments = parser.parse_args()
    if arguments.depth < 0:
        parser.error("--depth must be non-negative")
    generated = cases(arguments.depth)
    oversized = [
        f"{name} ({len(source.encode('utf-8'))} bytes)"
        for name, source in generated.items()
        if len(source.encode("utf-8")) > 1024 * 1024
    ]
    if oversized:
        parser.error("generated cases exceed 1 MiB: " + ", ".join(oversized))
    arguments.output.mkdir(parents=True, exist_ok=True)
    for name, source in generated.items():
        (arguments.output / name).write_bytes(source.encode("utf-8"))


if __name__ == "__main__":
    main()
