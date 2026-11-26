// A line the speech model made up rather than words someone said. Strong
// signs only, so a real English sentence always stays: a broken character,
// a line it is known to make up ("Thank you for watching"), one word over
// and over, and, in a meeting that is otherwise Korean, letters of a script
// that is neither Korean nor Latin or a short scrap of lowercase Latin that
// is not a word people say on its own. It is shown faintly and the minutes
// do not rest on it; the transcript keeps it as heard. The same rule as
// `facts::is_gibberish` and `facts::korean_meeting`
// (src-tauri/src/seogi/facts.rs); both are tested against
// gibberish.cases.json, keep the two alike.

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/u;
const LETTER = /\p{L}/u;
const LATIN = /[A-Za-z]/;

const NOISE_PHRASES = [
  "thankyouforwatching",
  "thanksforwatching",
  "pleasesubscribe",
  "subscribetomychannel",
  "likeandsubscribe",
  "subtitlesby",
  "amaraorg",
  "transcribedby",
  "시청해주셔서감사합니다",
  "구독과좋아요",
  "mbc뉴스",
];

const SAID_IN_ENGLISH = new Set([
  "ok",
  "okay",
  "yes",
  "no",
  "sure",
  "thanks",
  "thank",
  "you",
  "hello",
  "hi",
  "bye",
  "sorry",
  "good",
  "nice",
  "right",
  "wow",
  "oh",
]);

/** Whether a meeting is Korean by what was heard: most of its letters. */
export function koreanMeeting(lines: { text: string }[]): boolean {
  let letters = 0;
  let hangul = 0;
  for (const line of lines) {
    for (const c of line.text) {
      if (!LETTER.test(c)) continue;
      letters += 1;
      if (HANGUL.test(c)) hangul += 1;
    }
  }
  return letters > 0 && hangul >= 0.8 * letters;
}

/** Whether `text` is the speech model's noise; `korean` says the meeting is
 *  otherwise Korean. */
export function isGibberish(text: string, korean: boolean): boolean {
  if (text.includes("�")) return true;
  const squeezed = [...text]
    .filter((c) => /[\p{L}\p{N}]/u.test(c))
    .join("")
    .toLowerCase();
  if (NOISE_PHRASES.some((phrase) => squeezed.includes(phrase))) return true;
  const words = text
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
    .map((w) => w.toLowerCase());
  if (words.length >= 6 && words.every((w) => w === words[0])) return true;
  if (!korean) return false;
  let letters = 0;
  let hangul = 0;
  let foreign = 0;
  for (const c of text) {
    if (!LETTER.test(c)) continue;
    letters += 1;
    if (HANGUL.test(c)) hangul += 1;
    else if (!LATIN.test(c)) foreign += 1;
  }
  if (letters < 4) return false;
  const share = hangul / letters;
  if (share < 0.5 && foreign >= 2) return true;
  const latin = text.split(/[^A-Za-z]+/).filter(Boolean);
  const scrap =
    latin.length > 0 &&
    latin.length <= 2 &&
    latin.reduce((n, w) => n + w.length, 0) <= 16 &&
    latin.every((w) => /^[a-z]+$/.test(w)) &&
    latin.some((w) => w.length >= 3 && !SAID_IN_ENGLISH.has(w));
  return share < 0.3 && scrap;
}
