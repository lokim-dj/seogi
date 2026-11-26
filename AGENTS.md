# AGENTS.md

Guidance for AI coding assistants working in this repository.

## What Seogi is

Seogi (서기) is a desktop meeting secretary for macOS and Windows, built with Tauri 2 (Rust backend, React/TypeScript frontend). It records a meeting (or takes a recording file), transcribes it in Korean with speakers separated, and writes minutes that lead with the action items. Recording and transcription run on the user's own computer.

The product's main objects are **recordings and their minutes**. Dictation (hotkey, speak, text typed at the cursor) is a secondary feature.

## Product principles (read first)

1. **Trust over fluency.** Nobody re-listens to a 2–3 h recording, so minutes must be trustworthy: never invent; every action item and decision cites its source timestamp and a short quote; weak support goes to "확인 필요", not stated as fact.
2. **The machine verifies so the human doesn't have to.** The pipeline self-checks (claim ↔ transcript span, low-confidence ASR spans, names vs vocabulary, lone or conflicting numbers/dates/amounts). Human review should take seconds: a short 확인 필요 list with 5–15 s audio snippets.
3. **Don't bother the user after stop.** No dialogs or questions once a recording ends; defaults decide; failures retry and degrade gracefully; one notification when done.
4. **Meetings are the home.** Recordings and minutes are the main objects; settings and dictation are secondary.
5. **Private and free by default.** Audio stays on the user's machine; paid/cloud AI only with the user's own key/login; personal vocabulary, calendar rules and keys never enter the repo.
6. **Korean first, plain words.** UI and minutes speak natural Korean from the user's side; no developer jargon in the UI.
7. **Accuracy is measured.** Whenever transcription, chunking, diarization or minutes prompts change, run the eval on practice recordings whose correct text is known (synthetic clips; the private real-sample set when available) and confirm nothing got worse — put the before/after numbers in the commit message.

## Hard rules

- **Seogi has nothing to do with any company.** Do not tie it to a workplace's chat, tracker or feedback channel; feedback goes to this repository's GitHub issues. In public text (README, store listings, release notes) describe it as a secretary that keeps the record, writes it up and tells you what you have to do — not in legal or dispute terms (avoid words like "evidence"/"증거").
- **No company, staff or customer names, emails or tokens in this repository.** It is public. Personal vocabulary, calendar rules and API keys live in per-user local files or the OS keychain. Examples in the repo must be generic.
- **Real recordings never enter the repository.** Only synthetic clips made by `eval/synthetic/make_clips.py` may be used in tests, and the generated audio is gitignored.
- **The original recording is the record.** Never re-encode, trim or delete it. Edits to a transcript or minutes never touch the original.
- **Free by default.** The default configuration uses only local models. Paid APIs are opt-in, with the user's own key.
- **Third-party code keeps its notice.** A file derived from another project keeps that project's copyright line in its header, and the project is listed in `THIRD_PARTY_NOTICES.md`.
- Push with the explicit URL form: `git push https://github.com/lokim-dj/seogi.git <ref>`.

## How work lands

One pull request, one commit on main. Seogi has one maintainer, so the flow stays light.

1. **Issue first only for a bigger change** (a feature, a behaviour change, anything on the "Who decides" list): open one with the task template — the problem, what is true when it is done, what is left out. A typo, wording or a small fix goes straight to a pull request. If you were given an issue number, use it.
2. **Branch from fresh `origin/main`**, named `fix/short-name` (or `fix/123-short-name` with an issue; the prefix is the commit prefix). When another session may be using this folder, work in a worktree under `.claude/worktrees/` instead of switching branches here.
3. **Only what was asked.** A problem you notice next door goes in its own pull request or issue, not in this branch.
4. **Check it yourself.** Run the tests that touch the change. A screen change is looked at in the preview page. A change to transcription, chunking, speakers or minutes prompts carries the eval numbers before and after (principle 7).
5. **One review.** Before the pull request is merged, an agent that did not write the code — another model when one is at hand — reviews the branch once with `.claude/agents/seogi-reviewer.md`. Fix what it confirms. If it still says "needs fixes" on something you cannot settle, ask the maintainer.
6. **Push through the hook.** `bun run setup:hooks` once per clone; the pre-push hook checks what is pushed against the maintainer's private lists, which CI cannot see. Never skip it (`--no-verify`).
7. **Pull request** from the template; the title is the commit line (`fix: …`); with an issue, the body ends with `Closes #123`. Check its title and body yourself (`bun scripts/check-public-text.ts --stdin`); the hook does not see them.
8. **Merge by squash when the checks are green** (`text`, `rust` and `android` in `.github/workflows/check.yml`; `gh pr merge --squash`). main is protected: no direct push, no getting around a check.
9. **Close the loop.** With an issue, comment on it: what changed, what turned out different, what is left. Remove the worktree and the branch.

