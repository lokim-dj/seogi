# 갤럭시 앱 첫 판(1단계: 녹음 + 드라이브 올리기) 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 폰에서 끊기지 않게 녹음하고, 끝나면 묻지 않고 내 구글 드라이브의 `서기 폰 녹음` 폴더로 올리는 안드로이드 앱 첫 판.

**Architecture:** `android/` 의 단일 모듈 앱(`io.github.lokimdj.seogi`). 녹음은 마이크 포그라운드 서비스가 AAC 를 ADTS 스트림으로 앱 폴더에 계속 써서 강제 종료에도 앞부분이 남고, 끝나면 같은 AAC 프레임을 다시 인코딩하지 않고 m4a 로 옮겨 담아 MediaStore `Recordings/서기/` 에 둔다(이후 원본 불변). 올리기는 WorkManager 한 줄 대기열이 Drive REST v3 이어 올리기(resumable)로 처리한다. 화면은 Compose 다섯 장(처음 실행 A · 홈 A · 녹음 중 · 올리기 · 설정)과 받아쓰기 빈 자리 한 장.

**Tech Stack:** Kotlin 2.0, Jetpack Compose(BOM), AndroidX WorkManager · Credentials, Google Identity Services(AuthorizationClient), kotlinx.serialization, AGP 8.7 / Gradle 8.11, minSdk 33 · targetSdk 35 · build-tools 35.0.0.

**Spec:** `docs/specs/2026-11-24-galaxy-app-design.md`

## Global Constraints

- 구글 권한은 `https://www.googleapis.com/auth/drive.file` 하나. 캘린더는 폰 캘린더(`CalendarContract`)에서 읽는다.
- OAuth 클라이언트 정보 · 키스토어 · 암호는 저장소에 넣지 않는다. 빌드는 `android/local.properties`(gitignore) 또는 환경 변수에서 읽는다.
- 원본 녹음은 앱이 다시 인코딩 · 자르기 · 지우기를 하지 않는다. 올린 뒤에도 폰에 남는다.
- 녹음이 끝난 뒤 아무것도 묻지 않는다. 실패는 다시 하고, 사용자가 볼 수 있게 빨강으로 남긴다.
- 파일 이름 `YYYY-MM-DD HHMM 일정제목.m4a`(일정 없으면 시각만), 옆에 같은 이름의 `.json`(시작 시각 · 길이 · 중요 표시 · 일정 제목 · 기기 · SHA-256).
- 화면 말투 · 색은 시안(먹색 바탕, 빨강은 녹음 · 실패에만, 지마켓 산스 Bold 본문). 이모지 · 카드 · 굵은 상자 없음. 회사 · 사람 이름 없음.
- 새 SDK 구성요소를 받지 않는다(설치된 android-35 · build-tools 35.0.0 만).

## Review Focus

1. 녹음 중 앱이 강제 종료됨 → 다음 실행 때 그때까지의 녹음이 목록에 '되살림'으로 나타나고 올라간다. (Task 2 `RecoveryTest`)
2. 일정 제목에 `/` `:` 같은 파일 이름에 못 쓰는 글자가 있음 → 파일 이름에서 빼고 제목 표시에는 그대로. (Task 1 `FileNamesTest`)
3. 올리는 도중 인터넷이 끊기거나 일이 10분 제한으로 멈춤 → 처음부터가 아니라 올린 곳 다음부터 이어 올린다. (Task 3 `ResumableUploadTest`)
4. 로그인이 풀림(토큰 만료 · 권한 회수) → 조용히 멈추지 않고 '로그인 필요' 빨강 + 다시 누를 길. (Task 3 `UploadQueueTest`)
5. 폰의 원본을 사용자가 지움 → 앱이 되살리지 않고 '폰에 없음'으로 표시, 올리기는 실패로 남김. (Task 3 `UploadQueueTest`)

---

### Task 1: 뼈대 · 이름 규칙 · 기록 저장소

**Files:**

- Create: `android/settings.gradle.kts`, `android/build.gradle.kts`, `android/gradle.properties`, `android/gradle/libs.versions.toml`, `android/gradlew*`, `android/app/build.gradle.kts`, `android/app/src/main/AndroidManifest.xml`, 아이콘 · 글꼴 리소스
- Create: `android/app/src/main/java/io/github/lokimdj/seogi/data/FileNames.kt`, `data/Recording.kt`, `data/RecordingStore.kt`, `data/Settings.kt`
- Test: `android/app/src/test/.../FileNamesTest.kt`, `RecordingStoreTest.kt`

**Interfaces (Produces):**

