// Listening to a whole recording: play, pause, jump to a moment, speed.
//
// The webview plays the original file itself where it can (wav, m4a, mp3).
// For a format it cannot play, jumping to a moment still works: the backend
// cuts that piece from the original and it is played on its own.

import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listenTo, stopListening } from "./listen";

const RATES = [1, 1.25, 1.5, 2];

export interface Player {
  playing: boolean;
  timeMs: number;
  durationMs: number;
  rate: number;
  /** False when the webview cannot play this file through. */
  canPlayThrough: boolean;
  toggle: () => void;
  /** Plays from `ms`; for `forMs` only, when given. */
  seek: (ms: number, forMs?: number) => void;
  nextRate: () => void;
}

export function usePlayer(
  id: string,
  path: string | null,
  lengthMs: number,
): Player {
  const audio = useRef<HTMLAudioElement | null>(null);
  const stopAt = useRef<number | undefined>(undefined);
  const [playing, setPlaying] = useState(false);
  const [timeMs, setTimeMs] = useState(0);
  const [durationMs, setDurationMs] = useState(lengthMs);
  const [rate, setRate] = useState(1);
  const [canPlayThrough, setCanPlayThrough] = useState(true);

  useEffect(() => {
    setDurationMs(lengthMs);
  }, [lengthMs]);

  useEffect(() => {
    if (!path) return;
    const element = new Audio();
    element.preload = "metadata";
    element.src = convertFileSrc(path);
    audio.current = element;
    setCanPlayThrough(true);
    setTimeMs(0);

    const onTime = () => setTimeMs(element.currentTime * 1000);
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    const onError = () => setCanPlayThrough(false);
    const onMeta = () => {
      if (Number.isFinite(element.duration) && element.duration > 0) {
        setDurationMs(element.duration * 1000);
      }
    };
    element.addEventListener("timeupdate", onTime);
    element.addEventListener("play", onPlay);
    element.addEventListener("pause", onPause);
    element.addEventListener("ended", onPause);
    element.addEventListener("error", onError);
    element.addEventListener("loadedmetadata", onMeta);
    return () => {
      window.clearTimeout(stopAt.current);
      element.pause();
      element.removeAttribute("src");
      element.load();
      audio.current = null;
      stopListening();
    };
  }, [path]);

  const toggle = useCallback(() => {
    const element = audio.current;
    if (!element || !canPlayThrough) return;
    window.clearTimeout(stopAt.current);
    if (element.paused)
      void element.play().catch(() => setCanPlayThrough(false));
    else element.pause();
  }, [canPlayThrough]);

  const seek = useCallback(
    (ms: number, forMs?: number) => {
      const from = Math.max(0, ms);
      const element = audio.current;
      window.clearTimeout(stopAt.current);
      if (!element || !canPlayThrough) {
        setTimeMs(from);
        void listenTo(id, from, (forMs ?? 15000) / 1000);
        return;
      }
      element.currentTime = from / 1000;
      void element.play().catch(() => {
        setCanPlayThrough(false);
        void listenTo(id, from, (forMs ?? 15000) / 1000);
      });
      if (forMs) {
        stopAt.current = window.setTimeout(
          () => element.pause(),
          forMs / element.playbackRate,
        );
      }
    },
    [id, canPlayThrough],
  );

  const nextRate = useCallback(() => {
    setRate((current) => {
      const next = RATES[(RATES.indexOf(current) + 1) % RATES.length];
      if (audio.current) audio.current.playbackRate = next;
      return next;
    });
  }, []);

  return {
    playing,
    timeMs,
    durationMs,
    rate,
    canPlayThrough,
    toggle,
    seek,
    nextRate,
  };
}

/** A file inside a folder, joined the way that folder's path is written. */
export function joinPath(folder: string, file: string): string {
  const separator = folder.includes("\\") && !folder.includes("/") ? "\\" : "/";
  return `${folder}${separator}${file}`;
}