## Who decides

The maintainer decides these. Bring options and a recommendation, then wait; silence is not a yes.

- Anything that touches an original recording, or the files and layout of a meeting folder that people already have on disk.
- Anything that sends audio, text or names off the computer, or moves a default from local and free to cloud or paid.
- What the app is allowed to do on the system: entitlements, capabilities, the permission prompts and their wording.
- The path to users: the workflows, the updater key and address, version numbers, publishing a release (see `docs/RELEASING.md`), the repository's settings and rules.
- Reversing a decision recorded in `docs/adr/`, or adding a dependency or bundled file whose license is not MIT, Apache-2.0 or BSD.

Everything else the agent decides, and writes the reason where the next reader will look: the commit body, or an ADR line marked `Decided by: agent`.

## Development commands

Prerequisites: Rust (stable), Bun, CMake. See [BUILD.md](BUILD.md).

```bash
bun install
bun run tauri dev                 # run the app (CMAKE_POLICY_VERSION_MINIMUM=3.5 if cmake complains)
bun run tauri build               # release build and installers
cd src-tauri && cargo test        # Rust tests
bun run lint                      # ESLint
bun run format                    # Prettier + cargo fmt
bun run check:translations        # every locale has every key
bun run check:public              # no private names, emails or tokens (private lists only on the maintainer's computer)
bun run check:version             # the version agrees in three files and has release notes
bun run check:deps                # known advisories in npm and Rust parts (needs cargo-audit)
bun run setup:hooks               # once per clone: the pre-push hook
```

On a Homebrew toolchain: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.bun/bin:$PATH"`.

## Layout

```
src-tauri/src/
  seogi/            Seogi's own backend: recordings, pipeline, minutes (see below)
  brand.rs          App name and links
  managers/         Engine-side managers: audio capture, models, transcription, history
  engine_supervisor/  Runs the speech engine (transcribe.cpp) in a child process
  audio_toolkit/    Decoding, VAD, long-form chunking, text cleanup
  commands/         Tauri commands for the dictation settings screens
  settings.rs       Dictation settings (tauri-plugin-store)
src/
  seogi/            Seogi's own frontend (the product UI)
  components/       Dictation settings screens and onboarding
  i18n/locales/     Translations; `ko` and `en` are the ones Seogi maintains
eval/               Evaluation scripts; `eval/synthetic/` builds test recordings
android/            The phone app (Kotlin, Compose): records, uploads to the user's Drive; see android/README.md
```

Seogi's backend exposes a command and event API to the frontend, so the UI can be replaced without touching the pipeline. Keep UI concerns out of `src-tauri/src/seogi/`.

Inside `src-tauri/src/seogi/`:

| File                                                                                 | What it holds                                                                                                                             |
| ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `app.rs`                                                                             | The secretary: the queue, retries, watched folders, the recorder's start and stop, notifications, the tray menu                           |
| `commands.rs`                                                                        | Every command the UI may call (`seogi_*`). With the two events in `app.rs` this is the whole UI boundary                                  |
| `store.rs`                                                                           | A meeting on disk: one folder each, `meeting.json`, `회의록.md`, `전사.txt`                                                               |
| `evidence.rs`                                                                        | SHA-256 and `원본정보.json`                                                                                                               |
| `recorder.rs`                                                                        | Microphone to a crash-safe WAV                                                                                                            |
| `watch.rs`                                                                           | Watched folders: which files have settled, and the ledger of what was already brought in                                                  |
| `pipeline.rs`, `chunks.rs`, `diarize.rs`, `audio_in.rs`, `vocab.rs`, `transcript.rs` | Decoding, speech detection, speakers, per-chunk transcription with vocabulary hints                                                       |
| `minutes.rs`, `templates.rs`, `render.rs`, `llm.rs`                                  | Minutes: extraction with quotes, machine checks, templates as data, the Markdown document, the summary model providers                    |
| `config.rs`                                                                          | Seogi's settings (`seogi.json` in the app's data folder)                                                                                  |
| `cli.rs`                                                                             | Headless runs for checking and for `eval/`                                                                                                |
| `calendar.rs`, `mac_calendar.rs`                                                     | The user's calendar from secret iCal addresses or macOS's Calendar app: repeating events, title rules, which event a recording belongs to |
| `banner.rs`                                                                          | The always-on-top banner window (a panel on macOS, so it never takes the keyboard)                                                        |
| `dictation.rs`, `mic_button.rs`                                                      | Dictation: on/off, the shortcut and model, keeping out of a meeting's way, tidying with the user's model; the floating microphone key     |
| `snippet.rs`, `secrets.rs`, `power.rs`                                               | A few seconds of a recording for checking by ear; API keys in the OS keychain; keeping the computer awake                                 |

