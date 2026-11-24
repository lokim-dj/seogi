// Times, dates and lengths as the screens write them.

const two = (n: number) => String(n).padStart(2, "0");

/** A clock time of day: 13:48. */
export function hm(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** A clock time of day with seconds: 14:00:07. */
export function hms(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

/** A place in a recording: 08:14, or 1:08:30 past the hour. */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor(total / 60) % 60;
  const s = total % 60;
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${two(m)}:${two(s)}`;
}

export function sameDay(a: number, b: number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return (
    x.getFullYear() === y.getFullYear() &&
    x.getMonth() === y.getMonth() &&
    x.getDate() === y.getDate()
  );
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** How a day stands to today: 0 today, 1 yesterday, more for earlier. */
export function daysAgo(ms: number, now: number): number {
  const start = (t: number) => {
    const d = new Date(t);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  return Math.round((start(now) - start(ms)) / DAY_MS);
}

/** 11월 24일 화요일 */
export function longDay(ms: number, lang: string): string {
  return new Intl.DateTimeFormat(lang, {
    month: "long",
    day: "numeric",
    weekday: "long",
  }).format(new Date(ms));
}

/** 2026년 11월 24일 (화) */
export function fullDay(ms: number, lang: string): string {
  const d = new Date(ms);
  const weekday = new Intl.DateTimeFormat(lang, { weekday: "short" }).format(d);
  const date = new Intl.DateTimeFormat(lang, {
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(d);
  return `${date} (${weekday})`;
}

/** 2026-11-24 */
export function isoDay(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
}

/** The last part of a path, whichever way its slashes lean. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** A path as people say it: Google Drive (whose) › 내 드라이브 › 녹음함. */
export function friendlyPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  const drive = parts.findIndex((part) => part.startsWith("GoogleDrive-"));
  if (drive >= 0) {
    const account = parts[drive].slice("GoogleDrive-".length);
    return [`Google Drive (${account})`, ...parts.slice(drive + 1)].join(" › ");
  }
  const home = parts.findIndex((part) => part === "Users");
  if (home >= 0 && parts.length > home + 2) {
    return parts.slice(home + 2).join(" › ");
  }
  return parts.join(" › ");
}

/** A shortcut as the keyboard shows it: Control + Option + R. */
export function keysLabel(shortcut: string, mac: boolean): string {
  return shortcut
    .split("+")
    .map((key) => key.trim())
    .filter(Boolean)
    .map((key) => {
      const lower = key.toLowerCase();
      if (lower === "alt" || lower === "option") return mac ? "Option" : "Alt";
      if (lower === "ctrl" || lower === "control")
        return mac ? "Control" : "Ctrl";
      if (lower === "cmd" || lower === "super" || lower === "meta")
        return mac ? "Command" : "Win";
      if (lower === "shift") return "Shift";
      return key.length === 1 ? key.toUpperCase() : key;
    })
    .join(" + ");
}

/** A model as people know it: whisper-large-v3-turbo, without the file's
 *  folder, ending and quantization tag. */
export function shortModel(id: string): string {
  const file = id.split("/").pop() ?? id;
  return file.replace(/\.gguf$/i, "").replace(/-(Q\d\w*|F16|F32|BF16)$/i, "");
}

/** A shortcut as the Mac menus write it, ⌃⌥R; on Windows, Ctrl+Alt+R. */
export function keysShort(shortcut: string, mac: boolean): string {
  const keys = shortcut
    .split("+")
    .map((key) => key.trim())
    .filter(Boolean);
  if (!mac) return keys.join("+");
  const symbols: Record<string, string> = {
    ctrl: "⌃",
    control: "⌃",
    alt: "⌥",
    option: "⌥",
    shift: "⇧",
    cmd: "⌘",
    command: "⌘",
    super: "⌘",
    meta: "⌘",
  };
  return keys
    .map(
      (key) =>
        symbols[key.toLowerCase()] ??
        (key.length === 1 ? key.toUpperCase() : key),
    )
    .join("");
}
