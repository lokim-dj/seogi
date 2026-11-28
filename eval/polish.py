#!/usr/bin/env python3
"""Measures the transcript polish (issue #108) on the practice scripts.

    python3 eval/polish.py --llm claude:sonnet
    python3 eval/polish.py --llm ollama:gemma3:12b --binary path/to/seogi

The polish may tidy spacing, punctuation and fillers, and must never change
a number, a name or a negation. Each practice script (eval/synthetic/scripts
and the scenes under eval/synthetic/minutes) is made into a transcript the
way a speech model writes one: no punctuation, a filler ("음", "어", "그")
at the start of some lines and a word said twice in others, chosen the same
way every run. `seogi --seogi-polish-from` polishes it and writes the
model's answers and the lines the checks kept; this script counts, on its
own, what changed:

    numbers   digit runs with the letter after them, and numbers said as
              words ("하나", "다섯", "열")
    names     the script's vocabulary line, its speakers and people
    negations "안", "못", "않", "없", "아니"

for the model's answers (before the checks) and for the lines shown (after
them). The lines shown must change none of these: the last column is the
one that counts.

The names in the scripts are made up (eval/synthetic/check_names.py).
"""

from __future__ import annotations

import argparse
import json
import os
import random
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import find_binary  # noqa: E402

NUMBER_WORDS = ["하나", "둘", "셋", "넷", "다섯", "여섯", "일곱", "여덟", "아홉", "열", "스물", "서른"]
NEGATIONS = ["안", "못", "않", "없", "아니"]
FILLERS = ["음", "어", "그", "이제", "아"]


def script_lines(path: Path) -> tuple[list[tuple[str, str]], list[str]]:
    """The lines ("화자: 문장") of a script or scene, and the names it names."""
    lines: list[tuple[str, str]] = []
    names: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("#"):
            key, _, value = line.lstrip("#").strip().partition(":")
            key = key.strip()
            if key in ("vocabulary", "people", "customer"):
                names += [v.strip().split(" ")[0] for v in value.split(",") if v.strip()]
            elif key == "script":
                more, more_names = script_lines(path.parent / value.strip())
                lines += more
                names += more_names
            continue
        speaker, sep, text = line.partition(":")
        if sep and text.strip():
            lines.append((speaker.strip(), text.strip()))
    return lines, names


def as_heard(text: str, rng: random.Random) -> str:
    """The words as a speech model writes them: no punctuation, a filler or
    a word said twice now and then."""
    words = re.sub(r"[.,?!]", "", text).split()
    roll = rng.random()
    if roll < 0.35:
        words.insert(0, rng.choice(FILLERS))
    elif roll < 0.5 and len(words) > 3:
        at = rng.randrange(1, len(words) - 1)
        words.insert(at, rng.choice(FILLERS))
    elif roll < 0.6 and len(words) > 2:
        at = rng.randrange(0, len(words) - 1)
        words.insert(at, words[at])
    return " ".join(words)


def meeting_json(lines: list[tuple[str, str]], names: list[str], seed: str) -> dict:
    rng = random.Random(seed)
    speakers: dict[str, int] = {}
    segments = []
    at = 0
    for speaker, text in lines:
        number = speakers.setdefault(speaker, len(speakers) + 1)
        heard = as_heard(text, rng)
        length = 400 + 120 * len(heard)
        segments.append({"start_ms": at, "end_ms": at + length, "speaker": number, "text": heard})
        at += length + 300
    return {
        "transcript": {
            "language": "ko", "model": "script", "segments": segments,
            "speakers": [{"id": n, "name": s} for s, n in speakers.items()],
        },
        "terms": names,
    }


def squeezed(text: str) -> str:
    return re.sub(r"\s", "", text)


def numbers(text: str) -> list[str]:
    bare = re.sub(r"(?<=\d),(?=\d)", "", squeezed(text))
    found = re.findall(r"\d+(?:\.\d+)?[^\d\s.,?!]?", bare)
    found += [w for w in NUMBER_WORDS for _ in range(bare.count(w))]
    return sorted(found)


