#!/usr/bin/env python3
"""Compares Seogi with other tools on your own recordings.

This is a developer's tool, not part of the app. It works on a folder of
samples that lives outside this repository (real recordings must never be
copied into it); only numbers come back, into eval/results/, which is ignored.

    python3 eval/compare.py --samples "/path/to/samples"
    python3 eval/compare.py --samples DIR --model large-v3 --llm claude:sonnet
    python3 eval/compare.py --samples DIR --skip-seogi      # only score what is there

Each sample is a folder:

    <sample>/
      <recording>.m4a|mp3|wav|...   the recording (the first audio file found)
      reference.txt                 the transcript as a person corrected it
      actions.txt                   the tasks a person would list, one per line (optional)
      vocabulary.txt                names and terms that matter here, one per line (optional)
      tools/<name>.txt              another tool's transcript: clova.txt, plaud.txt, ...
      tools/<name>.actions.txt      that tool's tasks, one per line (optional)

For every tool, and for Seogi, it reports:
    CER        character error rate against reference.txt
    terms      vocabulary terms spoken that came out right
    tasks      tasks in actions.txt that the tool also found / tasks it listed
    time       how long Seogi took (other tools' times are not known)

Speaker labels and timestamps in the other tools' transcripts are ignored:
lines are compared as text.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import AUDIO_SUFFIXES, MODELS, find_binary  # noqa: E402
from score import cer, normalize, term_counts  # noqa: E402

# What other tools put in front of a line: a time, a speaker, or both.
LINE_PREFIX = re.compile(
    r"^\s*(?:\[?\(?\d{1,2}:\d{2}(?::\d{2})?\)?\]?\s*)?(?:(?:화자|참석자|speaker)\s*\d+|[^\s:]{1,12})?\s*[:：]\s*",
    re.IGNORECASE,
)
TIME_ONLY = re.compile(r"^\s*\[?\(?\d{1,2}:\d{2}(?::\d{2})?\)?\]?\s*")


def spoken_text(text: str) -> str:
    """A transcript without its timestamps and speaker labels."""
    lines = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        stripped = LINE_PREFIX.sub("", line, count=1)
        if stripped == line:
            stripped = TIME_ONLY.sub("", line, count=1)
        lines.append(stripped)
    return "\n".join(lines)


def read_lines(path: Path) -> list[str]:
    if not path.exists():
        return []
    return [line.strip(" -•*\t") for line in path.read_text(encoding="utf-8").splitlines() if line.strip(" -•*\t")]


def likeness(a: str, b: str) -> float:
    """How alike two short texts are: shared two-letter pieces (Dice)."""
    def pieces(text: str) -> set[str]:
        text = normalize(text)
        return {text[i:i + 2] for i in range(len(text) - 1)}
    x, y = pieces(a), pieces(b)
    if not x or not y:
        return 0.0
    return 2 * len(x & y) / (len(x) + len(y))


def tasks_found(wanted: list[str], listed: list[str], threshold: float = 0.45) -> int:
    """How many of the wanted tasks are among the listed ones, each listed
    task counting for one wanted task at most."""
    free = list(listed)
    found = 0
    for task in wanted:
        best = max(free, key=lambda other: likeness(task, other), default=None)
        if best is not None and likeness(task, best) >= threshold:
            free.remove(best)
            found += 1
    return found


def seogi_tasks(minutes_json: Path) -> list[str]:
    if not minutes_json.exists():
        return []
    minutes = json.loads(minutes_json.read_text(encoding="utf-8")).get("minutes", {})
    tasks = []
    for section in minutes.get("sections", []):
        if section.get("kind") == "actions":
            tasks += [entry.get("text", "") for entry in section.get("entries", [])]
    return [task for task in tasks if task]


def run_seogi(binary: Path, audio: Path, out: Path, model: str, vocab: Path | None, llm: str | None):
    """Runs the pipeline on one recording. Returns (seconds, error)."""
    command = [str(binary), "--seogi-transcribe", str(audio), "--model", MODELS.get(model, model),
               "--seogi-out", str(out)]
    if vocab:
        command += ["--seogi-vocab", str(vocab)]
    if llm:
        command += ["--seogi-minutes", "--seogi-llm", llm]
    started = time.time()
    done = subprocess.run(command, capture_output=True, text=True)
    if done.returncode != 0:
        return time.time() - started, (done.stderr.strip().splitlines() or ["failed"])[-1][:200]
    return time.time() - started, None


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--samples", type=Path, required=True)
    parser.add_argument("--model", default="turbo", help=f"one of {', '.join(MODELS)} or a model id")
    parser.add_argument("--vocab", type=Path, help="a vocabulary for every sample (your own; only read)")
    parser.add_argument("--llm", help="also write minutes with this model: ollama:<model>, claude[:<model>], codex")
    parser.add_argument("--skip-seogi", action="store_true", help="do not run Seogi, score the other tools only")
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()

    samples = sorted(p for p in args.samples.iterdir() if p.is_dir() and not p.name.startswith("."))
    if not samples:
        raise SystemExit(f"no sample folders in {args.samples}")
    binary = None if args.skip_seogi else (args.binary or find_binary())
    stamp = time.strftime("%Y%m%d-%H%M%S")
    work = HERE / "results" / f"compare-{stamp}"

    rows = []
    for number, sample in enumerate(samples, 1):
        # Samples are named by number in what is printed and saved, never by
        # their folder name: that is often a customer's name.
        label = f"sample {number}"
        audio = next((f for f in sorted(sample.iterdir()) if f.suffix.lower() in AUDIO_SUFFIXES), None)
        reference_path = sample / "reference.txt"
        reference = spoken_text(reference_path.read_text(encoding="utf-8")) if reference_path.exists() else ""
        wanted = read_lines(sample / "actions.txt")
        own_vocab = sample / "vocabulary.txt"
        terms = read_lines(own_vocab)

        candidates: list[tuple[str, str, list[str], float | None]] = []
        tools = sample / "tools"
        if tools.is_dir():
            for transcript in sorted(tools.glob("*.txt")):
                if transcript.name.endswith(".actions.txt"):
                    continue
                listed = read_lines(tools / f"{transcript.stem}.actions.txt")
                candidates.append((transcript.stem, spoken_text(transcript.read_text(encoding="utf-8")), listed, None))

        if binary and audio:
            out = work / f"sample-{number}"
            vocab = args.vocab or (own_vocab if own_vocab.exists() else None)
            seconds, error = run_seogi(binary, audio, out, args.model, vocab, args.llm)
            if error:
                print(f"{label}: Seogi failed: {error}", file=sys.stderr)
            else:
                text = (out / "transcript.txt").read_text(encoding="utf-8")
                candidates.append((f"seogi ({args.model})", spoken_text(text), seogi_tasks(out / "minutes.json"), seconds))
        elif binary:
            print(f"{label}: no recording found", file=sys.stderr)

        for name, text, listed, seconds in candidates:
            row: dict = {"sample": label, "tool": name}
            if reference:
                row["cer"] = round(cer(reference, text), 4)
                spoken = term_counts(terms, reference, text)
                row["terms_spoken"] = sum(count for _, count, _ in spoken)
                row["terms_right"] = sum(right for _, _, right in spoken)
            if wanted:
                row["tasks_wanted"] = len(wanted)
                row["tasks_found"] = tasks_found(wanted, listed)
                row["tasks_listed"] = len(listed)
            if seconds is not None:
                row["seconds"] = round(seconds, 1)
            rows.append(row)

    print("| sample | tool | CER | terms | tasks found / wanted (listed) | time |")
    print("| --- | --- | --- | --- | --- | --- |")
    for row in rows:
        cer_text = f"{row['cer'] * 100:.2f}%" if "cer" in row else "-"
        terms_text = f"{row['terms_right']}/{row['terms_spoken']}" if row.get("terms_spoken") else "-"
        tasks_text = (f"{row['tasks_found']}/{row['tasks_wanted']} ({row['tasks_listed']})"
                      if "tasks_wanted" in row else "-")
        time_text = f"{row['seconds']:.0f}s" if "seconds" in row else "-"
        print(f"| {row['sample']} | {row['tool']} | {cer_text} | {terms_text} | {tasks_text} | {time_text} |")

    # The mean of each tool over the samples it has a transcript for.
    print("\n| tool | samples | mean CER | terms | tasks found / wanted |")
    print("| --- | --- | --- | --- | --- |")
    for tool in dict.fromkeys(row["tool"] for row in rows):
        own = [row for row in rows if row["tool"] == tool]
        scored = [row["cer"] for row in own if "cer" in row]
        mean = f"{sum(scored) / len(scored) * 100:.2f}%" if scored else "-"
        right = sum(row.get("terms_right", 0) for row in own)
        spoken = sum(row.get("terms_spoken", 0) for row in own)
        found = sum(row.get("tasks_found", 0) for row in own)
        wanted_total = sum(row.get("tasks_wanted", 0) for row in own)
        print(f"| {tool} | {len(own)} | {mean} | {right}/{spoken} | {found}/{wanted_total} |")

    work.parent.mkdir(parents=True, exist_ok=True)
    saved = work.parent / f"compare-{stamp}.json"
    saved.write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nsaved {saved.relative_to(HERE.parent)}")


if __name__ == "__main__":
    main()
