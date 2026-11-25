#!/usr/bin/env python3
"""Enforce workspace crate layering (see AGENTS.md).

Surfaces: biz/* (C-end) and ops/* (reserved; add when ready).
Contracts: contracts/*_contract (shared wire/models).

Forbidden:
  - biz/* → biz/* | ops/* (implementation crates)
  - ops/* → ops/* | biz/* (implementation crates)
  - contracts/* → biz/* | ops/*
  - biz|ops → http_server
  - crates/* → biz|ops|contracts|infra
  - infra/* → biz|ops; contract only via allowlist

Allowed infra → *_contract (frozen; new entries need script + AGENTS + rationale):
  - persistence → identity_contract, file_contract  (toasty models! only)
  - file_attachment → file_contract  (attachment errors/paths align with file)
  - NOT allowed: http_auth / file_attachment → identity_contract
    (account primary key lives in crates/account_id)
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

DEP_RE = re.compile(
    r'^(?P<name>[A-Za-z0-9_-]+)\s*=\s*\{[^}]*path\s*=\s*"(?P<path>[^"]+)"',
    re.M,
)
SIMPLE_PATH_RE = re.compile(
    r"^(?P<name>[A-Za-z0-9_-]+)\s*=\s*\{[^}]*workspace\s*=\s*true",
    re.M,
)

# package name → (layer, kind)
# layer: bin | biz | ops | contract | infra | crate


def classify(rel: Path) -> tuple[str, str]:
    parts = rel.parts
    if parts[0] == "bin":
        return "bin", parts[1]
    if parts[0] == "biz":
        return "biz", parts[1]
    if parts[0] == "ops":
        return "ops", parts[1]
    if parts[0] == "contracts":
        return "contract", parts[1]
    if parts[0] == "infra":
        return "infra", parts[1]
    if parts[0] == "crates":
        return "crate", parts[1]
    return "other", rel.as_posix()


def package_name(cargo_toml: Path) -> str:
    text = cargo_toml.read_text()
    m = re.search(r'(?m)^name\s*=\s*"([^"]+)"', text)
    if not m:
        raise SystemExit(f"no package name in {cargo_toml}")
    return m.group(1)


def workspace_members() -> list[Path]:
    root_toml = (ROOT / "Cargo.toml").read_text()
    m = re.search(r"members\s*=\s*\[(.*?)\]", root_toml, re.S)
    if not m:
        raise SystemExit("no workspace.members")
    return [ROOT / p.strip().strip('"') for p in m.group(1).split(",") if p.strip().strip('"')]


def deps_of(cargo_toml: Path) -> set[str]:
    """Return workspace/path dependency package names (deps + dev-deps + build)."""
    text = cargo_toml.read_text()
    names: set[str] = set()
    for m in SIMPLE_PATH_RE.finditer(text):
        names.add(m.group("name"))
    for m in DEP_RE.finditer(text):
        names.add(m.group("name"))
    return names


# Frozen allowlist. Adding a row requires: update this map, AGENTS.md layering
# exceptions table, and a one-line rationale comment here.
INFRA_CONTRACT_ALLOW: dict[str, set[str]] = {
    # persistence: register toasty models only (no business logic)
    "persistence": {"identity_contract", "file_contract"},
    # file_attachment: FileError codes + path helpers must stay aligned with file
    "file_attachment": {"file_contract"},
}


def main() -> int:
    members = workspace_members()
    pkg_to_meta: dict[str, tuple[str, str, Path]] = {}
    for member in members:
        cargo = member / "Cargo.toml"
        if not cargo.is_file():
            continue
        name = package_name(cargo)
        layer, kind = classify(member.relative_to(ROOT))
        pkg_to_meta[name] = (layer, kind, member)

    errors: list[str] = []

    biz_impls = {n for n, (layer, _, _) in pkg_to_meta.items() if layer == "biz"}
    ops_impls = {n for n, (layer, _, _) in pkg_to_meta.items() if layer == "ops"}
    surface_impls = biz_impls | ops_impls
    contracts = {n for n, (layer, _, _) in pkg_to_meta.items() if layer == "contract"}
    infras = {n for n, (layer, _, _) in pkg_to_meta.items() if layer == "infra"}
    kits = {n for n, (layer, _, _) in pkg_to_meta.items() if layer == "crate"}

    for name, (layer, _kind, member) in sorted(pkg_to_meta.items()):
        cargo = member / "Cargo.toml"
        deps = deps_of(cargo) & set(pkg_to_meta.keys())

        if layer in {"biz", "ops"}:
            bad_surf = deps & surface_impls - {name}
            if bad_surf:
                errors.append(
                    f"{name}: {layer} must not depend on other surface impls {sorted(bad_surf)}"
                )
            if "http_server" in deps:
                errors.append(f"{name}: {layer} must not depend on http_server")

        if layer == "contract":
            bad = deps & surface_impls
            if bad:
                errors.append(f"{name}: contract must not depend on surfaces {sorted(bad)}")

        if layer == "crate":
            bad = (deps & surface_impls) | (deps & contracts) | (deps & infras)
            if bad:
                errors.append(f"{name}: crate/kit must not depend on template layers {sorted(bad)}")

        if layer == "infra":
            bad_surf = deps & surface_impls
            if bad_surf:
                errors.append(f"{name}: infra must not depend on surface impls {sorted(bad_surf)}")
            contract_deps = deps & contracts
            allowed = INFRA_CONTRACT_ALLOW.get(name, set())
            extra = contract_deps - allowed
            if extra:
                errors.append(
                    f"{name}: infra→contract only via allowlist {sorted(allowed)}; got extra {sorted(extra)}"
                )

    if errors:
        print("layering check FAILED:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    print(
        f"layering OK ({len(pkg_to_meta)} crates; "
        f"{len(biz_impls)} biz, {len(ops_impls)} ops, {len(contracts)} contracts, "
        f"{len(infras)} infra, {len(kits)} crates)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
