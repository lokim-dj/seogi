#!/usr/bin/env python3
"""Scores a transcript against a recording's known text.

    python3 eval/score.py --truth truth.json --hyp transcript.json [--vocab vocabulary.txt]

`truth.json` is what eval/synthetic/make_clips.py writes: turns with speaker,
start_ms, end_ms and text. `transcript.json` is what `seogi --seogi-transcribe
... --seogi-out DIR` writes.

Measures:
  cer             character error rate of the whole text (spaces, punctuation
                  and digit grouping ignored): edits / reference characters
  term_recall     of the vocabulary terms spoken, how many came out right
  speaker_acc     share of speech time given to the right speaker, under the
                  best one-to-one naming of the transcript's speakers
"""

from __future__ import annotations

import argparse
import itertools
import json
import re
import sys
import unicodedata
from pathlib import Path

UNIT_FORMS = [
    ("밀리미터", "밀리"), ("mm", "밀리"), ("㎜", "밀리"),
    ("센티미터", "센티"), ("cm", "센티"), ("㎝", "센티"),
    ("퍼센트", "%"), ("프로", "%"),
    ("제곱미터", "㎡"), ("m2", "㎡"),
]


def normalize(text: str) -> str:
    """The text as compared: one script form, no spaces or punctuation."""
    text = unicodedata.normalize("NFC", text).lower()
    for spoken, written in UNIT_FORMS:
        text = text.replace(spoken, written)
    # Keep letters and digits (and the two unit signs); drop the rest.
    return "".join(ch for ch in text if ch.isalnum() or ch in "%㎡")


def edit_distance(ref: str, hyp: str) -> int:
    if not ref:
        return len(hyp)
    previous = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        current = [i]
        for j, h in enumerate(hyp, 1):
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (r != h)))
        previous = current
    return previous[-1]


def cer(ref: str, hyp: str) -> float:
    ref_n, hyp_n = normalize(ref), normalize(hyp)
    return edit_distance(ref_n, hyp_n) / max(1, len(ref_n))


def term_counts(terms: list[str], ref: str, hyp: str):
    """Per term: times spoken, times found (capped at spoken)."""
    ref_n, hyp_n = normalize(ref), normalize(hyp)
    rows = []
    for term in terms:
        key = normalize(term)
        if not key:
            continue
        spoken = ref_n.count(key)
        if spoken:
            rows.append((term, spoken, min(spoken, hyp_n.count(key))))
    return rows


def speaker_accuracy(truth_turns, segments) -> tuple[float, int]:
    """Share of true speech time whose transcript line has the right speaker."""
    truth_speakers = sorted({t["speaker"] for t in truth_turns})
    hyp_speakers = sorted({s["speaker"] for s in segments})
    # Milliseconds of overlap between each true speaker and each transcript speaker.
    overlap = {(t, h): 0 for t in truth_speakers for h in hyp_speakers}
    for turn in truth_turns:
        for seg in segments:
            shared = min(turn["end_ms"], seg["end_ms"]) - max(turn["start_ms"], seg["start_ms"])
            if shared > 0:
                overlap[(turn["speaker"], seg["speaker"])] += shared
    total = sum(t["end_ms"] - t["start_ms"] for t in truth_turns)
    if not total or not hyp_speakers:
        return 0.0, len(hyp_speakers)
    # Best one-to-one naming. Speaker counts are small, so try them all.
    best = 0
    if len(hyp_speakers) <= len(truth_speakers):
        for chosen in itertools.permutations(truth_speakers, len(hyp_speakers)):
            best = max(best, sum(overlap[(t, h)] for t, h in zip(chosen, hyp_speakers)))
    else:
        for chosen in itertools.permutations(hyp_speakers, len(truth_speakers)):
            best = max(best, sum(overlap[(t, h)] for t, h in zip(truth_speakers, chosen)))
    return best / total, len([h for h in hyp_speakers if h != 0])


def score(truth_path: Path, hyp_path: Path, vocab_path: Path | None) -> dict:
    truth = json.loads(truth_path.read_text(encoding="utf-8"))
    hyp = json.loads(hyp_path.read_text(encoding="utf-8"))
    transcript = hyp.get("transcript", hyp)
    segments = transcript["segments"]
    ref_text = " ".join(t["text"] for t in truth["turns"])
    # The words as the minutes read them: a line's fixes (the vocabulary's
    # corrections, kept beside the words transcribed) when it has them.
    hyp_text = " ".join(s["edited"] if s.get("edited") is not None else s["text"] for s in segments)

    terms: list[str] = []
    if vocab_path and vocab_path.exists():
        for line in vocab_path.read_text(encoding="utf-8").splitlines():
            line = line.split("#")[0].strip()
            if line and "=>" not in line:
                terms.extend(t.strip() for t in re.split(r"[,，、]", line) if t.strip())
    rows = term_counts(terms, ref_text, hyp_text)
    spoken = sum(r[1] for r in rows)
    found = sum(r[2] for r in rows)
    accuracy, speakers = speaker_accuracy(truth["turns"], segments)
    stats = hyp.get("stats", {})
    return {
        "cer": round(cer(ref_text, hyp_text), 4),
        "ref_chars": len(normalize(ref_text)),
        "term_recall": round(found / spoken, 4) if spoken else None,
        "terms_spoken": spoken,
        "terms_found": found,
        "terms_missed": [r[0] for r in rows if r[2] < r[1]],
        "speaker_acc": round(accuracy, 4),
        "speakers_found": speakers,
        "speakers_true": len({t["speaker"] for t in truth["turns"]}),
        "lines": len(segments),
        "unsure_lines": sum(1 for s in segments if s.get("unsure")),
        "failed_chunks": stats.get("failed_chunks"),
        "elapsed_secs": round(hyp.get("elapsed_secs", 0.0), 1),
        "audio_secs": round(stats.get("audio_ms", 0) / 1000, 1),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--truth", type=Path, required=True)
    parser.add_argument("--hyp", type=Path, required=True)
    parser.add_argument("--vocab", type=Path)
    args = parser.parse_args()
    json.dump(score(args.truth, args.hyp, args.vocab), sys.stdout, ensure_ascii=False, indent=2)
    print()


if __name__ == "__main__":
    main()
