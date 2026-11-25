# 판 내기

순서대로 합니다. 한 단계가 빨간불이면 다음으로 가지 않습니다. 판을 내는 것은 주인이 정합니다(AGENTS.md, Who decides).

판을 내는 동안(4번 머지부터 7번 Publish 까지) main 에는 아무것도 머지하지 않습니다. 초안과 태그가 가리키는 커밋이 어긋납니다.

1.  **main 이 초록인지 봅니다.** 마지막 커밋의 Check 가 통과. Build 는 앱 코드가 바뀐 커밋에만 돌므로, 앱 코드가 마지막으로 바뀐 커밋의 Build 가 통과했는지 봅니다(Actions › Build). 문서만 바뀐 커밋에는 Build 가 없습니다.
2.  **버전을 올립니다.** `package.json` · `src-tauri/tauri.conf.json` · `src-tauri/Cargo.toml` 세 곳을 고치고, `cd src-tauri && cargo metadata --format-version 1 > /dev/null` 로 `Cargo.lock` 의 seogi 줄을 따라 바꿔 **`Cargo.lock` 도 같은 PR 에 넣습니다.** `bun run check:version` 이 네 곳이 맞는지 봐 줍니다.
3.  **판 노트를 씁니다.** `docs/release-notes/v<버전>.md`. 이 파일이 없으면 Release 가 중간에 멈춥니다. 쓰는 사람 말로, 바뀐 점과 설치 방법만. 사람·회사 이름, AGENTS.md 의 Hard rules 가 피하라고 한 말은 쓰지 않습니다(`bun run check:public`).
4.  **`chore: version <버전>` PR 을 머지합니다.**
5.  **Actions › Release › Run workflow.** "Publish at once" 는 끕니다. 30분쯤 뒤 초안이 생깁니다. 초안은 주인만 봅니다.
6.  **초안의 파일로 두 기기에서 확인합니다.**
    - macOS: dmg 로 새로 깔기 · 앞 판에서 앱 안 "설치"로 올리기 · 녹음 30초 → 회의록이 나오는지
    - Windows: 설치 파일 실행 · 창이 뜨는지 · 녹음 30초 → 회의록이 나오는지
    - 파일 여섯 개가 다 있는지: dmg, `.app.tar.gz`, `.app.tar.gz.sig`, `-setup.exe`, `-setup.exe.sig`, `latest.json`
7.  **Publish 를 누릅니다.** 이때 태그 `v<버전>` 이 생기고, 이 판이 "최신"이 됩니다. 이미 깔린 서기는 이 순간부터 새 판을 압니다.
8.  **닿는지 확인합니다.**

        curl -sL https://github.com/lokim-dj/seogi/releases/latest/download/latest.json | grep '"version"'

    방금 낸 버전이 보여야 합니다. 404 면 공개된 판이 하나도 없다는 뜻입니다.

## 알아 둘 것

- 초안으로만 두면 업데이트가 아무에게도 닿지 않습니다. 써 보고 괜찮으면 7번까지 갑니다.
- 낸 판의 파일은 바꾸지 않습니다. 고칠 것이 있으면 버전을 올려 새로 냅니다.
- 업데이트 서명 키는 저장소 Secrets 와 주인의 컴퓨터에만 있습니다. **이 키를 잃으면 이미 깔린 서기는 다시는 스스로 업데이트하지 못합니다.** 컴퓨터 밖 한 곳에 더 보관합니다.
- 서명하지 않은 앱이라 처음 열 때 막힙니다. 판 노트의 설치 안내를 빼지 않습니다.
