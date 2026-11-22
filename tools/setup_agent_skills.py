#!/usr/bin/env python3
"""Install the Roadside-Picnic agent skills from the canonical tooling repo."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "agent-skills.json"


def run(command: list[str], cwd: Path | None = None) -> None:
    environment = os.environ.copy()
    environment["GIT_TERMINAL_PROMPT"] = "0"
    subprocess.run(command, cwd=cwd, check=True, env=environment)


def cache_root() -> Path:
    if os.name == "nt":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
        return base / "Roadside-Picnic" / "agentic-tooling"
    return (
        Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
        / "roadside-picnic"
        / "agentic-tooling"
    )


def load_manifest() -> dict:
    try:
        manifest = json.loads(MANIFEST.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"could not read {MANIFEST}: {exc}") from exc
    required = ("source", "skills", "targets")
    missing = [key for key in required if key not in manifest]
    if missing:
        raise RuntimeError(f"{MANIFEST} is missing: {', '.join(missing)}")
    return manifest


def source_checkout(manifest: dict, requested: Path | None, update: bool) -> Path:
    env = os.environ.get("ROADSIDE_PICNIC_AGENTIC_TOOLING")
    candidates = [
        requested,
        Path(env) if env else None,
        Path.home() / "Projects" / "agentic" / "agentic-tooling",
    ]
    for candidate in candidates:
        if candidate and (candidate / ".git").is_dir():
            return candidate.resolve()

    cached = cache_root()
    source = manifest["source"]
    if not (cached / ".git").is_dir():
        cached.parent.mkdir(parents=True, exist_ok=True)
        repositories = [
            source["repository"],
            source.get("fallback_repository"),
        ]
        error = None
        for repository in filter(None, repositories):
            print(f"Cloning canonical skills from {repository} to {cached}")
            try:
                run(["git", "clone", "--quiet", repository, str(cached)])
                error = None
                break
            except subprocess.CalledProcessError as exc:
                error = exc
                if cached.exists():
                    shutil.rmtree(cached)
        if error is not None:
            raise RuntimeError(
                "cannot access the canonical skill repository; set "
                "ROADSIDE_PICNIC_AGENTIC_TOOLING or pass --source-dir"
            ) from error
    elif update:
        print(f"Updating cached skills in {cached}")
        run(["git", "fetch", "--quiet", "origin"], cached)

    revision = source["revision"]
    current = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=cached,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if current != revision:
        run(["git", "fetch", "--quiet", "origin", revision], cached)
        run(["git", "checkout", "--quiet", "--detach", revision], cached)
    return cached


def tree_digest(path: Path) -> tuple[tuple[str, str], ...]:
    digest = []
    for item in sorted(path.rglob("*")):
        if not item.is_file():
            continue
        relative = item.relative_to(path).as_posix()
        content = hashlib.sha256(item.read_bytes()).hexdigest()
        digest.append((relative, content))
    return tuple(digest)


def remove_existing(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


def install_skill(
    source: Path,
    destination: Path,
    force: bool,
    copy_only: bool,
) -> str:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink() and destination.resolve() == source.resolve():
        return "up to date"
    if destination.exists() or destination.is_symlink():
        if not force:
            if destination.is_dir() and not destination.is_symlink():
                if tree_digest(destination) == tree_digest(source):
                    return "already installed"
            return "exists; rerun with --force to replace"
        remove_existing(destination)

    if not copy_only:
        try:
            destination.symlink_to(source, target_is_directory=True)
            return "linked"
        except OSError as exc:
            if os.name != "nt":
                print(f"  symlink unavailable ({exc}); copying")

    shutil.copytree(source, destination)
    return "copied"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source-dir",
        type=Path,
        help="existing agentic-tooling checkout; otherwise a pinned cache is used",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="fetch the cached source before installing",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="replace an existing non-identical skill directory",
    )
    parser.add_argument(
        "--copy",
        action="store_true",
        help="copy skill directories instead of creating symlinks",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest = load_manifest()
    source = source_checkout(manifest, args.source_dir, args.update)
    skills_root = source / "skills"
    target_roots = [Path.home() / target for target in manifest["targets"]]

    skipped = False
    for name in manifest["skills"]:
        skill = skills_root / name
        skill_file = skill / "SKILL.md"
        if not skill_file.is_file():
            print(f"missing skill in canonical source: {skill}", file=sys.stderr)
            return 1
        for root in target_roots:
            destination = root / name
            status = install_skill(skill, destination, args.force, args.copy)
            print(f"{status:36} {destination}")
            skipped |= status.startswith("exists;")

    if skipped:
        print("Some existing skills were left unchanged; review them and rerun with --force.")
    else:
        print("Roadside-Picnic agent skills are installed.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as exc:
        print(f"setup failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
