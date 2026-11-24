// Asking for the microphone before the first recording, so the system's
// question comes at the moment the user pressed record and the recording
// starts once they have answered, instead of failing or recording silence.

import { platform } from "@tauri-apps/plugin-os";
import {
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";

/** How long to wait for the user to answer the system's question. */
const ANSWER_WITHIN_MS = 90000;

/**
 * True when the microphone may be used. On macOS this shows the system's
 * question the first time and waits for the answer; elsewhere the system
 * asks on its own when recording starts.
 */
export async function microphoneAllowed(): Promise<boolean> {
  if (platform() !== "macos") return true;
  try {
    if (await checkMicrophonePermission()) return true;
    await requestMicrophonePermission();
    const until = Date.now() + ANSWER_WITHIN_MS;
    while (Date.now() < until) {
      if (await checkMicrophonePermission()) return true;
      await new Promise((resolve) => window.setTimeout(resolve, 400));
    }
    return false;
  } catch {
    // The check itself is not available: let the recorder find out.
    return true;
  }
}
