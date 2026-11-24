// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

/**
 * The CJK subset font, for Handy's own CJK strings only (language names,
 * Demo's lines): text outside the subset would mix fonts mid-line.
 */
const CJK =
  /[\p{Script=Han}\p{Script=Hangul}\p{Script=Hiragana}\p{Script=Katakana}]/u;

export const cjkSubsetFont = (text: string) =>
  CJK.test(text) ? "font-cjk-subset" : undefined;
