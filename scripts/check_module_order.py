#!/usr/bin/env python3
"""Assert bin/api module order: FTL/mapper features before http_server.

Discovers features that call register_ftl / register_domain_mapper (by scanning
features/*/ sources, ignoring // line comments), then checks each appears before
http_server in bin/api/main.rs. http_server::register seals locale + domain mappers.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
API_MAIN = ROOT / "bin" / "api" / "main.rs"
FEATURES = ROOT / "features"

MODULE_RE = re.compile(
    r"\.module\(\s*(?P<name>[A-Za-z0-9_]+)\s*::\s*module\s*\(\s*\)\s*\)"
)
REGISTER_RE = re.compile(r"\bregister_(?:ftl|domain_mapper)\s*\(")


def strip_line_comments(text: str) -> str:
    """Drop `// …` tails so commented examples do not false-positive."""
    lines: list[str] = []
    for line in text.splitlines():
        code, _sep, _comment = line.partition("//")
        lines.append(code)
    return "\n".join(lines)


def features_that_register_locale() -> set[str]:
    found: set[str] = set()
    if not FEATURES.is_dir():
        return found
    for path in FEATURES.rglob("*.rs"):
        parts = path.relative_to(FEATURES).parts
        if not parts:
            continue
        crate = parts[0]
        if crate.endswith("_contract"):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as err:
            print(f"module-order check FAILED: read {path}: {err}", file=sys.stderr)
            raise SystemExit(1) from err
        if REGISTER_RE.search(strip_line_comments(text)):
            found.add(crate)
    return found


def main() -> int:
    required = features_that_register_locale()
    if not required:
        print(
            "module-order check FAILED: no feature calls register_ftl/"
            "register_domain_mapper (scan broken?)",
            file=sys.stderr,
        )
        return 1

    text = API_MAIN.read_text(encoding="utf-8")
    modules = [m.group("name") for m in MODULE_RE.finditer(text)]
    if not modules:
        print(
            f"module-order check FAILED: no .module(...::module()) in {API_MAIN}",
            file=sys.stderr,
        )
        return 1
    if "http_server" not in modules:
        print(
            "module-order check FAILED: http_server::module() missing",
            file=sys.stderr,
        )
        return 1

    http_idx = modules.index("http_server")
    errors: list[str] = []
    for name in sorted(required):
        if name not in modules:
            errors.append(
                f"{name}: registers FTL/mapper but missing "
                f".module({name}::module()) before http_server"
            )
            continue
        if modules.index(name) >= http_idx:
            errors.append(
                f"{name}: must appear before http_server "
                f"(got order: {' → '.join(modules)})"
            )

    if errors:
        print("module-order check FAILED:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    print(
        f"module-order OK ({len(modules)} modules; "
        f"{', '.join(sorted(required))} before http_server)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