The frontend in `src/seogi/`:

| Path                  | What it holds                                                                             |
| --------------------- | ----------------------------------------------------------------------------------------- |
| `App.tsx`, `store.ts` | Which screen shows, and the state the screens share (all of it from the backend)          |
| `screens/`            | Home (the record key and the meetings), Minutes, Recording, Onboarding, Settings          |
| `banner/`             | The small always-on-top window: a meeting is starting, minutes are ready                  |
| `mic/`                | The floating microphone key's window: press to dictate, drag to move, right-click to hide |
| `components/`, `lib/` | Shared pieces: the timestamp key, the player, formats                                     |
| `i18n/`               | Every text of these screens, `ko.ts` (the original) and `en.ts`                           |
| `seogi.css`           | The design tokens and the few classes every sheet repeats                                 |
| `preview/`            | Every screen in a plain browser on made-up data, without the backend                      |

The look is fixed by the chosen design ("A안 한 줄로 비운 화면"): one quiet column on a warm ink black that is the first impression (a light system gets the same layout on paper), small type, thin lines, red only for recording and for what needs a look; the record key and the meetings on the home, finding, importing and settings behind three icons; the minutes as tasks, then the story, then everything else folded under "자세히", with a thin playing line at the bottom; timestamp keys on every line, quotes in 「 」. No cards, pills, glows, emoji or heavy boxes. The tokens and shared classes are in `seogi.css`.

To look at a screen while changing it, run `bun run dev` and open `http://localhost:1420/src/seogi/preview/index.html?screen=home` (or `minutes`, `recording`, `onboarding`, `settings`, `banner-starting`, `banner-ready`). `node scripts/seogi-screens.mjs out/` photographs all of them.

To try things without touching your own meetings, point a development run at its own data folder: `SEOGI_DATA_DIR=/some/dir bun run tauri dev` (put a `seogi.json` with `library_dir` there), or use the headless `--seogi-library` runs.

## Speech engine facts worth knowing

- Every model runs through `transcribe-cpp` (GGUF models, Metal on Apple Silicon, Vulkan or CPU on Windows). The engine runs in a worker process: the app re-launches itself with a hidden flag, so a native crash never takes the app down.
- Models are downloaded into the shared Hugging Face cache and listed in `src-tauri/src/catalog/catalog.json`, which also carries per-language benchmark results.
- Whisper applies a vocabulary prompt only to the first 30-second window of a run. To reach every part of a long recording, the audio is cut into chunks of at most 30 seconds and each chunk is a separate run with the prompt. Conditioning on previous text stays off for long audio.
- Speaker diarization uses the Sortformer model (up to four speakers) through the same engine, in its own child process.

## Conventions

**Rust:** run `cargo fmt` and `cargo clippy` before committing. Handle errors explicitly; no `unwrap` on paths that can fail at runtime. Doc comments on public items.

**TypeScript/React:** strict TypeScript, no `any`. Functional components. Tailwind CSS. Path alias `@/` → `./src/`.

**Text:** all user-facing strings go through i18next. Seogi's own screens keep theirs in `src/seogi/i18n/ko.ts` and `en.ts` (the type makes a missing English key a build error); other locales fall back to English. No emoji as UI icons. UI text is set in Gmarket Sans (bundled, see `THIRD_PARTY_NOTICES.md`), falling back to Pretendard.

**Names in examples:** an invented name in a test, a fixture or a comment must not be a real one. `python3 eval/synthetic/check_names.py` compares the practice scripts against a private vocabulary when one is on the computer, and `bun run check:public` compares every file with the private lists (in `~/seogi/private`, or `SEOGI_PRIVATE_DIR`) without printing them.

**Errors the user must see** while not looking at the window go through `notices::raise` (`src-tauri/src/notices.rs`); Rust sends codes, the frontend maps them to text in `src/lib/notices.ts`.

**Security checks (all free):** CI's `text` job runs `bun run check:deps` (bun audit, cargo audit): a high or critical advisory fails unless `KNOWN` in `scripts/check-deps.ts` lists it with a reason why it cannot reach the installed app; upgrade instead of listing whenever you can. A Rust advisory without a CVSS 3 score (many RustSec ones) only warns, so read the warnings too. CodeQL (`.github/workflows/codeql.yml`) scans TypeScript and Rust; Dependabot (`.github/dependabot.yml`) opens pull requests only for security fixes (for npm it sees only the packages named in `package.json`) and for the actions' pins. Every `uses:` in a workflow names a commit SHA with its tag in a comment (`@<sha> # v5.1.0`), never a bare tag.

**Commits:** conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Say why, not what.

## Debug mode

`Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows) toggles the debug settings section.
