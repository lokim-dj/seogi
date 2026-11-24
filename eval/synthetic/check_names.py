#!/usr/bin/env python3
"""Checks that the invented names in the practice scripts are nobody's.

The scripts under eval/synthetic/scripts are public. Their people, sites and
companies are made up, and a made-up name must not happen to be a real one
from your own records. If you keep a private vocabulary on this computer
(see the README: a folder with vocabulary.json, never in this repository),
this compares the scripts against it:

    python3 eval/synthetic/check_names.py
    python3 eval/synthetic/check_names.py --private-vocab ~/somewhere/vocabulary.json

It looks for the private vocabulary at --private-vocab, then at the
SEOGI_PRIVATE_VOCAB environment variable, then at ~/seogi-private. When there
is none it says so and passes: there is nothing to compare against.

What it checks:
- every term on a script's `# vocabulary:` line that is a person, site or
  company in the private vocabulary (the generic words on that line, such as
  materials, are expected to be shared and are not names);
- every person's name of three or more letters from the private vocabulary
  that appears anywhere in a script.

The private file is only read. The only thing printed is the colliding word,
which is already in the public script.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE / "scripts"

# Categories whose entries are names of real people, places or companies.
NAME_CATEGORIES = {"직원", "고객", "현장", "업체", "staff", "customer", "site", "vendor", "person"}
PERSON_CATEGORIES = {"직원", "고객", "staff", "customer", "person"}


def find_private_vocab(given: Path | None) -> Path | None:
    candidates = [given, os.environ.get("SEOGI_PRIVATE_VOCAB"), Path.home() / "seogi-private"]
    for candidate in candidates:
        if not candidate:
            continue
        path = Path(candidate).expanduser()
        if path.is_file():
            return path
        if path.is_dir():
            found = sorted(path.rglob("vocabulary.json"))
            if found:
                return found[0]
    return None


def load_names(path: Path) -> tuple[set[str], set[str]]:
    """(every name form, the person names of three letters or more)."""
    data = json.loads(path.read_text(encoding="utf-8"))
    entries = data if isinstance(data, list) else next(
        (data[key] for key in ("entries", "terms", "vocabulary", "items") if key in data), [])
    names: set[str] = set()
    people: set[str] = set()
    ordinary: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        forms = [entry.get("term", "")] + list(entry.get("aliases") or [])
        forms = [form.strip() for form in forms if form and form.strip()]
        if entry.get("category") not in NAME_CATEGORIES:
            ordinary.update(forms)
            continue
        names.update(forms)
        if entry.get("category") in PERSON_CATEGORIES:
            people.update(form for form in forms if len(form) >= 3)
    # A word that is also an ordinary term there (a material that someone
    # happens to use as a nickname) is a word, not a name.
    return names - ordinary, people - ordinary


def collisions(script: Path, names: set[str], people: set[str]) -> list[str]:
    text = script.read_text(encoding="utf-8")
    declared: list[str] = []
    for line in text.splitlines():
        if line.startswith("# vocabulary:"):
            declared = [term.strip() for term in line.split(":", 1)[1].split(",") if term.strip()]
    spoken = "\n".join(line for line in text.splitlines() if not line.startswith("#"))
    found = {term for term in declared if term in names}
    found |= {name for name in people if name in spoken}
    return sorted(found)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--private-vocab", type=Path)
    parser.add_argument("scripts", nargs="*", type=Path)
    args = parser.parse_args()

    vocab = find_private_vocab(args.private_vocab)
    if vocab is None:
        print("no private vocabulary on this computer; nothing to compare the names against")
        return
    names, people = load_names(vocab)
    scripts = args.scripts or sorted(SCRIPTS.glob("*.txt"))
    bad = False
    for script in scripts:
        found = collisions(script, names, people)
        if found:
            bad = True
            print(f"{script.name}: these names are real ones in your private vocabulary: {', '.join(found)}")
    if bad:
        print("Rename them in the script (and in any test that uses them), then make the clips again.")
        sys.exit(1)
    print(f"{len(scripts)} script(s): no invented name is a real one")


if __name__ == "__main__":
    main()
