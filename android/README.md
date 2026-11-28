# Seogi for Android

The phone app: it records a meeting and uploads the recording to the user's own
Google Drive, into a folder `서기 폰 녹음` that the app makes. The computer's
Seogi watches that folder and writes the minutes. It also types by voice into
other apps (말로 입력, a floating microphone key; see below). Design:
[`docs/specs/2026-11-24-galaxy-app-design.md`](../docs/specs/2026-11-24-galaxy-app-design.md).

Kotlin and Jetpack Compose, minimum Android 13 (API 33), target API 35.

## Build

JDK 17 and the Android SDK (platform 35, build-tools 35.0.0), NDK
29.0.14206865 and CMake 3.31.6 (Gradle installs the last two when they are
missing; `sdkmanager "ndk;29.0.14206865" "cmake;3.31.6"` does it ahead). The
native build downloads the transcribe.cpp 0.4.2 source tarball from GitHub and
checks its SHA-256; to build offline, point `seogi.transcribeTarball` in
`local.properties` (or `SEOGI_TRANSCRIBE_TARBALL`) at a local copy of the same
file.

```bash
cd android
./gradlew testDebugUnitTest      # unit tests
./gradlew assembleDebug          # app/build/outputs/apk/debug/app-debug.apk
./gradlew bundleRelease          # app/build/outputs/bundle/release/app-release.aab
```

## Values that stay off the repository

Put them in `android/local.properties` (gitignored) or the matching
environment variables. Without them the debug build still works; Drive
sign-in needs the OAuth client below.

| Key                          | Environment variable          | What it is                                                                               |
| ---------------------------- | ----------------------------- | ---------------------------------------------------------------------------------------- |
| `seogi.signing.storeFile`    | `SEOGI_SIGNING_STORE_FILE`    | The upload keystore (kept outside the repository)                                        |
| `seogi.signing.passwordFile` | `SEOGI_SIGNING_PASSWORD_FILE` | A file holding its password (or `seogi.signing.storePassword`)                           |
| `seogi.signing.keyAlias`     | `SEOGI_SIGNING_KEY_ALIAS`     | The key's alias (default `upload`)                                                       |
| `seogi.googleWebClientId`    | `SEOGI_GOOGLE_WEB_CLIENT_ID`  | Optional: a Web OAuth client ID, to show Google's account picker before asking for Drive |

Google sign-in for Drive needs an **Android OAuth client** in the Google Cloud
project, with the package name `io.github.lokimdj.seogi` and the SHA-1 of each
certificate that signs the app: the debug keystore for local builds, the upload
key, and the Play app signing key for builds installed from the Play Store. The
app asks for one scope, `drive.file` (only the files the app made); no client
secret is in the app.

## How recording stays safe

- A microphone foreground service records AAC (mono, 48 kHz, 64 kbps) as an
  ADTS stream in the app's own storage, so a crash or a kill keeps everything
  up to the last frame. The next start finishes such a recording and marks it
  "되살림".
- On stop, the same AAC frames are moved, not re-encoded, straight into an
  `.m4a` in `Recordings/서기/` (MediaStore). A recording starts only with
  200 MB free and stops (saving what it has) when room runs short. If that fails, the ADTS stream itself is
  kept as `.aac`. The app never changes or deletes that file afterwards.
- Uploads run in one WorkManager queue with Drive's resumable upload, 8 MB at a
  time; an interrupted upload continues from the last piece. A small JSON with
  the start time, length, important marks, calendar title, phone and SHA-256
  goes next to each recording.

## Deleting

- Only the user deletes, and only after a question: "버리기" while recording
  (nothing is saved), or a long press on a recording in the lists.
- A confirmed delete waits eight seconds for "되돌리기" (undo); until then
  nothing happens to the file, the list or Drive. Leaving the app ends the
  wait; turning the phone or the screen going off does not.
- With "드라이브에 올린 것도 휴지통으로" (on by default) the recording and its
  JSON go to the Drive trash with `files.update trashed=true`, never a permanent
  delete. An upload still under way is stopped, and either trashed (if it had
  just finished) or given up. Every file in the folder that belongs to the
  recording goes, found by name and by the recording id each upload carries
  (`appProperties seogi_rec`), so a copy the list lost track of is not left
  behind. A file uploaded with another account is left alone and the user is
  told.
- A sidecar JSON is made once: a retry first looks for the one an earlier try
  made (its answer lost, or the list forgot its id) and updates it.

## 말로 입력 (dictation)

A round microphone key floats over other apps. Hold it and speak, or tap to
start and tap to stop; the words go into the field that has the cursor.

- Turned on in 설정 › 말로 입력, one step a screen: microphone and
  notifications, "다른 앱 위에 표시", the accessibility service, the model.
- Speech becomes text on the phone: Qwen3-ASR 0.6B Q8_0 (~850 MB, Apache 2.0)
  through transcribe.cpp 0.4.2 on the CPU (arm64 with dot product and int8
  matrix instructions; the app checks the CPU first). The model is never in
  the APK: it is downloaded after the user asks, continuing where it stopped,
  and named only after its SHA-256 matches the desktop catalog's. Loaded on
  the first use, let go after three minutes without speaking.
- The sound stays in memory, is never written or sent, and is dropped once it
  has become text. Long speech is cut into pieces of at most 25 s at quiet
  moments; listening stops on its own after two minutes.
- Spoken numbers before a unit are written as digits by the computer's rules
  ("세 시 반" → "3시 반", "이십 프로 할인" → "20% 할인"; "세 번째" and
  "아이폰 십오 프로" stay). `dictation/Numbers.kt` is a port of the desktop's
  `src-tauri/src/seogi/numbers.rs`, and both are tested against the same
  sentences in `src-tauri/src/seogi/numbers.cases.json`, so a rule changed on
  one side fails the other side's test. Punctuation is kept; nothing else
  tidies the text.
- Text goes in through an accessibility service (`InsertService`), which only
  looks for the focused field when there is text to put in. Without it, or
  when no field has the cursor, the text is copied and a toast says
  "복사했어요, 붙여 넣으세요". Google Play asks apps that use an accessibility
  service for something other than helping people with disabilities for a
  declaration; the in-app screen is the disclosure and consent it requires.
- The key and the engine run in their own process (`:dictation`), so a crash
  or a kill there never stops a meeting being recorded. While a meeting is
  recorded the key does not listen.
- No notification while the key waits, when accessibility is on: the
  accessibility service holds the key's service bound (Android keeps an
  accessibility service running) and shows the key as its own overlay
  (`TYPE_ACCESSIBILITY_OVERLAY`; that sits above the keyboard and the lock
  screen, so the key keeps above the keyboard's top edge and never shows on
  the lock screen). Only while the microphone is open is it a
  microphone foreground service, started from the key the user is touching:
  Android lets an app with a window on screen start one from behind another
  app. That notification has no buttons, so Android holds it back for ten
  seconds and a short sentence shows only the microphone dot. Without
  accessibility nothing else keeps the key, so it stays a foreground service
  with its notification (Android raises a lowest-importance channel to low for
  a foreground service, so hiding it that way does not work).
