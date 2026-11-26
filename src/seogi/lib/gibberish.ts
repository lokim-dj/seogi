// A line the speech model made up in a Korean meeting: mostly letters of
// another language, or a broken character. It is shown faintly and the
// minutes do not rest on it; the transcript itself keeps it as heard. The
// same rule as `facts::is_gibberish` (src-tauri/src/seogi/facts.rs);
// both are tested against gibberish.cases.json, keep the two alike.

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/u;
const LETTER = /\p{L}/u;
const LATIN = /[A-Za-z]/;

/** Whether `text`, heard in a Korean meeting, is the speech model's noise. */
export function isGibberish(text: string): boolean {
  if (text.includes("�")) return true;
  let letters = 0;
  let hangul = 0;
  let latin = 0;
  let foreign = 0;
  for (const c of text) {
    if (!LETTER.test(c)) continue;
    letters += 1;
    if (HANGUL.test(c)) hangul += 1;
    else if (LATIN.test(c)) latin += 1;
    else foreign += 1;
  }
  if (letters < 4) return false;
  const share = hangul / letters;
  if (share < 0.5 && foreign >= 2) return true;
  const lowerWord = /(^|[^A-Za-z])[a-z][a-z]{2,}/.test(text);
  return share < 0.3 && latin >= 4 && lowerWord;
}
