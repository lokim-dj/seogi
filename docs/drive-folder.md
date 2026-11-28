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
  on either side go to `고친것-컴퓨터.json` and `고친것-폰.json`, one file per
  device, each written only by its own device (below).
- "드라이브에서 빼기" moves a meeting's folder to Drive's trash. Nothing is
  deleted for good.
- Deleting a meeting (decision 5 of #67) moves its folder to Drive's trash,
  with everything in it, and keeps the computer's copy. On the computer the
  meeting itself goes to the system's trash as before, and bringing it back
  puts it in Drive again. On the phone the folder goes to Drive's trash (and
  the phone's own recording of it is deleted); the computer finds the folder
  in the trash (one search for all meetings, or when it next goes to update
  it) and takes the meeting out of Drive as "드라이브에서 빼기" does, so it is
  not put back. "드라이브에 다시 올리기" (or undoing the delete on the computer)
  takes the same folder out of Drive's trash, with the recording still in it,
  so nothing goes up twice; a folder deleted for good is made again.

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

## Changes made on either side

A task ticked, a line fixed or a speaker named can be changed on the computer
and on the phone. Each device keeps its latest change of each thing, with the
time it was made, in its own file in the meeting's folder:

```json
{
  "format": 1,
  "device": "phone",
  "meeting": "20261127-100000-1a2b",
  "edits": [
    { "key": "done:3f9a0c12b7de", "value": true, "at_ms": 1795741800000 },
    {
      "key": "line:61000",
      "value": "견적서는 금요일까지",
      "at_ms": 1795741900000
    },
    { "key": "speaker:1", "value": "가", "at_ms": 1795742000000 }
  ]
}
```

- Keys: `done:<task id>` (`true`/`false`), `line:<start_ms>` (the line as the
  user reads it), `speaker:<id>` (the speaker's name).
- For each key changed on either side, the later change stands; at the very
  same `at_ms`, the computer's. Both sides decide it the same way.
- When a meeting is transcribed or its minutes written again, the changes so
  far were for what was replaced: the computer sets them aside and writes the
  moment in `회의.json` as `edits_since`; both sides leave changes made before
  it alone.
- A folder may hold more than one `고친것-폰.json` (two phones, or a reply
  lost while one was made): the computer reads them as one, the latest change
  of each key, and the phone writes to the one there before making one.
- The change that did not stand, when it says something else, is shown on
  both sides as a faint line under "다르게 고친 것"; pressing it takes that
  change instead, as made now. Nothing is dropped without a trace.
- The computer reads every `고친것-폰.json` with one search, when its Drive
  `version` changed, makes the phone's standing changes to the meeting (a line
  keeps the words heard beside the user's; the recording is never touched)
  and puts `회의.json` and `회의록.md` up again. The phone reads
  `고친것-컴퓨터.json` with the meeting's other files and shows the minutes
  with every standing change, also before `회의.json` has them.

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
  "edits_since": 0,
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
- `lines` are the transcript as the user reads it (with their fixes). A line
  may also have `polished`: its words tidied for reading (spacing,
  punctuation, fillers), when the user turned "받아쓴 글 다듬기" on. `text`
  stays the words as said; a line the user fixed has no `polished`. Older
  files have none, and an app that does not know it reads `text`.
