// The meetings' speech model as the screens describe it: what it is good
// at, how big its download is and how long that takes.

/** The speech models the settings can say something measured about. */
export type SpeechAbout = "qwen" | "turbo" | "funasr";

/** Which measured description fits a model id, if any. The numbers behind
 *  them come from one comparison on the same recording (see `config.rs`,
 *  `DEFAULT_SPEECH_MODEL`); other models get none. */
export function speechAbout(id: string): SpeechAbout | null {
  const file = id.split("/").pop() ?? id;
  if (/^Qwen3-ASR-1\.7B/i.test(file)) return "qwen";
  if (/^whisper-large-v3-turbo/i.test(file)) return "turbo";
  if (/^Fun-ASR-MLT-Nano/i.test(file)) return "funasr";
  return null;
}

/** A download's size in gigabytes, one decimal: 2185030624 → "2.2". */
export function gigabytes(bytes: number): string {
  return (bytes / 1e9).toFixed(1);
}

/** The line speed the estimate before a download assumes, in bits per
 *  second: an ordinary home line. */
export const ASSUMED_BITS_PER_SECOND = 100e6;

/** Whole minutes a download of `bytes` takes on an ordinary line, at least
 *  one. */
export function minutesToDownload(bytes: number): number {
  const seconds = (bytes * 8) / ASSUMED_BITS_PER_SECOND;
  return Math.max(1, Math.ceil(seconds / 60));
}

/** Whole minutes left of a download, at least one; `null` while the speed
 *  is not measured yet. */
export function minutesLeft(
  downloaded: number,
  total: number,
  bytesPerSecond: number | null | undefined,
): number | null {
  if (!bytesPerSecond || bytesPerSecond <= 0 || total <= 0) return null;
  const seconds = Math.max(0, total - downloaded) / bytesPerSecond;
  return Math.max(1, Math.ceil(seconds / 60));
}

/** Where the screens keep the models' downloads. */
export interface Downloads {
  /** Progress by model id, 0 to 100, while a download runs. */
  downloads: Record<string, number>;
  /** Whole minutes left, once a download's speed is known. */
  downloadMinutesLeft: Record<string, number>;
  /** Models whose last download failed, until one is tried again. */
  downloadFailed: string[];
}

/** A download reported progress: it runs, so it has not failed. */
export function downloadProgressed(
  state: Downloads,
  progress: {
    model_id: string;
    percentage: number;
    downloaded: number;
    total: number;
    bytes_per_sec: number | null;
  },
): Downloads {
  const { model_id, percentage, downloaded, total, bytes_per_sec } = progress;
  const left = minutesLeft(downloaded, total, bytes_per_sec);
  const downloadMinutesLeft = { ...state.downloadMinutesLeft };
  if (left === null) delete downloadMinutesLeft[model_id];
  else downloadMinutesLeft[model_id] = left;
  return {
    downloads: { ...state.downloads, [model_id]: percentage },
    downloadMinutesLeft,
    downloadFailed: state.downloadFailed.filter((id) => id !== model_id),
  };
}

/** A download ended, well or not: its progress goes, and a failure is kept
 *  so the screens can offer to try again. */
export function downloadEnded(
  state: Downloads,
  modelId: string,
  failed: boolean,
): Downloads {
  const downloads = { ...state.downloads };
  delete downloads[modelId];
  const downloadMinutesLeft = { ...state.downloadMinutesLeft };
  delete downloadMinutesLeft[modelId];
  const others = state.downloadFailed.filter((id) => id !== modelId);
  return {
    downloads,
    downloadMinutesLeft,
    downloadFailed: failed ? [...others, modelId] : others,
  };
}
