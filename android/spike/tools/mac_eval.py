#!/usr/bin/env python3
"""The same clips and engines on the Mac's CPU, for accuracy before the phone.

NOT phone numbers: the Mac's cores are much faster, so only CER carries over
(same model files, same decoding). Needs a venv with `sherpa-onnx` and
`transcribe-cpp==0.4.2` (the desktop's engine version):

    python3 -m venv ~/seogi/spike-cache/venv
    ~/seogi/spike-cache/venv/bin/pip install sherpa-onnx numpy transcribe-cpp==0.4.2
    ~/seogi/spike-cache/venv/bin/python android/spike/tools/mac_eval.py [--threads 4]
"""
import argparse
import json
import statistics
import sys
import time
import wave
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[3]
CLIPS = ROOT / "eval/synthetic/out/dictation_phone/lines"
CACHE = Path.home() / "seogi/spike-cache"


def norm(s):
    return "".join(c for c in s.lower() if c.isalnum())


def lev(a, b):
    prev = list(range(len(b) + 1))
    for i in range(1, len(a) + 1):
        cur = [i] + [0] * len(b)
        for j in range(1, len(b) + 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] != b[j - 1]))
        prev = cur
    return prev[-1]


def cer(ref, hyp):
    r, h = norm(ref), norm(hyp)
    return lev(r, h) / max(1, len(r))


def read(path):
    with wave.open(str(path)) as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768


def zipformer(threads):
    import sherpa_onnx
    d = CACHE / "zipformer-ko"
    rec = sherpa_onnx.OnlineRecognizer.from_transducer(
        tokens=str(d / "tokens.txt"), encoder=str(d / "encoder-epoch-99-avg-1.int8.onnx"),
        decoder=str(d / "decoder-epoch-99-avg-1.onnx"), joiner=str(d / "joiner-epoch-99-avg-1.int8.onnx"),
        num_threads=threads, sample_rate=16000, feature_dim=80, decoding_method="greedy_search")

    def run(pcm):
        s = rec.create_stream()
        compute = 0.0
        for i in range(0, len(pcm), 1600):  # 100 ms at a time, as from a microphone
            t = time.perf_counter()
            s.accept_waveform(16000, pcm[i:i + 1600])
            while rec.is_ready(s):
                rec.decode_stream(s)
            compute += time.perf_counter() - t
        t = time.perf_counter()
        s.accept_waveform(16000, np.zeros(4800, dtype=np.float32))
        s.input_finished()
        while rec.is_ready(s):
            rec.decode_stream(s)
        end = time.perf_counter() - t
        return rec.get_result(s).strip(), end, compute + end
    return run


def qwen(gguf, threads):
    import transcribe_cpp as tc
    model = tc.Model(str(gguf), backend="cpu")
    sess = model.session(n_threads=threads)

    def run(pcm):
        t = time.perf_counter()
        text = sess.run(pcm, language="ko").text
        d = time.perf_counter() - t
        return text.strip(), d, d
    return run


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--takes", default="clean")
    args = ap.parse_args()
    clips = [c for c in json.loads((CLIPS / "clips.json").read_text()) if c["file"].split("_")[0] in args.takes.split(",")]
    engines = [("zipformer-ko-int8", lambda: zipformer(2))]
    for q in ("Q4_K_M", "Q8_0"):
        f = CACHE / f"qwen3/Qwen3-ASR-0.6B-{q}.gguf"
        if f.exists():
            engines.append((f"qwen3-asr-0.6b-{q.lower()}", lambda f=f: qwen(f, args.threads)))
    out = {"note": "Mac CPU, not the phone", "rows": [], "summary": []}
    for name, make in engines:
        t = time.perf_counter()
        run = make()
        load = time.perf_counter() - t
        run(read(CLIPS / clips[0]["file"]))  # warm-up
        lat, cers, rtfs = [], [], []
        for c in clips:
            text, end, compute = run(read(CLIPS / c["file"]))
            e = cer(c["text"], text)
            lat.append(end * 1000); cers.append(e); rtfs.append(compute / c["seconds"])
            out["rows"].append({"engine": name, "clip": c["file"], "cer": e, "end_ms": end * 1000, "ref": c["text"], "text": text})
            print(f"{name} {c['file']} {end*1000:6.0f} ms CER {e:.2f} | {text}", file=sys.stderr)
        lat.sort()
        s = {"engine": name, "n": len(clips), "load_s": round(load, 2), "median_end_ms": round(statistics.median(lat)),
             "p90_end_ms": round(lat[int(0.9 * (len(lat) - 1) + 0.5)]), "mean_cer": round(sum(cers) / len(cers), 4),
             "mean_rtf": round(sum(rtfs) / len(rtfs), 3)}
        out["summary"].append(s)
        print(json.dumps(s, ensure_ascii=False))
    dest = CLIPS.parent / f"mac_eval_{args.takes.replace(',', '+')}.json"
    dest.write_text(json.dumps(out, ensure_ascii=False, indent=1))
    print(f"-> {dest}")


if __name__ == "__main__":
    main()