def counts(text: str, words: list[str]) -> list[int]:
    bare = squeezed(text)
    return [bare.count(w) for w in words]


def changes(said: str, shown: str, names: list[str]) -> tuple[int, int, int]:
    return (
        int(numbers(said) != numbers(shown)),
        int(counts(said, names) != counts(shown, names)),
        int(counts(said, NEGATIONS) != counts(shown, NEGATIONS)),
    )


def run_one(binary: Path, path: Path, llm: str, work: Path) -> dict:
    lines, names = script_lines(path)
    folder = work / f"{path.parent.name}-{path.stem}"
    folder.mkdir(parents=True, exist_ok=True)
    record = meeting_json(lines, names, path.stem)
    source = folder / "meeting.json"
    source.write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    # A data folder of its own: the user's settings and keys are not read.
    env = {**os.environ, "SEOGI_DATA_DIR": str(work / "data")}
    done = subprocess.run([str(binary), "--seogi-polish-from", str(source), "--seogi-out", str(folder),
                           "--seogi-llm", llm], capture_output=True, text=True, env=env)
    (folder / "stderr.log").write_text(done.stderr, encoding="utf-8")
    if done.returncode != 0:
        return {"name": folder.name, "error": (done.stderr.strip().splitlines() or ["failed"])[-1][:200]}
    out = json.loads((folder / "polish.json").read_text(encoding="utf-8"))
    names = sorted(set(names) | {s["name"] for s in record["transcript"]["speakers"]})
    before = [0, 0, 0]
    after = [0, 0, 0]
    for line in out["lines"]:
        if line["answer"] is not None:
            for i, n in enumerate(changes(line["text"], line["answer"], names)):
                before[i] += n
        for i, n in enumerate(changes(line["text"], line["shown"], names)):
            after[i] += n
    return {
        "name": folder.name, "lines": len(out["lines"]), "polished": out["polished"],
        "kept_as_said": out["kept_as_said"], "note": out["note"], "before": before, "after": after,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--llm", default="claude:sonnet")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--work", type=Path, default=HERE / "results" / "polish")
    args = parser.parse_args()
    binary = args.binary or find_binary()
    paths = sorted((HERE / "synthetic" / "scripts").glob("*.txt"))
    paths += sorted(p for p in (HERE / "synthetic" / "minutes").glob("*.txt")
                    if "# script:" not in p.read_text(encoding="utf-8"))
    results = [run_one(binary, p, args.llm, args.work) for p in paths]
    print(f"polish with {args.llm}: changed lines (numbers / names / negations)")
    print(f"{'script':<28}{'lines':>6}{'polished':>9}{'as said':>8}  {'model answer':>14}  {'shown':>10}")
    total = {"lines": 0, "polished": 0, "kept_as_said": 0, "before": [0, 0, 0], "after": [0, 0, 0]}
    for r in results:
        if "error" in r:
            print(f"{r['name']:<28} error: {r['error']}")
            continue
        note = f"  ({r['note']})" if r["note"] else ""
        print(f"{r['name']:<28}{r['lines']:>6}{r['polished']:>9}{r['kept_as_said']:>8}  "
              f"{'/'.join(map(str, r['before'])):>14}  {'/'.join(map(str, r['after'])):>10}{note}")
        for key in ("lines", "polished", "kept_as_said"):
            total[key] += r[key]
        for key in ("before", "after"):
            total[key] = [a + b for a, b in zip(total[key], r[key])]
    print(f"{'total':<28}{total['lines']:>6}{total['polished']:>9}{total['kept_as_said']:>8}  "
          f"{'/'.join(map(str, total['before'])):>14}  {'/'.join(map(str, total['after'])):>10}")
    if any(total["after"]):
        raise SystemExit("a shown line changed a number, a name or a negation")


if __name__ == "__main__":
    main()
