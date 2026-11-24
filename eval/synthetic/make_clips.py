#!/usr/bin/env python3
"""Synthesizes Korean test recordings from a script, with ground truth.

macOS only: it speaks each line with `say`, so every clip comes with an exact
transcript, speaker labels and turn times. The clips are synthetic on purpose:
real recordings never belong in this repo.

macOS ships one voice that reads Korean (Yuna). Further speakers are made from
it by shifting pitch and formants together, which is what a smaller or larger
speaker sounds like: `# voices: 실장=Yuna 소장=Yuna@0.80 고객=Yuna@1.18`.

    python3 eval/synthetic/make_clips.py                 # every script
    python3 eval/synthetic/make_clips.py consult_3spk    # one script

Each script produces, under eval/synthetic/out/<name>/:
    clean.wav        16 kHz mono, speech only
    noisy.wav        the same with room echo and pink noise at 15 dB SNR
    phone.m4a        AAC, the format phone recorders and Voice Memos write
    call.amr         AMR narrowband 8 kHz, as old call recorders write (only if
    call.3gp         ffmpeg has the AMR encoder), and the same in a 3GPP container
    memo.mp3         MP3
    truth.json       turns: speaker, start_ms, end_ms, text
    reference.txt    the transcript as plain text, one turn per line
    vocabulary.txt   the script's vocabulary line, one term per line
"""

from __future__ import annotations

import json
import random
import subprocess
import sys
import tempfile
import wave
from array import array
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE / "scripts"
OUT = HERE / "out"
RATE = 16_000
SPEECH_RATE_WPM = 165


