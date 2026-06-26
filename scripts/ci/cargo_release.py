#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

import tomllib


ROOT = Path(__file__).resolve().parents[2]
ROOT_MANIFEST = ROOT / "Cargo.toml"
PACKAGE_ORDER = [
    "ttsync-contract",
    "ttsync-core",
    "ttsync-fs",
    "ttsync-http",
    "ttsync-client",
    "tt-sync",
]
INTERNAL_DEPENDENCIES = [
    "ttsync-contract",
    "ttsync-core",
    "ttsync-fs",
    "ttsync-http",
    "ttsync-client",
]


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as file:
        return tomllib.load(file)


def load_git_toml(ref: str, path: str) -> dict[str, Any] | None:
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    return tomllib.loads(result.stdout)


def workspace_version(manifest: dict[str, Any]) -> str:
    version = maybe_workspace_version(manifest)
    if version is None:
        raise SystemExit("workspace.package.version must be set in the root Cargo.toml")
    return version


def maybe_workspace_version(manifest: dict[str, Any]) -> str | None:
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    return version if isinstance(version, str) and version else None


def package_version(package: dict[str, Any]) -> str | None:
    version = package.get("version")
    if isinstance(version, str):
        return version
    if isinstance(version, dict) and version.get("workspace") is True:
        return None
    raise SystemExit(f"{package.get('name', '<unknown>')} has an unsupported package.version")


def workspace_members(manifest: dict[str, Any]) -> list[tuple[Path, dict[str, Any]]]:
    members = manifest.get("workspace", {}).get("members")
    if not isinstance(members, list):
        raise SystemExit("workspace.members must be set in the root Cargo.toml")

    loaded = []
    for member in members:
        if not isinstance(member, str):
            raise SystemExit("workspace.members must only contain string paths")
        path = ROOT / member / "Cargo.toml"
        loaded.append((path, load_toml(path)))
    return loaded


def validate() -> None:
    root = load_toml(ROOT_MANIFEST)
    version = workspace_version(root)
    members = workspace_members(root)
    package_names = [manifest["package"]["name"] for _, manifest in members]
    errors: list[str] = []

    if package_names != PACKAGE_ORDER:
        errors.append(
            "workspace member order must match the crates.io publish order: "
            + ", ".join(PACKAGE_ORDER)
        )

    for path, manifest in members:
        package = manifest["package"]
        declared_version = package_version(package)
        if declared_version is not None and declared_version != version:
            errors.append(f"{path.relative_to(ROOT)} declares {declared_version}, expected {version}")

    workspace_dependencies = root.get("workspace", {}).get("dependencies", {})
    for dependency in INTERNAL_DEPENDENCIES:
        spec = workspace_dependencies.get(dependency)
        if not isinstance(spec, dict):
            errors.append(f"workspace dependency {dependency} must use a table dependency")
            continue
        if spec.get("version") != version:
            errors.append(f"workspace dependency {dependency} must require version {version}")
        if "path" not in spec:
            errors.append(f"workspace dependency {dependency} must keep its local path")

    if errors:
        raise SystemExit("\n".join(errors))

    print(f"workspace version {version} verified for {', '.join(package_names)}")


def is_published(crate_name: str, version: str) -> bool:
    url = "https://crates.io/api/v1/crates/{}/{}".format(
        urllib.parse.quote(crate_name, safe=""),
        urllib.parse.quote(version, safe=""),
    )
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "TT-Sync release workflow (https://github.com/Darkatse/TT-Sync)"},
    )
    try:
        with urllib.request.urlopen(request, timeout=20):
            return True
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return False
        raise SystemExit(f"crates.io lookup failed for {crate_name} {version}: HTTP {error.code}")
    except urllib.error.URLError as error:
        raise SystemExit(f"crates.io lookup failed for {crate_name} {version}: {error.reason}")


def main() -> None:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command", required=True)

    subcommands.add_parser("version")
    previous = subcommands.add_parser("previous-version")
    previous.add_argument("--ref", default="HEAD^")
    subcommands.add_parser("validate")
    subcommands.add_parser("packages")
    published = subcommands.add_parser("published")
    published.add_argument("crate_name")
    published.add_argument("version")

    args = parser.parse_args()

    if args.command == "version":
        print(workspace_version(load_toml(ROOT_MANIFEST)))
    elif args.command == "previous-version":
        manifest = load_git_toml(args.ref, "Cargo.toml")
        previous_version = maybe_workspace_version(manifest) if manifest else None
        print(previous_version or "")
    elif args.command == "validate":
        validate()
    elif args.command == "packages":
        print(" ".join(PACKAGE_ORDER))
    elif args.command == "published":
        sys.exit(0 if is_published(args.crate_name, args.version) else 1)


if __name__ == "__main__":
    main()
