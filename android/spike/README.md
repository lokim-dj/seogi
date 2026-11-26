# Phone dictation engine spike (throwaway)

Which on-phone Korean speech engine is fast and right enough for dictation
(the floating key in the design, "받아쓰기"). Target: text within about 1.5 s
after speaking stops. Not part of the app: its own package
`io.github.lokimdj.seogi.spike`, built only with `-Pseogi.spike`, so CI and
the real app never see it. Delete this folder once the engine is chosen.

## Candidates

| Engine | Runtime | Model (license) | Files on the phone |
| --- | --- | --- | --- |
| `zipformer-ko-int8` | sherpa-onnx 2.0.3 AAR (Apache 2.0) | streaming Zipformer Korean 2024-06-16, int8 encoder/joiner (Apache 2.0) | ~125 MB |
| `qwen3-asr-0.6b-q4_k_m`, `-q8_0` | transcribe.cpp 0.4.2 (MIT), the desktop's engine, built with the NDK for arm64 CPU | Qwen3-ASR 0.6B GGUF, desktop catalog revision (Apache 2.0) | 590 / 850 MB |

Zipformer writes text while you speak and has no punctuation. Qwen3-ASR
starts only after you stop and writes punctuation.

## What it measures

Per utterance: text, **time from the end of speech to the final text**,
compute time and real-time factor, time to the first partial text
(streaming), CER (letters only: spaces and punctuation dropped on both sides,
digits not normalised). Per engine: model size, load time, peak RSS and
anonymous memory, battery temperature and thermal status before and after,
battery charge used (meaningless while charging over USB).

In the file test the Zipformer gets the audio at speaking speed (100 ms
pieces), so its end latency is what a user would feel; Qwen3-ASR gets the
whole clip at the end, as it would after the key is released.

## Build (Mac)

```sh
tools/fetch.sh                                  # AAR into spike/libs, models into ~/seogi/spike-cache
python3 eval/synthetic/make_clips.py dictation_phone
python3 android/spike/tools/cut_clips.py        # 16 lines x clean/noisy, 16 kHz mono, + clips.json
cd android
JAVA_HOME=... ANDROID_HOME=... ./gradlew -Pseogi.spike :spike:testDebugUnitTest :spike:assembleDebug
```

Needs NDK 29.0.14206865 and CMake 3.31.6 (`sdkmanager "ndk;29.0.14206865" "cmake;3.31.6"`).
The transcribe.cpp 0.4.2 source tarball comes from `tools/fetch.sh` (or CMake
downloads it; checksum pinned either way; `SEOGI_TRANSCRIBE_TARBALL` points
elsewhere).

## On the phone

```sh
tools/phone.sh check      # one phone, its model, what is on screen
tools/phone.sh install    # installs the test app beside Seogi
tools/phone.sh push       # ~1.6 GB of models + clips into the test app's folder
tools/phone.sh run        # file test, clean and noisy; results pulled to eval/synthetic/out/dictation_phone/phone/
tools/phone.sh uninstall  # when done
```

Live: open "서기 엔진 시험" on the phone, press the bar once to load the
ticked engines, then hold it, speak, release. Zipformer shows text while you
speak; each engine's time after release is logged and saved in `results/`.
The live audio is not saved.

`tools/mac_eval.py` runs the same clips on the Mac's CPU (accuracy only; the
Mac's times are not the phone's).

## Mac CPU results (11/27, NOT the phone)

`tools/mac_eval.py`, MacBook Pro M4 CPU (no GPU), 16 clips × clean/noisy,
Qwen3 4 threads, Zipformer 2 threads. CER carries over to the phone; the
times do not (the phone's CPU is slower).

| Engine | CER clean | CER noisy | end of speech → text (median) |
| --- | --- | --- | --- |
| zipformer-ko-int8 | 0.280 | 0.655 | 9 ms (text already there while speaking) |
| qwen3-asr-0.6b-q4_k_m | 0.031 | 0.066 | 587 ms |
| qwen3-asr-0.6b-q8_0 | 0.011 | 0.035 | 486 ms |

The Zipformer with full-precision weights was no better (clean 0.302,
noisy 0.593). The clips are one synthetic voice; the live test with a real
voice is what decides.
