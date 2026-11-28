# Third-party notices

Seogi (서기) includes and builds on the open-source software listed here. Each
entry keeps the project's copyright notice and license terms, as its license
requires.

## Handy

- Project: <https://github.com/cjpais/Handy>
- License: MIT
- What Seogi uses: substantial portions of Seogi's source are derived from
  Handy, including microphone capture, the speech-engine supervisor and model
  manager, global shortcuts, pasting into other apps, the dictation overlay,
  history, and the settings and onboarding screens. Source files derived from
  Handy carry a `Derived from Handy` line in their header. Everything under
  `src-tauri/src/seogi/` and `src/seogi/` is Seogi's own unless a file header
  says otherwise.

```text
MIT License

Copyright (c) 2025 CJ Pais

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## transcribe.cpp Rust bindings

- Project: <https://github.com/handy-computer/transcribe.cpp>
- License: MIT, Copyright (c) 2026 The transcribe.cpp authors
- What Seogi uses: `src-tauri/vendor/transcribe-cpp` is a copy of the
  `transcribe-cpp` 0.4.2 crate with one accessor added for Whisper's decode
  measures. Its license file is kept in that folder.

## transcribe.cpp in the phone app

- Project: <https://github.com/handy-computer/transcribe.cpp>
- License: MIT, Copyright (c) 2026 The transcribe.cpp authors; it carries ggml
  (MIT, Copyright (c) 2023-2026 The ggml authors), Silero VAD (MIT) and miniz
  (MIT)
- What Seogi uses: the Android app's dictation builds transcribe.cpp 0.4.2
  from its source tarball (downloaded at build time, checked by SHA-256) into
  `libseogi_asr.so`; `android/app/src/main/cpp/transcribe_jni.c` is Seogi's
  own bridge to it. All four license texts ship in the app
  (`res/raw/license_transcribe_cpp.txt`, shown under 오픈소스 라이선스).
- The speech model (Qwen3-ASR 0.6B, Apache 2.0) is not part of the app; it is
  downloaded on first use from the same Hugging Face repository and revision as
  the desktop catalog.

## Libraries

Seogi links many Rust crates and npm packages, each under its own license. The
exact versions are pinned in `src-tauri/Cargo.lock` and `bun.lock`, and each
package ships its license text in its source distribution. Notable ones:

- transcribe.cpp and ggml (MIT): the on-device speech engine.
- Tauri (MIT or Apache-2.0): the desktop application framework.
- React (MIT), Tailwind CSS (MIT), Lucide icons (ISC).
- modern-screenshot (MIT), Copyright (c) 2021-present wxm
  <https://github.com/qq15725/modern-screenshot>: draws the one-page summary
  ("한 장 요약") into the picture and the A4 page that are saved.
- The Android app (`android/`): AndroidX, Jetpack Compose, WorkManager and
  Credential Manager (Apache-2.0), Kotlin and kotlinx libraries (Apache-2.0),
  OkHttp and Okio (Apache-2.0),
  Google Identity Services (`play-services-auth`) and the Google ID library
  under the Android Software Development Kit License, for Google sign-in.
  Versions are pinned in `android/gradle/libs.versions.toml`. The app shows
  each licence in full under 설정 › 오픈소스 라이선스 (texts in
  `android/app/src/main/res/raw/`).
- Gradle wrapper (`android/gradle/wrapper/gradle-wrapper.jar`, `android/gradlew`,
  `android/gradlew.bat`), Apache-2.0, Copyright the original authors (Gradle,
  Inc.) <https://gradle.org>. Used only to build, not shipped in the app; CI
  checks the jar against Gradle's published checksums.

## Fonts

- Pretendard (SIL Open Font License 1.1), Copyright (c) 2021 Kil Hyung-jin,
  with Reserved Font Name Pretendard.
  <https://github.com/orioncactus/pretendard>. The typeface of every screen.
  The computer app ships the web files of the `pretendard` npm package
  unchanged; the phone app bundles the original Regular, Medium and Bold
  OpenType files unchanged in `android/app/src/main/res/font/` (under the
  lower-case file names Android requires) and shows the license from
  `android/app/src/main/res/raw/license_ofl_pretendard.txt`. The license text
  is also in `src/seogi/fonts/Pretendard-OFL.txt`. The license allows
  bundling with software and commercial use; selling the font on its own is
  not allowed, and a modified version may not use the font's name.

## Models

Speech models are downloaded by the user from Hugging Face and are not part of
this repository. Each model has its own license, shown on its model page.
