#!/usr/bin/env python3
"""Assert bin composition roots: FTL/mapper surfaces before http_server.

Discovers api/* and admin/* crates that call register_ftl / register_domain_mapper /
register_locale_bundle, then checks each appears before http_server in each
bin/*/main.rs that wires http_server.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "bin"
SURFACES = (ROOT / "api", ROOT / "admin")

MODULE_RE = re.compile(
    r"\.module\(\s*(?P<name>[A-Za-z0-9_]+)\s*::\s*module\s*\(\s*\)\s*\)"
)
REGISTER_RE = re.compile(r"\bregister_(?:ftl|domain_mapper|locale_bundle)\s*\(")


def strip_line_comments(text: str) -> str:
    """Drop `// …` tails so commented examples do not false-positive."""
    lines: list[str] = []
    for line in text.splitlines():
        code, _sep, _comment = line.partition("//")
        lines.append(code)
    return "\n".join(lines)


def surfaces_that_register_locale() -> set[str]:
    found: set[str] = set()
    for surface_root in SURFACES:
        if not surface_root.is_dir():
            continue
        for path in surface_root.rglob("*.rs"):
            parts = path.relative_to(surface_root).parts
            if not parts:
                continue
            crate = parts[0]
            try:
                text = path.read_text(encoding="utf-8")
            except OSError as err:
                print(f"module-order check FAILED: read {path}: {err}", file=sys.stderr)
                raise SystemExit(1) from err
            if REGISTER_RE.search(strip_line_comments(text)):
                found.add(crate)
    return found


def check_main(main: Path, required: set[str]) -> list[str]:
    text = main.read_text(encoding="utf-8")
    modules = [m.group("name") for m in MODULE_RE.finditer(text)]
    if not modules:
        return [f"{main}: no .module(...::module())"]
    if "http_server" not in modules:
        # Composition roots without http_server (e.g. migrator) are ignored by caller.
        return []
    http_idx = modules.index("http_server")
    errors: list[str] = []
    # Only enforce crates that this main actually modules.
    for name in sorted(required & set(modules)):
        if modules.index(name) >= http_idx:
            errors.append(
                f"{main.relative_to(ROOT)}: {name} must appear before http_server "
                f"(got order: {' → '.join(modules)})"
            )
    for name in sorted(required - set(modules)):
        # Registered locale in a surface that this binary does not wire — OK
        # (e.g. admin-only crates when checking bin/api).
        pass
    # Surfaces this binary modules that register locale must be before http_server —
    # covered above. Also: if binary modules a locale surface, it must be in required
    # (scan found it). If scan missed, soft.
    return errors


def mains_with_http_server() -> list[Path]:
    found: list[Path] = []
    if not BIN.is_dir():
        return found
    for main in BIN.glob("*/main.rs"):
        text = main.read_text(encoding="utf-8")
        if "http_server::module" in text or re.search(
            r"\.module\(\s*http_server\s*::\s*module\s*\(\s*\)\s*\)", text
        ):
            found.append(main)
    return found


def main() -> int:
    required = surfaces_that_register_locale()
    if not required:
        print(
            "module-order check FAILED: no api/admin crate calls register_ftl/"
            "register_domain_mapper/register_locale_bundle (scan broken?)",
            file=sys.stderr,
        )
        return 1

    mains = mains_with_http_server()
    if not mains:
        print(
            "module-order check FAILED: no bin/*/main.rs wires http_server",
            file=sys.stderr,
        )
        return 1

    errors: list[str] = []
    for main_path in mains:
        errors.extend(check_main(main_path, required))

    # Every locale-registering surface must appear in at least one http_server bin
    # before seal — otherwise dead registration or wrong bin.
    wired: set[str] = set()
    for main_path in mains:
        text = main_path.read_text(encoding="utf-8")
        wired.update(m.group("name") for m in MODULE_RE.finditer(text))
    for name in sorted(required - wired):
        errors.append(
            f"{name}: registers FTL/mapper but not .module()'d in any http_server bin"
        )

    if errors:
        print("module-order check FAILED:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    mains_rel = ", ".join(str(p.relative_to(ROOT)) for p in mains)
    print(
        f"module-order OK ({mains_rel}; "
        f"{', '.join(sorted(required))} before http_server)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
