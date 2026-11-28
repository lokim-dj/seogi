# The shared Drive folder

The phone and the computer share one folder in the user's Google Drive. Both
reach it with the `drive.file` scope only, so each sees just what a Seogi app
made (issue #67).

## Finding it

The folder is found by its marker, never by its name: `appProperties`
`seogi=recordings-root`, a folder, not in the trash; the oldest one when there
are several. Its name is `서기`. A folder still named `서기 폰 녹음` (its name
while only the phone wrote to it) is renamed to `서기` by the computer; a name the user chose is
left alone.

## What is in it

```
서기/
  2026-11-26 1842 주간 회의.m4a     a phone recording not brought in yet
  2026-11-26 1842 주간 회의.json    its JSON (written by the phone)
  2026-11-27 1000 견적 회의/        one folder per meeting with minutes
    녹음.wav                        the original recording, as it is
    전사.txt                        the transcript, as transcribed
    회의록.md                        the minutes document
    회의.json                       what the apps read (below)
```

- A meeting's folder is named like the meeting's folder on the computer and
  carries the marker `appProperties` `seogi_meeting=<meeting id>`. It is
  renamed when the computer's folder is.
- For a meeting recorded on the phone, the phone's recording and its JSON are
  moved (the same Drive files, not copies) into the meeting's folder once its
  minutes are ready. No recording is uploaded for it.
- For a meeting recorded on the computer, the original recording is uploaded
  as it is when "녹음 파일도 올리기" is on and the account has at least 1 GB
  left beside it; otherwise the text goes up without it and the recording
  follows when there is room.
- Only the computer writes `전사.txt`, `회의록.md` and `회의.json`. Changes made
  on either side will go to `고친것-컴퓨터.json` and `고친것-폰.json`, one file
  per device (a later stage).
- "드라이브에서 빼기" moves a meeting's folder to Drive's trash. Nothing is
  deleted for good.

## How the phone reads it

The phone lists the folders in `서기` that carry `seogi_meeting`, then the
files in them (one search for many folders). A folder without `회의.json` is
not ready yet and is left out. Each `회의.json` is read when it is new or its
Drive `version` changed, and kept on the phone, so the minutes open without a
network. A meeting whose folder went to the trash leaves the phone's list.

- The recording is the file `audio.file` names (else the only file that is
  not `.json`, `.txt` or `.md`). It is downloaded only when the user presses
  play, into the app's cache; a download cut off goes on from where it
  stopped. The file in Drive is only read.
- A meeting recorded on the phone is matched with the phone's own recording
  by `appProperties seogi_rec` on the moved files (by the file name for files
  uploaded before that was written), and shows as one line; it plays the
  phone's own copy, nothing is downloaded.

## `회의.json`

Written last, so a phone may take the meeting as ready once it is there.
Fields are only ever added; `format` changes only if one changes meaning.

```json
{
  "format": 1,
  "app": "seogi-desktop",
  "app_version": "0.1.18",
  "id": "20261127-100000-1a2b",
  "title": "견적 회의",
  "started_at": "2026-11-27T10:00:00+09:00",
  "started_at_ms": 1795741200000,
  "duration_ms": 3540000,
  "origin": "recorded",
  "audio": { "file": "녹음.wav", "sha256": "…", "in_folder": true },
  "kind": "회의",
  "summary": "한 줄 요약",
  "tasks": [
    {
      "id": "3f9a0c12b7de",
      "text": "견적서 보내기",
      "owner": "가",
      "due": "금요일",
      "done": false,
      "needs_check": false,
      "added": false,
      "sources": [{ "ms": 61000, "quote": "견적서는 금요일까지" }]
    }
  ],
  "story": [{ "title": "…", "text": "…", "sources": [] }],
  "sections": [
    { "key": "actions", "title": "할 일", "kind": "tasks", "entries": [] }
  ],
  "marks_ms": [120000],
  "speakers": [{ "id": 1, "name": "화자 1" }],
  "lines": [{ "start_ms": 0, "end_ms": 4200, "speaker": 1, "text": "…" }]
}
```

- `origin`: `recorded`, `imported`, `watched` or `phone`.
- `audio.in_folder`: whether the recording is in the meeting's folder.
- `tasks`: every entry of the task sections, in order; `sections` holds every
  section (`kind`: `tasks`, `items`, `quotes`) with its entries.
- Every entry's `id` stays the same for as long as the entry lives, whatever
  is changed in it. Meetings written before ids existed get them when first
  uploaded.
- Times (`ms`, `start_ms`, `marks_ms`) are milliseconds into the recording.
- `lines` are the transcript as the user reads it (with their fixes).