def run(*args: str) -> None:
    subprocess.run(args, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def parse_script(path: Path):
    voices: dict[str, str] = {}
    meta: dict[str, str] = {}
    turns: list[tuple[str, str]] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith("#"):
            body = line.lstrip("#").strip()
            if body.startswith("voices:"):
                for pair in body.removeprefix("voices:").split():
                    speaker, voice = pair.split("=")
                    voices[speaker] = voice
            elif ":" in body:
                key, value = body.split(":", 1)
                if key.strip() in ("title", "vocabulary"):
                    meta[key.strip()] = value.strip()
            continue
        speaker, text = line.split(":", 1)
        turns.append((speaker.strip(), text.strip()))
    missing = {speaker for speaker, _ in turns} - voices.keys()
    if missing:
        raise SystemExit(f"{path.name}: no voice for {sorted(missing)}")
    return voices, meta, turns


def speak(voice: str, text: str, workdir: Path) -> array:
    """One line of speech as 16 kHz mono int16 samples."""
    name, _, shift = voice.partition("@")
    spoken = workdir / "spoken.wav"
    run(
        "say", "-v", name, "-r", str(SPEECH_RATE_WPM),
        "--file-format=WAVE", f"--data-format=LEI16@{RATE}", "-o", str(spoken), text,
    )
    target = spoken
    if shift:
        # Play it faster or slower (pitch and formants move together), then
        # stretch it back to the original tempo.
        factor = float(shift)
        target = workdir / "shifted.wav"
        run(
            "ffmpeg", "-y", "-i", str(spoken),
            "-af", f"asetrate={RATE * factor:.0f},aresample={RATE},atempo={1 / factor:.4f}",
            "-ar", str(RATE), "-ac", "1", "-c:a", "pcm_s16le", str(target),
        )
    with wave.open(str(target), "rb") as wav:
        assert wav.getframerate() == RATE and wav.getnchannels() == 1 and wav.getsampwidth() == 2
        samples = array("h")
        samples.frombytes(wav.readframes(wav.getnframes()))
    samples = trim_silence(samples)
    if len(samples) < RATE // 10:
        raise SystemExit(f"voice {voice!r} said nothing for: {text[:30]}… (it may not read Korean)")
    return samples


def trim_silence(samples: array, threshold: int = 200) -> array:
    """Drops the silence `say` pads a line with, so turn times are the speech itself."""
    start, end = 0, len(samples)
    while start < end and abs(samples[start]) < threshold:
        start += 1
    while end > start and abs(samples[end - 1]) < threshold:
        end -= 1
    pad = RATE // 50  # keep 20 ms each side so onsets are not clipped
    return samples[max(0, start - pad): min(len(samples), end + pad)]


def write_wav(path: Path, samples: array) -> None:
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(samples.tobytes())


def build(script: Path) -> None:
    name = script.stem
    voices, meta, turns = parse_script(script)
    out = OUT / name
    out.mkdir(parents=True, exist_ok=True)
    rng = random.Random(name)  # the same script always gives the same timing

    audio = array("h", [0] * (RATE // 2))  # half a second of lead-in
    truth = []
    previous = None
    with tempfile.TemporaryDirectory() as tmp:
        for speaker, text in turns:
            line = speak(voices[speaker], text, Path(tmp))
            gap = rng.uniform(0.15, 0.4) if speaker == previous else rng.uniform(0.35, 0.9)
            audio.extend([0] * int(gap * RATE))
            start = len(audio)
            audio.extend(line)
            truth.append({
                "speaker": speaker,
                "start_ms": round(start * 1000 / RATE),
                "end_ms": round(len(audio) * 1000 / RATE),
                "text": text,
            })
            previous = speaker
    audio.extend([0] * (RATE // 2))

    clean = out / "clean.wav"
    write_wav(clean, audio)
    (out / "truth.json").write_text(
        json.dumps({"title": meta.get("title", name), "speakers": sorted(voices), "turns": truth},
                   ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    (out / "reference.txt").write_text(
        "\n".join(f"{t['speaker']}: {t['text']}" for t in truth) + "\n", encoding="utf-8"
    )
    terms = [term.strip() for term in meta.get("vocabulary", "").split(",") if term.strip()]
    (out / "vocabulary.txt").write_text("\n".join(terms) + "\n", encoding="utf-8")

    # A meeting-room take: a little echo, then pink noise 15 dB under the speech.
    run(
        "ffmpeg", "-y", "-i", str(clean),
        "-f", "lavfi", "-i", f"anoisesrc=color=pink:sample_rate={RATE}:amplitude=0.5",
        "-filter_complex",
        "[0:a]aecho=0.8:0.7:40|75:0.25|0.15,volume=0.9[speech];"
        "[1:a]volume=-27dB[noise];"
        "[speech][noise]amix=inputs=2:duration=first:normalize=0[out]",
        "-map", "[out]", "-ar", str(RATE), "-ac", "1", str(out / "noisy.wav"),
    )
    run("ffmpeg", "-y", "-i", str(clean), "-ar", "44100", "-c:a", "aac", "-b:a", "64k", str(out / "phone.m4a"))
    # AMR needs ffmpeg built with libopencore-amrnb, which Homebrew's is not.
    # Without it the two call clips are skipped; nothing else depends on them.
    for call in ("call.amr", "call.3gp"):
        try:
            run("ffmpeg", "-y", "-i", str(clean), "-ar", "8000", "-ac", "1",
                "-c:a", "libopencore_amrnb", "-b:a", "12.2k", str(out / call))
        except subprocess.CalledProcessError:
            (out / call).unlink(missing_ok=True)
            print(f"{name}: skipped {call} (this ffmpeg has no AMR encoder)")
    run("ffmpeg", "-y", "-i", str(clean), "-c:a", "libmp3lame", "-b:a", "64k", str(out / "memo.mp3"))

    seconds = len(audio) / RATE
    print(f"{name}: {len(turns)} turns, {seconds:.0f}s -> {out}")


def main() -> None:
    wanted = set(sys.argv[1:])
    scripts = sorted(p for p in SCRIPTS.glob("*.txt") if not wanted or p.stem in wanted)
    if not scripts:
        raise SystemExit("no matching script")
    # Invented names must not be real ones (see check_names.py). Where a
    # private vocabulary exists this stops before anything is spoken.
    checked = subprocess.run([sys.executable, str(HERE / "check_names.py"), *map(str, scripts)])
    if checked.returncode != 0:
        raise SystemExit(checked.returncode)
    for script in scripts:
        build(script)


if __name__ == "__main__":
    main()
