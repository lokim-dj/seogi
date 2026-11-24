// Keeps private things out of this public repository.
//
//   bun scripts/check-public-text.ts                  every tracked file
//   bun scripts/check-public-text.ts --messages A..B  the commit messages in a range
//
// 1. Shapes that never belong here: real-looking email addresses, phone
//    numbers, access tokens, private keys. Checked everywhere, also in CI.
// 2. On the maintainer's computer only, from the private folder
//    (SEOGI_PRIVATE_DIR, by default ~/seogi/private; never in this repository):
//    - public-check/names.txt: words that must not appear anywhere (one per
//      line, any case), such as a workplace's name and its tools;
//    - the people, sites and companies in 용어집/vocabulary.json (the same
//      file eval/synthetic/check_names.py reads), names of three letters or
//      more; public-check/not-names.txt lists the ones that are ordinary words;
//    - public-check/prose-words.txt: words public prose does not use
//      (README, docs, screen text, issue and pull request templates, commit
//      messages). See AGENTS.md, Hard rules.
//    A private word is never printed: a finding names the file and line only.
//
// The private list cannot run in CI, so the pre-push hook (.githooks/pre-push)
// runs this script on the maintainer's computer before anything leaves it.
import { execFileSync } from "child_process";
import fs from "fs";
import os from "os";
import path from "path";

// Files that are not ours to check: lock files, vendored code, upstream
// translations Seogi does not maintain, the list of other projects' notices.
const SKIP =
  /^(bun\.lock|src-tauri\/Cargo\.lock|src-tauri\/vendor\/|src\/i18n\/locales\/(?!ko\/|en\/)|THIRD_PARTY_NOTICES\.md)/;
// Prose people read: what the prose word list applies to.
const PROSE =
  /^(README\.md|GLOSSARY\.md|BUILD\.md|docs\/.*\.md|src\/seogi\/i18n\/.*\.ts|\.github\/.*\.md|\.github\/ISSUE_TEMPLATE\/.*)$/;

const SHAPES: [string, RegExp][] = [
  [
    "email address",
    /[A-Za-z0-9._%+-]+@(?!users\.noreply\.github\.com)[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g,
  ],
  [
    "phone number",
    /(?<!\d)(01[016789]-\d{3,4}-\d{4}|\+82[- ]?10[- ]?\d{4}[- ]?\d{4})(?!\d)/g,
  ],
  [
    "access token",
    /\b(xox[abprs]-[A-Za-z0-9-]{10,}|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|hf_[A-Za-z0-9]{30,})/g,
  ],
  ["private key", /-----BEGIN [A-Z ]*PRIVATE KEY-----/g],
];
// Reserved made-up domains (RFC 2606), keys spelled as fakes in tests, and
// image names such as icon@2x.png.
const ALLOWED_SHAPE =
  /\.(example|test|invalid)\b|example\.(com|org|net)|secret-key|@\dx\./;

// ---- the private folder --------------------------------------------------

const privateDir =
  process.env.SEOGI_PRIVATE_DIR || path.join(os.homedir(), "seogi", "private");

function listFile(file: string): string[] {
  const full = path.join(privateDir, file);
  if (!fs.existsSync(full)) return [];
  return fs
    .readFileSync(full, "utf8")
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"));
}

const NAME_CATEGORIES = new Set([
  "직원",
  "고객",
  "현장",
  "업체",
  "staff",
  "customer",
  "site",
  "vendor",
  "person",
]);

/** Names of people, sites and companies from the private vocabulary. */
function vocabularyNames(): string[] {
  const file = path.join(privateDir, "용어집", "vocabulary.json");
  if (!fs.existsSync(file)) return [];
  type Entry = { term?: string; category?: string; aliases?: string[] };
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  const entries: Entry[] = Array.isArray(data) ? data : (data.entries ?? []);
  const names = new Set<string>();
  const ordinary = new Set<string>(listFile("public-check/not-names.txt"));
  for (const e of entries) {
    const forms = [e.term ?? "", ...(e.aliases ?? [])]
      .map((f) => f.trim())
      .filter(Boolean);
    if (NAME_CATEGORIES.has(e.category ?? ""))
      forms.forEach((f) => names.add(f));
    else forms.forEach((f) => ordinary.add(f));
  }
  // Korean names of three letters or more. Shorter ones sit inside ordinary
  // words; nicknames in Latin letters are ordinary English words (a Latin name
  // that matters belongs in names.txt).
  return [...names].filter(
    (n) => !ordinary.has(n) && /[가-힣]/.test(n) && [...n].length >= 3,
  );
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
/**
 * One expression for a list. A Latin word must stand alone; a Korean word
 * must start a word (particles may follow it), so a name does not match the
 * middle of an ordinary word.
 */
function matcher(words: string[], flags: string): RegExp | null {
  if (!words.length) return null;
  const parts = [...new Set(words)]
    .sort((a, b) => b.length - a.length)
    .map((w) =>
      /^[A-Za-z0-9 ._-]+$/.test(w)
        ? `(?<![A-Za-z0-9])${escape(w)}(?![A-Za-z0-9])`
        : `(?<![가-힣])${escape(w)}`,
    );
  return new RegExp(parts.join("|"), flags);
}

const handNames = listFile("public-check/names.txt");
const vocabNames = vocabularyNames();
const proseWords = listFile("public-check/prose-words.txt");
const privateNames = [matcher(handNames, "i"), matcher(vocabNames, "")].filter(
  (r): r is RegExp => r !== null,
);
const proseMatcher = matcher(proseWords, "i");

// ---- checking ------------------------------------------------------------

const found: string[] = [];

function checkText(where: string, text: string, prose: boolean) {
  text.split("\n").forEach((line, i) => {
    const at = `${where}:${i + 1}`;
    for (const [kind, re] of SHAPES)
      for (const m of line.matchAll(re))
        if (!ALLOWED_SHAPE.test(m[0])) found.push(`${at} ${kind}: ${m[0]}`);
    if (privateNames.some((re) => re.test(line)))
      found.push(`${at} a name from the private list`);
    if (prose && proseMatcher?.test(line))
      found.push(`${at} a word public prose does not use`);
  });
}

const git = (...args: string[]) =>
  execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 28 });

const range = process.argv.includes("--messages")
  ? process.argv[process.argv.indexOf("--messages") + 1]
  : undefined;
let what: string;
if (range) {
  const shas = git("rev-list", range).split("\n").filter(Boolean);
  for (const sha of shas)
    checkText(
      `commit ${sha.slice(0, 10)}`,
      git("log", "-1", "--format=%B", sha),
      true,
    );
  what = `${shas.length} commit message(s)`;
} else {
  const files = git("ls-files")
    .split("\n")
    .filter((f) => f && !SKIP.test(f));
  for (const file of files) {
    let buf: Buffer;
    try {
      buf = fs.readFileSync(file);
    } catch {
      continue; // deleted in the working tree
    }
    if (buf.includes(0)) continue; // binary
    checkText(file, buf.toString("utf8"), PROSE.test(file));
  }
  what = `${files.length} files`;
}

const lists = [
  handNames.length && "names",
  vocabNames.length && "vocabulary",
  proseWords.length && "prose words",
].filter(Boolean);
if (found.length) {
  console.error(found.join("\n"));
  console.error(
    `\n${found.length} thing(s) that should not be in a public repository.`,
  );
  process.exit(1);
}
console.log(
  `public text is clean (${what}; ${lists.length ? `private lists checked: ${lists.join(", ")}` : "no private lists on this computer"})`,
);
