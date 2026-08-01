#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Compare every tape's current output against its committed golden trace, and
report which *fields* changed rather than that the files differ.

This exists to answer one question before re-recording baselines: "did I change
behaviour, or only add a column?" Adding a field to the probe rewrites every
baseline in traces/, and a plain diff cannot tell that apart from a jump arc
moving two pixels. Ignoring the new field names and finding zero remaining
differences is the proof.

Exits nonzero if any tape differs in a field that was not ignored, so it also
works as a gate before `UPDATE_TRACES=1 cargo test --test traces`.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path


def repo_root(start: Path) -> Path:
    """Walk up to the directory holding Cargo.toml."""
    for candidate in [start, *start.parents]:
        if (candidate / "Cargo.toml").is_file():
            return candidate
    sys.exit(f"error: no Cargo.toml at or above {start}; run inside the repo")


def replay(root: Path, tape: Path, out: Path) -> bool:
    """Replay a tape, returning whether its own assertions held.

    A failing assertion is not a replay failure: `sim` still writes the trace,
    and comparing it is exactly what you want when a change broke a tape and
    you are working out how far the damage spread. Only a missing trace file
    is fatal.
    """
    result = subprocess.run(
        [
            "cargo", "run", "--quiet", "--bin", "sim", "--",
            "--tape", str(tape), "--trace", str(out), "--quiet",
        ],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if not out.exists():
        sys.exit(
            f"error: replaying {tape.name} produced no trace\n"
            f"{result.stdout}\n{result.stderr}"
        )
    return result.returncode == 0


def differing_fields(old: dict, new: dict, ignore: set[str]) -> set[str]:
    """Field names that differ between two trace frames.

    NPC fields are reported as `npcs.<field>` so a change inside a nested entry
    is attributable rather than showing up as the whole `npcs` array.
    """
    changed: set[str] = set()
    for key in set(old) | set(new):
        if key in ignore or key == "npcs":
            continue
        if old.get(key) != new.get(key):
            changed.add(key)

    old_npcs, new_npcs = old.get("npcs", []), new.get("npcs", [])
    if len(old_npcs) != len(new_npcs):
        changed.add("npcs (count)")
    for o, n in zip(old_npcs, new_npcs):
        for key in set(o) | set(n):
            if key in ignore:
                continue
            if o.get(key) != n.get(key):
                changed.add(f"npcs.{key}")
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--ignore",
        nargs="*",
        default=[],
        metavar="FIELD",
        help="field names to treat as expected changes, e.g. --ignore hp iframes. "
        "Applies to player and NPC fields alike. Zero differences once these are "
        "ignored is the proof that only the new columns moved.",
    )
    parser.add_argument(
        "--tape",
        metavar="NAME",
        help="check only this tape (stem, e.g. knight_kill)",
    )
    parser.add_argument(
        "--repo",
        type=Path,
        default=Path.cwd(),
        help="path inside the repo (default: cwd)",
    )
    args = parser.parse_args()

    root = repo_root(args.repo.resolve())
    tapes = sorted((root / "tapes").glob("*.tape"))
    if args.tape:
        tapes = [t for t in tapes if t.stem == args.tape]
        if not tapes:
            sys.exit(f"error: no tape named `{args.tape}` in {root / 'tapes'}")

    ignore = set(args.ignore)
    if ignore:
        print(f"ignoring fields: {', '.join(sorted(ignore))}\n")

    problems = 0
    with tempfile.TemporaryDirectory() as tmp:
        for tape in tapes:
            out = Path(tmp) / f"{tape.stem}.jsonl"
            asserts_held = replay(root, tape, out)
            note = "" if asserts_held else "   [tape assertions FAILED]"
            baseline = root / "traces" / f"{tape.stem}.jsonl"

            if not baseline.exists():
                print(f"  {tape.stem:<20} NEW — no baseline recorded yet{note}")
                continue

            old = [json.loads(l) for l in baseline.read_text().splitlines() if l.strip()]
            new = [json.loads(l) for l in out.read_text().splitlines() if l.strip()]

            if len(old) != len(new):
                print(f"  {tape.stem:<20} LENGTH {len(old)} -> {len(new)} ticks{note}")
                problems += 1
                continue

            changed: set[str] = set()
            first_tick = None
            for i, (o, n) in enumerate(zip(old, new)):
                fields = differing_fields(o, n, ignore)
                if fields and first_tick is None:
                    first_tick = i
                changed |= fields

            if not changed:
                print(f"  {tape.stem:<20} unchanged ({len(old)} ticks){note}")
            else:
                print(
                    f"  {tape.stem:<20} DIFFERS from line {first_tick}: "
                    f"{', '.join(sorted(changed))}{note}"
                )
                problems += 1

    print()
    if problems:
        print(
            f"{problems} tape(s) changed in fields you did not declare.\n"
            "Either that is a behaviour change worth explaining, or add the field\n"
            "to --ignore. Only re-record once every remaining difference is\n"
            "accounted for:  UPDATE_TRACES=1 cargo test --test traces"
        )
        return 1

    print("No undeclared changes. Safe to re-record if fields were added.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