- `object FileNames { fun baseName(start: ZonedDateTime, eventTitle: String?): String; fun displayTitle(start: ZonedDateTime, eventTitle: String?): String }`
- `@Serializable data class Recording(id, title, startedAt: Long, durationMs: Long, marksMs: List<Long>, fileName, mediaUri: String?, sizeBytes, sha256, eventTitle: String?, recovered: Boolean, upload: UploadState, progress: Float, error: UploadError?, driveFileId: String?, sessionUri: String?)`
- `enum UploadState { WAITING, UPLOADING, UPLOADED, FAILED }`, `enum UploadError { OFFLINE, NEED_LOGIN, MISSING_ON_PHONE, SERVER }`
- `class RecordingStore(file: File) { val all: StateFlow<List<Recording>>; suspend fun upsert(r); suspend fun update(id, (Recording) -> Recording) }` — JSON 파일 하나, 임시 파일에 쓰고 이름 바꾸기.

- [ ] 시험: `baseName(2026-11-26T14:05, "주간 회의") == "2026-11-26 1405 주간 회의"`, 일정 없음 → `"2026-11-26 1405"`, `"a/b:c"` → `"abc"` 처럼 금지 글자 제거, 80자 넘으면 자름. `displayTitle(…, null) == "14:05 녹음"`.
- [ ] 시험: `RecordingStore` 에 넣고 새로 열면 그대로, 망가진 파일이면 빈 목록이 아니라 `.broken` 으로 옮기고 시작.
- [ ] 구현 → `./gradlew testDebugUnitTest` 통과 → 커밋.

### Task 2: 녹음 서비스 · 잠금 화면 알림 · 되살리기

**Files:** `record/RecorderService.kt`, `record/RecordingSession.kt`(진행 중 상태 JSON), `record/Finalizer.kt`(ADTS → m4a 옮겨 담기 · SHA-256 · MediaStore), `record/Calendar.kt`, `record/Notifications.kt`; Test: `RecoveryTest.kt`, `SessionStateTest.kt`

**Interfaces:**

- `RecorderService` 액션 `START` · `MARK` · `STOP`, 공개 상태 `RecorderService.state: StateFlow<RecState>` (`Idle` | `Recording(title, startedAt, marksMs, level)`).
- `RecordingSession.write(dir, SessionState)` / `read(dir): SessionState?` — 시작 · 중요 표시마다 즉시 저장.
- `Finalizer.finalize(context, adts: File, state): Recording` — `MediaExtractor` + `MediaMuxer` 로 옮겨 담기(실패하면 `.aac` 그대로 원본으로), `IS_PENDING` 으로 쓰고 다 쓴 뒤 0.
- `Recovery.findLeftovers(dir): List<Pair<File, SessionState>>`
- `CalendarReader.currentOrSoon(now): String?` (진행 중이거나 10분 안에 시작, 종일 일정 제외), `next(now): Event?`

- [ ] 시험: 상태 JSON 쓰기 → 읽기 같음, 중요 표시 추가 반영. 남은 `.aac` + 상태가 있으면 `findLeftovers` 가 찾고, 상태 없이 `.aac` 만 있어도 파일 시각으로 찾음.
- [ ] 구현: MediaRecorder `AAC_ADTS` · AAC · 모노 · 48 kHz · 64 kbps, 부분 wake lock, `foregroundServiceType="microphone"`. 알림은 `VISIBILITY_PUBLIC` · 시계(`setUsesChronometer`) · 작업 단추 '중요 표시' '끝내기'.
- [ ] 커밋.

### Task 3: 구글 로그인 · 드라이브 올리기 대기열

**Files:** `upload/GoogleAuth.kt`, `upload/DriveApi.kt`, `upload/UploadWorker.kt`, `upload/UploadQueue.kt`; Test: `ResumableUploadTest.kt`(가짜 HTTP), `UploadQueueTest.kt`

**Interfaces:**

- `GoogleAuth.authorizeIntent(activity)`(처음 한 번 동의), `GoogleAuth.token(context): TokenResult` (`Ok(token)` | `NeedLogin`) — 일 안에서는 화면 없이.
- `DriveApi(http: Http) { ensureFolder(token): String; startSession(token, folderId, name, mime, size): String; resume(token, session, file, from): Long; uploadSmall(token, folderId, name, json): String; aboutEmail(token): String }` — `Http` 는 시험에서 바꿔 끼우는 작은 인터페이스.
- `UploadQueue.enqueue(context, replace = false)` — 고유 일 `upload-queue`, 와이파이만 켜짐이면 `UNMETERED`, 아니면 `CONNECTED`, 지수 대기 30초부터.
- `UploadWorker` — 대기 · 실패(다시 할 수 있음) 녹음을 오래된 것부터 하나씩: `.json` 먼저, 녹음은 8 MB 씩 이어 올리기, 조각마다 진행률 저장.

