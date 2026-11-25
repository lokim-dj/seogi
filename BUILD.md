# 빌드하기 / Build instructions

## 준비물

모든 플랫폼: [Rust](https://rustup.rs/) (stable), [Bun](https://bun.sh/), [CMake](https://cmake.org/download/), 그리고 [Tauri의 준비물](https://tauri.app/start/prerequisites/).

### macOS

- Xcode Command Line Tools: `xcode-select --install`
- Homebrew를 쓴다면: `brew install rustup cmake` 후 `rustup default stable`. Homebrew의 rustup은 PATH에 자동으로 잡히지 않으므로 `export PATH="$(brew --prefix rustup)/bin:$PATH"`가 필요합니다.
- Apple Intelligence 연결은 전체 Xcode가 있을 때만 빌드됩니다. Command Line Tools만 있으면 빈 구현으로 대체되고 나머지는 그대로 동작합니다.

### Windows

- Visual Studio 2022(또는 Build Tools)의 "C++를 사용한 데스크톱 개발"
- CMake: `winget install Kitware.CMake`
- Vulkan SDK(GPU 가속 빌드에 필요): `winget install KhronosGroup.VulkanSDK` — 설치 후 새 터미널을 여세요.

## 실행과 빌드

```bash
bun install
bun run tauri dev            # 개발 실행
bun run tauri build          # 릴리스 빌드와 설치 파일
```

macOS에서 CMake 오류가 나면 `CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev`로 실행합니다.

음성 구간 검출 모델은 저장소에 들어 있습니다(`src-tauri/resources/models/silero-vad-v6.2-F32.gguf`). 음성 인식 모델은 앱에서 내려받습니다.

## 문제 해결

### macOS: 다시 빌드한 뒤 손쉬운 사용 권한이 켜져 있는데도 단축키가 안 먹을 때

로컬 빌드는 임시 서명(`signingIdentity: "-"`)이라, 다시 빌드하면 macOS가 다른 앱으로 봅니다. (내려받는 판은 GitHub 에서 자체 서명 인증서로 서명되어 판이 바뀌어도 같은 앱으로 보입니다. 인증서는 주인의 비공개 폴더와 저장소 Secrets 에만 있고, 비밀값이 없는 빌드(포크 등)는 임시 서명으로 돌아갑니다. 자세한 것은 [docs/RELEASING.md](docs/RELEASING.md).) 시스템 설정 › 개인정보 보호 및 보안 › 손쉬운 사용에서 서기 항목을 지웠다가 다시 추가하거나, 아래처럼 서기의 기록만 초기화합니다.

```bash
tccutil reset Accessibility io.github.lokimdj.seogi
```

### Windows: 경로 길이 오류(`MSB3491` / `FTK1011` / `MSB6003`)

Windows의 260자 경로 제한 때문입니다. 짧은 빌드 폴더를 지정하면 해결됩니다.

```powershell
$env:CARGO_TARGET_DIR = "C:\s"
```

## 테스트와 점검

```bash
cd src-tauri && cargo test          # Rust 테스트
bun run lint                        # ESLint
bun run format:check                # Prettier + cargo fmt
python3 eval/synthetic/make_clips.py  # 합성 시험 녹음 만들기 (macOS)
```
