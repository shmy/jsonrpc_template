#!/usr/bin/env python3
"""Scaffold a leaf biz surface crate from docs/biz-template/ into biz/<name>/."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = ROOT / "docs" / "biz-template"
BIZ = ROOT / "biz"
ROOT_TOML = ROOT / "Cargo.toml"

NAME_RE = re.compile(r"^[a-z][a-z0-9_]*$")
MEMBERS_RE = re.compile(r"members\s*=\s*\[(.*?)\]", re.S)
PKG_NAME_RE = re.compile(r'(?m)^name\s*=\s*"([^"]+)"')


def die(msg: str) -> None:
    print(f"new-biz FAILED: {msg}", file=sys.stderr)
    raise SystemExit(1)


def workspace_package_names() -> set[str]:
    """Package names of all workspace members (from each member Cargo.toml)."""
    text = ROOT_TOML.read_text(encoding="utf-8")
    m = MEMBERS_RE.search(text)
    if not m:
        die("no workspace.members in Cargo.toml")
    names: set[str] = set()
    for raw in m.group(1).split(","):
        rel = raw.strip().strip('"')
        if not rel:
            continue
        cargo = ROOT / rel / "Cargo.toml"
        if not cargo.is_file():
            die(f"workspace member missing Cargo.toml: {rel}")
        pkg = PKG_NAME_RE.search(cargo.read_text(encoding="utf-8"))
        if not pkg:
            die(f"no package name in {cargo}")
        names.add(pkg.group(1))
    return names


def rewrite(text: str, name: str) -> str:
    """Replace FEATURE_NAME / FEATURE placeholders with the concrete crate name."""
    out = text.replace("FEATURE_NAME", name)
    return out.replace("FEATURE", name)


def copy_tree(name: str) -> Path:
    dest = BIZ / name
    if dest.exists():
        die(f"{dest} already exists")
    if not TEMPLATE.is_dir():
        die(f"template missing: {TEMPLATE}")

    for src in TEMPLATE.rglob("*"):
        if src.is_dir():
            continue
        rel = src.relative_to(TEMPLATE)
        if rel.as_posix() == "README.md":
            continue
        target = dest / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        raw = src.read_text(encoding="utf-8")
        target.write_text(rewrite(raw, name), encoding="utf-8")
    return dest


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: just new-biz <name>", file=sys.stderr)
        print("  name: snake_case biz surface (e.g. billing)", file=sys.stderr)
        return 2
    name = argv[1]
    if not NAME_RE.match(name):
        die("name must match ^[a-z][a-z0-9_]*$")
    if name.endswith("_contract"):
        die("pass the surface name without _contract; create contracts/<name>_contract separately")

    taken = workspace_package_names()
    if name in taken:
        die(
            f"package name `{name}` already used by a workspace member "
            f"(also ensure biz/{name} does not already exist)"
        )

    dest = copy_tree(name)
    print(f"scaffolded {dest.relative_to(ROOT)}")
    print()
    print("Next steps:")
    print(f'  1. Add "biz/{name}" to workspace.members in Cargo.toml')
    print(f'  2. Add {name} = {{ version = "0.1.0", path = "biz/{name}" }}')
    print("     under [workspace.dependencies]")
    print(f"  3. bin/biz/Cargo.toml: depend on {name}; main.rs:")
    print(f"       .module({name}::module())  // BEFORE http_server")
    print("  4. Domain errors → shared/l10n.rs + locales (use locale::register_locale_bundle)")
    print("  5. Optional contracts/<name>_contract → persistence toasty::models! + just migration-generate")
    print(f"  6. just layering && just check-fast {name}")
    print()
    print("Checklist: docs/biz-template/README.md")
    print("Later ops surfaces go under ops/ (not created yet); same rules, separate bin.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
