#!/usr/bin/env python3
"""Runs Seogi's pipeline over practice recordings and prints the scores.

    python3 eval/run.py                              # every synthetic clip, default model
    python3 eval/run.py --models turbo large-v3      # compare models
    python3 eval/run.py --clips noisy.wav phone.m4a  # only these files of each set
    python3 eval/run.py --no-vocab                   # without the vocabulary hints

Each practice set is a folder with `truth.json` and audio files (see
eval/synthetic/make_clips.py). By default the sets under eval/synthetic/out
are used; `--sets DIR` points at another place, such as a private folder of
real recordings with corrected transcripts. Nothing from such a folder is
copied into this repository: results go to eval/results/, which is ignored.

Run this before and after any change to transcription, chunking, diarization
or the vocabulary handling, and put the two tables in the commit message.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))
from score import score  # noqa: E402

MODELS = {
    "turbo": "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
    "large-v3": "handy-computer/whisper-large-v3-gguf/whisper-large-v3-Q8_0.gguf",
    "qwen-1.7b": "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf",
    "qwen-0.6b": "handy-computer/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf",
    "funasr": "handy-computer/Fun-ASR-MLT-Nano-2512-gguf/Fun-ASR-MLT-Nano-2512-Q8_0.gguf",
    "sensevoice": "handy-computer/SenseVoiceSmall-gguf/SenseVoiceSmall-Q8_0.gguf",
    "cohere": "handy-computer/cohere-transcribe-03-2026-gguf/cohere-transcribe-03-2026-Q8_0.gguf",
}
AUDIO_SUFFIXES = {".wav", ".m4a", ".mp3", ".amr", ".3gp", ".aac", ".flac", ".ogg", ".mp4"}


def find_binary() -> Path:
    name = "seogi.exe" if sys.platform == "win32" else "seogi"
    for profile in ("release", "debug"):
        candidate = REPO / "src-tauri" / "target" / profile / name
        if candidate.exists():
            return candidate
    raise SystemExit("Build the app first: cd src-tauri && cargo build")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--sets", type=Path, default=HERE / "synthetic" / "out")
    parser.add_argument("--models", nargs="+", default=["qwen-1.7b"],
                        help=f"aliases ({', '.join(MODELS)}) or full model ids")
    parser.add_argument("--clips", nargs="+", help="file names to run within each set (default: all audio)")
    parser.add_argument("--no-vocab", action="store_true", help="run without each set's vocabulary.txt")
    parser.add_argument("--vocab", type=Path,
                        help="a vocabulary file or folder to use for every set instead of its own "
                             "(for example your private one; it is only read, never copied)")
    parser.add_argument("--no-diarize", action="store_true")
    parser.add_argument("--label", default="", help="a name for this run in the results file")
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()

    binary = args.binary or find_binary()
    sets = sorted(p for p in args.sets.iterdir() if (p / "truth.json").exists())
    if not sets:
        raise SystemExit(f"No practice sets under {args.sets} (run eval/synthetic/make_clips.py)")

    rows = []
    for practice in sets:
        clips = sorted(f for f in practice.iterdir() if f.suffix.lower() in AUDIO_SUFFIXES)
        if args.clips:
            clips = [c for c in clips if c.name in args.clips]
        own_vocab = practice / "vocabulary.txt"
        vocab = args.vocab or own_vocab
        for model in args.models:
            model_id = MODELS.get(model, model)
            for clip in clips:
                out = HERE / "results" / "runs" / f"{practice.name}-{clip.name}-{model}".replace("/", "_")
                command = [str(binary), "--seogi-transcribe", str(clip), "--model", model_id,
                           "--seogi-out", str(out)]
                use_vocab = vocab.exists() and not args.no_vocab
                if use_vocab:
                    command += ["--seogi-vocab", str(vocab)]
                if args.no_diarize:
                    command.append("--seogi-no-diarize")
                started = time.time()
                done = subprocess.run(command, capture_output=True, text=True)
                if done.returncode != 0:
                    reason = (done.stderr.strip().splitlines() or ["failed"])[-1]
                    print(f"FAILED {practice.name}/{clip.name} [{model}]: {reason[:200]}", file=sys.stderr)
                    rows.append({"set": practice.name, "clip": clip.name, "model": model, "error": reason[:200]})
                    continue
                result = score(practice / "truth.json", out / "transcript.json",
                               own_vocab if own_vocab.exists() else None)
                result.update({"set": practice.name, "clip": clip.name, "model": model,
                               "vocab": use_vocab, "wall_secs": round(time.time() - started, 1)})
                rows.append(result)

    print_table(rows)
    results_dir = HERE / "results"
    results_dir.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    name = f"{stamp}{'-' + args.label if args.label else ''}.json"
    (results_dir / name).write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nsaved eval/results/{name}")


def print_table(rows: list[dict]) -> None:
    print("| set | clip | model | vocab | CER | terms | speakers | speaker acc | unsure | speed |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for r in rows:
        if "error" in r:
            print(f"| {r['set']} | {r['clip']} | {r['model']} | | failed: {r['error']} | | | | | |")
            continue
        terms = f"{r['terms_found']}/{r['terms_spoken']}" if r["terms_spoken"] else "-"
        speed = f"{r['audio_secs'] / r['elapsed_secs']:.1f}x" if r["elapsed_secs"] else "-"
        print(
            f"| {r['set']} | {r['clip']} | {r['model']} | {'on' if r['vocab'] else 'off'} "
            f"| {r['cer'] * 100:.2f}% | {terms} | {r['speakers_found']}/{r['speakers_true']} "
            f"| {r['speaker_acc'] * 100:.1f}% | {r['unsure_lines']}/{r['lines']} | {speed} |"
        )


if __name__ == "__main__":
    main()
