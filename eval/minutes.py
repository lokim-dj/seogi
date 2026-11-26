#!/usr/bin/env python3
"""Measures the minutes on made-up scenes whose right answers are known.

    python3 eval/minutes.py --llm claude:sonnet                 # every scene
    python3 eval/minutes.py --llm claude:sonnet --scenes numbers reopen
    python3 eval/minutes.py --binary path/to/seogi --label before

Transcription is not part of this: each scene's script is turned into a
transcript as if the speech model had heard every word (a scene can carry
the kind of mistakes a speech model makes in its lines), and only the
minutes are written, with `seogi --seogi-minutes-from meeting.json`. So a
change to the minutes prompts or checks is measured on its own.

A scene is a text file under eval/synthetic/minutes/: lines "화자: 문장"
(or `# script: ../scripts/x.txt` to use a practice script's lines), and
what the minutes should do, in comment lines:

    # template: consult            the template to write with
    # people: 김서윤 실장, 박도현 소장  the people known beforehand
    # customer: 이서진               the customer; # place: the site
    # late: 8                      the calendar event began 8 minutes
                                   before the recording
    # midstart: yes                the recording starts in the middle of talk
    # task: 할 일 | 담당            a task the minutes should list, and whose
    # expect: 갈래,… | 정규식       some entry of these sections should say this
    # trap: 갈래,… | 정규식 [| unless: 정규식]
                                   no entry of these sections may state this
                                   without being marked for a look ("확인 필요");
                                   `story` and `summary` are the story and the
                                   one-line summary, `*` every section
    # trap-num: …                  the same, for a wrong number
    # trap-quote: 정규식            no entry may rest on a line saying this

For every scene it reports:
    tasks      expected tasks found / expected (tasks listed)
    owners     of the tasks found, how many name the right person
    traps      traps leaked (stated as fact) / traps — lower is better
    numbers    wrong-number traps leaked / wrong-number traps
    caught     traps that were marked for a look rather than left out
    expect     entries that should be there and are
    checks     entries marked for a look (확인 필요) in all
    header     the customer's name in the head of 회의록.md, and the note
               that the recording started late or in the middle, when due

The names in the scenes are made up (eval/synthetic/check_names.py checks
them against a private list). Results go to eval/results/, which is ignored.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from compare import likeness  # noqa: E402
from run import find_binary  # noqa: E402

SCENES = HERE / "synthetic" / "minutes"
# A made-up start: 2026-11-02 14:00 KST.
STARTED_AT_MS = 1793595600000


def parse_scene(path: Path) -> dict:
    scene: dict = {"name": path.stem, "turns": [], "tasks": [], "expect": [], "traps": [],
                   "people": [], "customer": "", "place": "", "template": "", "late": 0,
                   "midstart": False}
    lines = path.read_text(encoding="utf-8").splitlines()
    script: list[str] = []
    for raw in lines:
        line = raw.strip()
        if not line.startswith("#"):
            if line:
                script.append(line)
            continue
        body = line.lstrip("#").strip()
        key, _, value = body.partition(":")
        key, value = key.strip(), value.strip()
        if key == "script":
            script = [x.strip() for x in (path.parent / value).read_text(encoding="utf-8").splitlines()
                      if x.strip() and not x.strip().startswith("#")]
        elif key == "people":
            scene["people"] = [p.strip() for p in value.split(",") if p.strip()]
        elif key in ("customer", "place", "template"):
            scene[key] = value
        elif key == "late":
            scene["late"] = int(value or 0)
        elif key == "midstart":
            scene["midstart"] = value.lower() in ("yes", "true", "1")
        elif key == "task":
            text, _, owner = value.partition(" | ")
            scene["tasks"].append({"text": text.strip(), "owner": owner.strip()})
        elif key == "expect":
            sections, _, pattern = value.partition(" | ")
            scene["expect"].append({"sections": split_sections(sections), "pattern": pattern.strip()})
        elif key in ("trap", "trap-num"):
            # Fields are parted by " | "; a pattern's own "|" has no spaces.
            parts = [p.strip() for p in re.split(r"\s\|\s", value)]
            unless = ""
            if len(parts) > 2 and parts[-1].startswith("unless:"):
                unless = parts.pop().removeprefix("unless:").strip()
            scene["traps"].append({"kind": "num" if key == "trap-num" else "fact",
                                   "sections": split_sections(parts[0]),
                                   "pattern": parts[1], "unless": unless})
        elif key == "trap-quote":
            scene["traps"].append({"kind": "quote", "sections": ["*"], "pattern": value, "unless": ""})
    for line in script:
        speaker, _, text = line.partition(":")
        scene["turns"].append((speaker.strip(), text.strip()))
    return scene


def split_sections(text: str) -> list[str]:
    return [s.strip() for s in text.split(",") if s.strip()]


def meeting_json(scene: dict) -> dict:
    """The scene as a meeting.json: a transcript as if every word was heard."""
    ids: dict[str, int] = {}
    segments = []
    at = 1_000 if not scene["midstart"] else 0
    for speaker, text in scene["turns"]:
        # "실장~2" is the same person heard as another speaker.
        ids.setdefault(speaker, len(ids) + 1)
        length = max(2_000, len(text) * 180)
        segments.append({"start_ms": at, "end_ms": at + length, "speaker": ids[speaker], "text": text,
                         "confidence": 0.9, "unsure": False, "overlap": False})
        at += length + 600
    transcript = {"language": "ko", "model": "script", "segments": segments,
                  "speakers": [{"id": i, "name": f"화자 {i}"} for i in ids.values()], "corrections": []}
    meeting = {
        "id": f"eval-{scene['name']}", "title": "", "title_is_auto": True,
        "started_at_ms": STARTED_AT_MS, "time_source": "recorder", "duration_ms": at,
        "origin": "imported", "audio_file": "scene.wav", "status": "transcribed",
        "template_id": scene["template"], "people": scene["people"], "customer": scene["customer"],
        "place": scene["place"], "transcript": transcript,
    }
    if scene["late"]:
        start = STARTED_AT_MS - scene["late"] * 60_000
        meeting["event"] = {"uid": "eval", "title": "상담", "start_ms": start, "end_ms": start + 3_600_000}
    return meeting


def entries_of(minutes: dict) -> list[tuple[str, dict]]:
    found = []
    for section in minutes.get("sections", []):
        for entry in section.get("entries", []):
            found.append((section.get("key", ""), entry))
    return found


def flagged(entry: dict) -> bool:
    return bool(entry.get("reasons"))


def score(scene: dict, out: Path) -> dict:
    data = json.loads((out / "minutes.json").read_text(encoding="utf-8"))
    minutes = data["minutes"]
    markdown = (out / "minutes.md").read_text(encoding="utf-8")
    entries = entries_of(minutes)
    tasks = [(key, e) for key, e in entries if key.endswith("tasks") or key == "actions"]
    # Every task the minutes list counts for one expected task at most.
    free = list(tasks)
    found = owners_right = owners_wanted = 0
    for wanted in scene["tasks"]:
        best = max(free, key=lambda t: likeness(wanted["text"], t[1]["text"]), default=None)
        if best is None or likeness(wanted["text"], best[1]["text"]) < 0.3:
            continue
        free.remove(best)
        found += 1
        if wanted["owner"]:
            owners_wanted += 1
            first = wanted["owner"].split()[0]
            if first in best[1].get("owner", ""):
                owners_right += 1

    stated = [(key, e["text"], flagged(e), " ".join(s["quote"] for s in e.get("sources", [])))
              for key, e in entries]
    stated += [("story", part["text"], False, "") for part in minutes.get("story", [])]
    stated += [("summary", minutes.get("summary", ""), False, "")]

    def in_sections(key: str, sections: list[str]) -> bool:
        if "*" in sections:
            return key not in ("story", "summary") or key in sections
        return key in sections

    leaked = {"fact": 0, "num": 0, "quote": 0}
    total = {"fact": 0, "num": 0, "quote": 0}
    caught = 0
    leaks: list[str] = []
    for trap in scene["traps"]:
        total[trap["kind"]] += 1
        pattern = re.compile(trap["pattern"])
        unless = re.compile(trap["unless"]) if trap["unless"] else None
        hit_open = hit_flagged = False
        for key, text, is_flagged, quote in stated:
            if not in_sections(key, trap["sections"]):
                continue
            target = quote if trap["kind"] == "quote" else text
            if not pattern.search(target) or (unless and unless.search(text)):
                continue
            if is_flagged:
                hit_flagged = True
            else:
                hit_open = True
                leaks.append(f"{key}: {text}")
        if hit_open:
            leaked[trap["kind"]] += 1
        elif hit_flagged:
            caught += 1

    expected = sum(
        1 for want in scene["expect"]
        if any(in_sections(key, want["sections"]) and re.search(want["pattern"], text)
               for key, text, _, _ in stated))
    head = "\n".join(markdown.splitlines()[:12])
    header_wanted = header_right = 0
    if scene["customer"]:
        header_wanted += 1
        header_right += scene["customer"] in head
    if scene["late"] or scene["midstart"]:
        header_wanted += 1
        header_right += ("늦게 시작" in head) or ("중간부터" in head)
    return {
        "scene": scene["name"],
        "tasks_found": found, "tasks_wanted": len(scene["tasks"]), "tasks_listed": len(tasks),
        "owners_right": owners_right, "owners_wanted": owners_wanted,
        "traps_leaked": leaked["fact"] + leaked["quote"], "traps": total["fact"] + total["quote"],
        "numbers_leaked": leaked["num"], "numbers": total["num"],
        "caught": caught,
        "expect_found": expected, "expect": len(scene["expect"]),
        "checks": sum(1 for _, e in entries if flagged(e)),
        "entries": len(entries),
        "header_right": header_right, "header_wanted": header_wanted,
        "leaks": leaks,
    }


def run_scene(binary: Path, scene: dict, llm: str, work: Path, data_dir: Path) -> dict:
    folder = work / scene["name"]
    folder.mkdir(parents=True, exist_ok=True)
    source = folder / "meeting.json"
    source.write_text(json.dumps(meeting_json(scene), ensure_ascii=False, indent=2), encoding="utf-8")
    command = [str(binary), "--seogi-minutes-from", str(source), "--seogi-out", str(folder),
               "--seogi-llm", llm]
    started = time.time()
    env = {**__import__("os").environ, "SEOGI_DATA_DIR": str(data_dir)}
    done = subprocess.run(command, capture_output=True, text=True, env=env)
    if done.returncode != 0:
        reason = (done.stderr.strip().splitlines() or ["failed"])[-1][:200]
        return {"scene": scene["name"], "error": reason}
    result = score(scene, folder)
    result["seconds"] = round(time.time() - started)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--llm", default="claude:sonnet",
                        help="ollama:<model>, claude[:<model>] or codex[:<model>]")
    parser.add_argument("--scenes", nargs="+", help="scene names (default: all)")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--label", default="")
    parser.add_argument("--jobs", type=int, default=2,
                        help="scenes written at once (each is an app process; keep it small)")
    parser.add_argument("--repeat", type=int, default=1, help="write each scene this many times")
    parser.add_argument("--rescore", type=Path,
                        help="score the minutes of an earlier run (its folder) again, without writing")
    args = parser.parse_args()

    if args.rescore:
        rows = []
        for run in sorted(args.rescore.glob("run*")):
            for path in sorted(SCENES.glob("*.txt")):
                if (run / path.stem / "minutes.json").exists():
                    rows.append(score(parse_scene(path), run / path.stem))
        print_table(rows)
        return

    binary = args.binary or find_binary()
    paths = sorted(SCENES.glob("*.txt"))
    if args.scenes:
        paths = [p for p in paths if p.stem in args.scenes]
    scenes = [parse_scene(p) for p in paths]
    stamp = time.strftime("%Y%m%d-%H%M%S")
    work = HERE / "results" / f"minutes-{stamp}{'-' + args.label if args.label else ''}"
    data_dir = work / "data"
    data_dir.mkdir(parents=True, exist_ok=True)

    jobs = [(scene, n) for n in range(args.repeat) for scene in scenes]
    with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        rows = list(pool.map(
            lambda job: run_scene(binary, job[0], args.llm, work / f"run{job[1] + 1}", data_dir), jobs))

    print_table(rows)
    (work / "results.json").write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nsaved {work.relative_to(HERE.parent)}/results.json")


def print_table(rows: list[dict]) -> None:
    print("| scene | tasks | owners | traps leaked | numbers leaked | caught | expect | checks | header |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    sums: dict[str, int] = {}
    for r in rows:
        if "error" in r:
            print(f"| {r['scene']} | failed: {r['error']} | | | | | | | |")
            continue
        for key, value in r.items():
            if isinstance(value, int) and key != "seconds":
                sums[key] = sums.get(key, 0) + value
        print(row_text(r["scene"], r))
    if sums:
        print(row_text("**all**", sums))


def row_text(name: str, r: dict) -> str:
    return (f"| {name} | {r['tasks_found']}/{r['tasks_wanted']} ({r['tasks_listed']}) "
            f"| {r['owners_right']}/{r['owners_wanted']} | {r['traps_leaked']}/{r['traps']} "
            f"| {r['numbers_leaked']}/{r['numbers']} | {r['caught']} | {r['expect_found']}/{r['expect']} "
            f"| {r['checks']}/{r['entries']} | {r['header_right']}/{r['header_wanted']} |")


if __name__ == "__main__":
    main()
