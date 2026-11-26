// bun src/seogi/lib/speech.test.ts
//
// What the screens say about the meetings' speech model: the measured
// description only for the models that were measured, and the download's
// size and time in whole, honest numbers.

import assert from "node:assert/strict";
import {
  downloadEnded,
  downloadProgressed,
  gigabytes,
  minutesLeft,
  minutesToDownload,
  speechAbout,
} from "./speech";

const QWEN = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf";
const TURBO =
  "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf";
const FUNASR =
  "handy-computer/Fun-ASR-MLT-Nano-2512-gguf/Fun-ASR-MLT-Nano-2512-Q8_0.gguf";

// The three measured models, in any quantization; nothing for the others.
assert.equal(speechAbout(QWEN), "qwen");
assert.equal(speechAbout("Qwen3-ASR-1.7B-F16.gguf"), "qwen");
assert.equal(speechAbout(TURBO), "turbo");
assert.equal(speechAbout(FUNASR), "funasr");
assert.equal(
  speechAbout("handy-computer/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf"),
  null,
);
assert.equal(
  speechAbout(
    "handy-computer/whisper-large-v3-gguf/whisper-large-v3-Q8_0.gguf",
  ),
  null,
);
assert.equal(speechAbout(""), null);

// Qwen3 ASR 1.7B (Q8_0) is 2,185,030,624 bytes: 2.2 GB, three minutes on
// a 100 Mbps line. The size the backend sends is whole MiB, so a little
// less; the words do not change.
assert.equal(gigabytes(2185030624), "2.2");
assert.equal(gigabytes(2083 * 1024 * 1024), "2.2");
assert.equal(minutesToDownload(2185030624), 3);
assert.equal(minutesToDownload(1), 1);

// Minutes left come from the measured speed, and never say zero.
assert.equal(minutesLeft(0, 2185030624, null), null);
assert.equal(minutesLeft(0, 2185030624, 0), null);
assert.equal(minutesLeft(0, 0, 1e6), null);
assert.equal(minutesLeft(0, 600e6, 5e6), 2);
assert.equal(minutesLeft(599e6, 600e6, 5e6), 1);
assert.equal(minutesLeft(700e6, 600e6, 5e6), 1);

// A download runs, fails, is tried again and ends: the screens see its
// progress, then a failure they can offer again, then nothing.
const none = { downloads: {}, downloadMinutesLeft: {}, downloadFailed: [] };
const running = downloadProgressed(none, {
  model_id: QWEN,
  percentage: 40,
  downloaded: 400e6,
  total: 1000e6,
  bytes_per_sec: 5e6,
});
assert.deepEqual(running.downloads, { [QWEN]: 40 });
assert.deepEqual(running.downloadMinutesLeft, { [QWEN]: 2 });
const failed = downloadEnded(running, QWEN, true);
assert.deepEqual(failed, {
  downloads: {},
  downloadMinutesLeft: {},
  downloadFailed: [QWEN],
});
const again = downloadProgressed(failed, {
  model_id: QWEN,
  percentage: 1,
  downloaded: 0,
  total: 1000e6,
  bytes_per_sec: null,
});
assert.deepEqual(again.downloadFailed, []);
assert.deepEqual(again.downloadMinutesLeft, {});
assert.deepEqual(downloadEnded(again, QWEN, false), none);
// Another model's download leaves this one's failure where it is.
assert.deepEqual(downloadEnded(failed, TURBO, false).downloadFailed, [QWEN]);

console.log("speech: ok");
