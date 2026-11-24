// Hearing a moment of a recording. A short piece is cut from the original
// on demand by the backend and played; nothing is stored.

import { commands } from "@/bindings";

let playing: HTMLAudioElement | null = null;
let stopTimer: number | undefined;

export function stopListening(): void {
  window.clearTimeout(stopTimer);
  if (playing) {
    playing.pause();
    playing = null;
  }
}

/**
 * Plays `seconds` of the meeting's recording from `fromMs`. Resolves to
 * false when that piece could not be had.
 */
export async function listenTo(
  id: string,
  fromMs: number,
  seconds: number,
): Promise<boolean> {
  stopListening();
  const start = Math.max(0, fromMs);
  const result = await commands.seogiAudioSnippet(
    id,
    start,
    start + seconds * 1000,
  );
  if (result.status !== "ok") return false;
  const audio = new Audio(`data:audio/wav;base64,${result.data}`);
  playing = audio;
  audio.addEventListener("ended", () => {
    if (playing === audio) playing = null;
  });
  try {
    await audio.play();
    return true;
  } catch {
    return false;
  }
}
