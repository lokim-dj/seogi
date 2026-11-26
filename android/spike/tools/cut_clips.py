#!/usr/bin/env python3
"""Cuts eval/synthetic/out/dictation_phone/{clean,noisy}.wav into one WAV per line.

Run make_clips.py first:
    python3 eval/synthetic/make_clips.py dictation_phone
    python3 android/spike/tools/cut_clips.py

Writes eval/synthetic/out/dictation_phone/lines/ (gitignored):
    clean_01.wav … noisy_16.wav   16 kHz mono PCM, 0.25 s of silence each side
    clips.json                    [{"file", "text", "seconds"}]
The folder is pushed to the phone with adb (see android/spike/README.md).
"""
import json
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SRC = ROOT / "eval/synthetic/out/dictation_phone"
OUT = SRC / "lines"
RATE = 16_000
PAD = RATE // 4


def main() -> None:
    truth = json.loads((SRC / "truth.json").read_text(encoding="utf-8"))
    OUT.mkdir(exist_ok=True)
    manifest = []
    for take in ("clean", "noisy"):
        with wave.open(str(SRC / f"{take}.wav"), "rb") as w:
            assert w.getframerate() == RATE and w.getnchannels() == 1 and w.getsampwidth() == 2
            pcm = w.readframes(w.getnframes())
        for i, turn in enumerate(truth["turns"], 1):
            a = turn["start_ms"] * RATE // 1000
            b = turn["end_ms"] * RATE // 1000
            body = pcm[a * 2: b * 2]
            silence = b"\0\0" * PAD
            name = f"{take}_{i:02d}.wav"
            with wave.open(str(OUT / name), "wb") as w:
                w.setnchannels(1)
                w.setsampwidth(2)
                w.setframerate(RATE)
                w.writeframes(silence + body + silence)
            manifest.append({"file": name, "text": turn["text"], "seconds": round((b - a + 2 * PAD) / RATE, 2)})
    (OUT / "clips.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"{len(manifest)} clips -> {OUT}")


if __name__ == "__main__":
    main()
