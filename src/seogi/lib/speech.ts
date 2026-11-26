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
