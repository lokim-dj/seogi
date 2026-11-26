# Seogi for Android

The phone app: it records a meeting and uploads the recording to the user's own
Google Drive, into a folder `서기 폰 녹음` that the app makes. The computer's
Seogi watches that folder and writes the minutes. Design:
[`docs/superpowers/specs/2026-11-24-galaxy-app-design.md`](../docs/superpowers/specs/2026-11-24-galaxy-app-design.md).

Kotlin and Jetpack Compose, minimum Android 13 (API 33), target API 35.

## Build

JDK 17 and the Android SDK (platform 35, build-tools 35.0.0).

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
- On stop, the same AAC frames are moved, not re-encoded, into an `.m4a` in
  `Recordings/서기/` (MediaStore). If that fails, the ADTS stream itself is
  kept as `.aac`. The app never changes or deletes that file afterwards.
- Uploads run in one WorkManager queue with Drive's resumable upload, 8 MB at a
  time; an interrupted upload continues from the last piece. A small JSON with
  the start time, length, important marks, calendar title, phone and SHA-256
  goes next to each recording.