- [ ] 시험: 세션 상태 조회(`Content-Range: bytes */N`)가 `Range: bytes=0-8388607` 를 돌려주면 다음 조각은 8388608 부터. 401 → `NeedLogin` 으로 바뀜. 원본이 없으면 `MISSING_ON_PHONE` · 실패.
- [ ] 구현: `AuthorizationClient` 로 `drive.file`. `seogi.googleWebClientId` 가 빌드에 있으면 Credential Manager 로 계정 먼저 고르기(없으면 건너뜀).
- [ ] 커밋.

### Task 4: 화면

**Files:** `ui/Theme.kt`, `ui/Mark.kt`, `ui/OnboardingScreen.kt`, `ui/HomeScreen.kt`, `ui/RecordingScreen.kt`, `ui/UploadsScreen.kt`, `ui/SettingsScreen.kt`, `ui/DictationScreen.kt`, `MainActivity.kt`

- [ ] 처음 실행 A: 마이크 · 알림 · 캘린더(골라도 됨) · 구글 드라이브 네 줄, 'n / 4 됨', 마이크 · 알림이 되면 '시작하기'.
- [ ] 홈 A: 다음 일정 한 줄, 240dp 기운 타원이 곧 녹음 단추(녹음 중이면 가운데 빨간 점 1.6초 깜빡임), 최근 녹음 세 줄과 상태.
- [ ] 녹음 중: 큰 시간 · 소리 막대 · 중요 표시 목록 · 중요 표시 / 끝내기(길게 누르기).
- [ ] 올리기: 올리는 곳 · 계정, 한 줄에 상태 하나(올리는 중 n% · 기다림 · 실패 · 지금 다시 · 올림), 와이파이에서만 올리기.
- [ ] 설정: 구글 계정, 와이파이에서만 올리기, 받아쓰기(다음 판, 빈 자리), 개인정보처리방침 · 불편해요(GitHub 이슈) · 버전.
- [ ] 어둡게 · 밝게 모두 눈으로 확인(에뮬레이터가 없으면 Compose 미리보기 렌더) → 커밋.

### Task 5: 서명 · CI · 개인정보처리방침

- [ ] `android/app/build.gradle.kts`: `local.properties` 의 `seogi.signing.*` 가 있을 때만 release 서명. 업로드 키스토어는 저장소 밖 `~/seogi/private/android/`(암호 파일 600).
- [ ] `.github/workflows/check.yml` 에 `android` 작업: JDK 17, `./gradlew testDebugUnitTest assembleDebug`(키스토어 없이).
- [ ] `docs/privacy.md` 한국어 + 영어.
- [ ] `./gradlew assembleDebug bundleRelease` 성공, 폰이 연결돼 있으면 설치해 녹음 → 저장 확인.

## 드라이브 폴더 규칙 (폰 · 맥 · 윈도우 공통, 2단계에서도 지킬 것)

같은 구글 계정으로 연결한 기기는 녹음 폴더 하나를 함께 쓴다. 폴더는 이름이 아니라 표식으로 찾는다. 앱이 폴더를 만들 때 `appProperties` 에 `seogi=recordings-root` 를 붙인다. 연결할 때는 먼저 `appProperties has { key='seogi' and value='recordings-root' } and mimeType = 'application/vnd.google-apps.folder' and trashed = false` 로 찾고(`orderBy=createdTime`), 여러 개면 가장 오래된 것을 쓴다. 하나도 없을 때만 `서기 폰 녹음` 을 표식과 함께 만든다. 이름만 같고 표식이 없는 폴더는 쓰지 않는다. 저장해 둔 폴더 ID 는 올리기마다(한 번의 실행에 한 번) 있는지 · 폴더인지 · 휴지통이 아닌지 · 표식이 있는지 확인하고, 아니면 위 순서로 다시 찾거나 만든다. 이때 반쯤 올린 녹음은 새 폴더에서 처음부터 올리고, 이미 다 올린 녹음은 사용자가 지운 것이므로 다시 올리지 않는다. 표식은 `drive.file` 범위에서 같은 클라우드 프로젝트의 앱끼리만 보이는 것으로 본다(구글 Picker 문서가 `drive.file` 에 쓰는 앱 ID 를 클라우드 프로젝트 번호로 정의). 그래서 맥 · 윈도우 클라이언트도 같은 프로젝트에 만든다. 서로 다른 클라이언트끼리 실제로 보이는지는 두 번째 기기가 생기면 실측한다.
