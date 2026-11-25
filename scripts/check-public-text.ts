// Keeps private things out of this public repository.
//
//   bun scripts/check-public-text.ts                   every tracked file as it is now
//   bun scripts/check-public-text.ts --push BASE TIP   what a push would publish: every
//                                                      file at TIP, every line added by a
//                                                      commit in BASE..TIP (also lines a
//                                                      later commit removed), and those
//                                                      commits' messages; BASE may be "-"
//   bun scripts/check-public-text.ts --stdin           text from standard input, such as
//                                                      a pull request's title and body
//
// 1. Shapes that never belong here: real-looking email addresses, phone
//    numbers, access tokens, private keys. Checked everywhere, also in CI.
// 2. On the maintainer's computer only, from the private folder
//    (SEOGI_PRIVATE_DIR, by default ~/seogi/private; never in this repository):
//    - public-check/names.txt: words that must not appear anywhere (one per
//      line, any case);
//    - the names in 용어집/vocabulary.json (the same file
//      eval/synthetic/check_names.py reads): entries whose category is listed
//      in public-check/name-categories.txt, Korean forms of three letters or
//      more, less the ordinary words in public-check/not-names.txt;
//    - public-check/prose-words.txt: words public prose does not use
//      (README, docs, screen text, templates, commit messages, pull
//      requests). See AGENTS.md, Hard rules.
//    A private word is never printed: a finding names the place only.
//
// The private lists cannot run in CI, so the pre-push hook (.githooks/pre-push)
// runs this script on the maintainer's computer before anything leaves it.
import { execFileSync } from "child_process";
import fs from "fs";
import os from "os";
import path from "path";

// Files that are not ours to check: lock files, vendored code, upstream
// translations Seogi does not maintain, the list of other projects' notices.
export const SKIP =
  /^(bun\.lock|src-tauri\/Cargo\.lock|src-tauri\/vendor\/|src\/i18n\/locales\/(?!ko\/|en\/)|THIRD_PARTY_NOTICES\.md)/;
// Prose people read: what the prose word list applies to.
export const PROSE =
  /^(README\.md|GLOSSARY\.md|BUILD\.md|docs\/.*\.md|src\/seogi\/i18n\/.*\.ts|\.github\/.*\.md|\.github\/ISSUE_TEMPLATE\/.*)$/;

// ---- shapes ----------------------------------------------------------------

const EMAIL =
  /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;
// Korean mobile numbers, with -, . or a space between the parts, or none.
const PHONE =
  /(?<![0-9A-Za-z])(?:\+82[-. ]?0?1[016789]|01[016789])[-. ]?\d{3,4}[-. ]?\d{4}(?![0-9A-Za-z])/g;
const TOKEN =
  /\b(?:xox[abprs]-[A-Za-z0-9-]{10,}|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|hf_[A-Za-z0-9]{30,})/g;
const PRIVATE_KEY = /-----BEGIN [A-Z ]*PRIVATE KEY-----/g;

// Domains reserved for examples (RFC 2606, RFC 6761), matched at the end.
const EXAMPLE_TLDS = ["example", "test", "invalid", "localhost"];
const EXAMPLE_DOMAINS = ["example.com", "example.org", "example.net"];
// Addresses nobody reads: GitHub's private commit addresses and the
// co-author lines of commits.
const NOREPLY = ["noreply@anthropic.com", "noreply@github.com"];

const endsWithDomain = (domain: string, suffix: string) =>
  domain === suffix || domain.endsWith(`.${suffix}`);

function emailAllowed(address: string, next: string): boolean {
  const at = address.lastIndexOf("@");
  const local = address.slice(0, at).toLowerCase();
  const domain = address.slice(at + 1).toLowerCase();
  if (local === "git" && next === ":") return true; // git@github.com:owner/repo
  if (/^\d+x\.(png|jpe?g|webp|gif|svg|icns|ico)$/.test(domain)) return true; // icon@2x.png
  if (EXAMPLE_TLDS.some((tld) => domain.endsWith(`.${tld}`))) return true;
  if (EXAMPLE_DOMAINS.some((d) => endsWithDomain(domain, d))) return true;
  if (domain === "users.noreply.github.com") return true;
  return NOREPLY.includes(`${local}@${domain}`);
}

/** The private-looking shapes in one line, as "kind: text". */
export function findShapes(line: string): string[] {
  const out: string[] = [];
  for (const m of line.matchAll(EMAIL))
    if (!emailAllowed(m[0], line[m.index + m[0].length] ?? ""))
      out.push(`email address: ${m[0]}`);
  for (const m of line.matchAll(PHONE)) out.push(`phone number: ${m[0]}`);
  for (const m of line.matchAll(TOKEN))
    if (!m[0].includes("secret-key")) out.push(`access token: ${m[0]}`); // fakes in tests
  for (const m of line.matchAll(PRIVATE_KEY)) out.push(`private key: ${m[0]}`);
  return out;
}

// ---- private lists ---------------------------------------------------------

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * One expression for a list of words. A Latin word must stand alone. A Korean
 * word may be followed by anything (particles, 님). One of four letters or
 * more matches anywhere; one of three must not follow another Korean letter,
 * since three-letter names hide inside ordinary words (a name ending a longer
 * word matched in this repository only that way). So a three-letter name
 * glued to the word before it, with no space, is not caught.
 */
export function wordMatcher(words: string[], flags = ""): RegExp | null {
  if (!words.length) return null;
  const parts = [...new Set(words)]
    .sort((a, b) => b.length - a.length)
    .map((w) =>
      /^[A-Za-z0-9 ._-]+$/.test(w)
        ? `(?<![A-Za-z0-9])${escape(w)}(?![A-Za-z0-9])`
        : [...w].length >= 4
          ? escape(w)
          : `(?<![가-힣])${escape(w)}`,
    );
  return new RegExp(parts.join("|"), flags);
}

type Lists = { names: RegExp[]; prose: RegExp | null; loaded: string[] };

function loadLists(dir: string): Lists {
  const list = (file: string): string[] => {
    const full = path.join(dir, file);
    if (!fs.existsSync(full)) return [];
    return fs
      .readFileSync(full, "utf8")
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith("#"));
  };
  const hand = list("public-check/names.txt");
  const vocabulary = vocabularyNames(
    path.join(dir, "용어집", "vocabulary.json"),
    new Set(list("public-check/name-categories.txt")),
    new Set(list("public-check/not-names.txt")),
  );
  const prose = list("public-check/prose-words.txt");
  return {
    names: [wordMatcher(hand, "i"), wordMatcher(vocabulary)].filter(
      (r): r is RegExp => r !== null,
    ),
    prose: wordMatcher(prose, "i"),
    loaded: [
      hand.length && "names",
      vocabulary.length && "vocabulary",
      prose.length && "prose words",
    ].filter((x): x is string => Boolean(x)),
  };
}

/** The Korean names of three letters or more in the private vocabulary. */
function vocabularyNames(
  file: string,
  nameCategories: Set<string>,
  ordinaryWords: Set<string>,
): string[] {
  if (!fs.existsSync(file) || !nameCategories.size) return [];
  type Entry = { term?: string; category?: string; aliases?: string[] };
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  const entries: Entry[] = Array.isArray(data) ? data : (data.entries ?? []);
  const names = new Set<string>();
  const ordinary = new Set(ordinaryWords);
  for (const e of entries) {
    const forms = [e.term ?? "", ...(e.aliases ?? [])]
      .map((f) => f.trim())
      .filter(Boolean);
    const into = nameCategories.has(e.category ?? "") ? names : ordinary;
    forms.forEach((f) => into.add(f));
  }
  // Shorter forms sit inside ordinary words; nicknames in Latin letters are
  // ordinary English words (a Latin name that matters belongs in names.txt).
  return [...names].filter(
    (n) => !ordinary.has(n) && /[가-힣]/.test(n) && [...n].length >= 3,
  );
}

// ---- checking --------------------------------------------------------------

export class Checker {
  found: string[] = [];
  constructor(private lists: Lists) {}

  line(at: string, line: string, prose: boolean) {
    for (const s of findShapes(line)) this.found.push(`${at} ${s}`);
    if (this.lists.names.some((re) => re.test(line)))
      this.found.push(`${at} a name from the private list`);
    if (prose && this.lists.prose?.test(line))
      this.found.push(`${at} a word public prose does not use`);
  }

  text(where: string, text: string, prose: boolean) {
    text
      .split("\n")
      .forEach((l, i) => this.line(`${where}:${i + 1}`, l, prose));
  }
}

const git = (args: string[], input?: string) =>
  execFileSync("git", ["-c", "core.quotePath=false", ...args], {
    maxBuffer: 1 << 30,
    input,
  });
const nulList = (b: Buffer) => b.toString("utf8").split("\0").filter(Boolean);
const isBinary = (b: Buffer) => b.subarray(0, 8000).includes(0);

function checkWorkingTree(c: Checker): string {
  const files = nulList(git(["ls-files", "-z"])).filter((f) => !SKIP.test(f));
  for (const file of files) {
    let buf: Buffer;
    try {
      buf = fs.readFileSync(file);
    } catch (e) {
      // Only a file deleted in the working tree is skipped.
      if ((e as NodeJS.ErrnoException).code === "ENOENT") continue;
      throw e;
    }
    if (!isBinary(buf)) c.text(file, buf.toString("utf8"), PROSE.test(file));
  }
  return `${files.length} files`;
}

/** Every file in a commit, read in one `git cat-file --batch`. */
function checkCommitFiles(c: Checker, tip: string): number {
  const files = nulList(
    git(["ls-tree", "-r", "-z", "--name-only", tip]),
  ).filter((f) => !SKIP.test(f) && !f.includes("\n"));
  const out = git(
    ["cat-file", "--batch"],
    files.map((f) => `${tip}:${f}\n`).join(""),
  );
  let pos = 0;
  for (const file of files) {
    const eol = out.indexOf(10, pos);
    const header = out.subarray(pos, eol).toString("utf8").split(" ");
    if (header[1] === "missing")
      throw new Error(`git cat-file: ${file} missing`);
    const size = Number(header[2]);
    const body = out.subarray(eol + 1, eol + 1 + size);
    pos = eol + 1 + size + 1;
    if (header[1] === "blob" && !isBinary(body))
      c.text(
        `${tip.slice(0, 10)}:${file}`,
        body.toString("utf8"),
        PROSE.test(file),
      );
  }
  return files.length;
}

/** Every line the commits in a range add, even ones a later commit removed. */
function checkAddedLines(c: Checker, range: string[]): number {
  const log = git([
    "log",
    "-p",
    "--no-color",
    "--no-renames",
    "--no-ext-diff",
    "--unified=0",
    "--format=%x00%H",
    ...range,
  ]).toString("utf8");
  let commit = "";
  let file: string | null = null;
  let inHeader = false;
  let lineNo = 0;
  let added = 0;
  for (const line of log.split("\n")) {
    if (line.startsWith("\0")) {
      commit = line.slice(1, 11);
      file = null;
    } else if (line.startsWith("diff --git ")) {
      inHeader = true;
      file = null;
    } else if (inHeader) {
      if (line.startsWith("+++ "))
        file = line === "+++ /dev/null" ? null : line.slice(6); // "+++ b/<path>"
      else if (line.startsWith("@@")) inHeader = false;
      if (line.startsWith("@@"))
        lineNo = Number(/\+(\d+)/.exec(line)?.[1] ?? 0);
    } else if (line.startsWith("@@")) {
      lineNo = Number(/\+(\d+)/.exec(line)?.[1] ?? 0);
    } else if (line.startsWith("+") && file && !SKIP.test(file)) {
      c.line(
        `commit ${commit} ${file}:${lineNo} (added)`,
        line.slice(1),
        PROSE.test(file),
      );
      lineNo++;
      added++;
    }
  }
  return added;
}

function checkMessages(c: Checker, range: string[]): number {
  const shas = nulList(
    git(["rev-list", ...range])
      .toString("utf8")
      .replace(/\n/g, "\0"),
  );
  for (const sha of shas)
    c.text(
      `commit ${sha.slice(0, 10)} message`,
      git(["log", "-1", "--format=%B", sha]).toString("utf8"),
      true,
    );
  return shas.length;
}

function main(argv: string[]) {
  const dir =
    process.env.SEOGI_PRIVATE_DIR ||
    path.join(os.homedir(), "seogi", "private");
  const lists = loadLists(dir);
  const c = new Checker(lists);
  let what: string;
  if (argv[0] === "--push") {
    const [base, tip] = [argv[1], argv[2]];
    if (!base || !tip) throw new Error("usage: --push BASE|- TIP");
    const range = base === "-" ? [tip] : [`${base}..${tip}`];
    const files = checkCommitFiles(c, tip);
    const added = checkAddedLines(c, range);
    const messages = checkMessages(c, range);
    what = `${files} files at ${tip.slice(0, 10)}, ${added} added line(s) and ${messages} message(s) in ${range[0].slice(0, 24)}`;
  } else if (argv[0] === "--stdin") {
    c.text("stdin", fs.readFileSync(0, "utf8"), true);
    what = "standard input";
  } else {
    what = checkWorkingTree(c);
  }
  if (c.found.length) {
    console.error(c.found.join("\n"));
    console.error(
      `\n${c.found.length} thing(s) that should not be in a public repository.`,
    );
    process.exit(1);
  }
  const lists_ = lists.loaded.length
    ? `private lists checked: ${lists.loaded.join(", ")}`
    : "no private lists on this computer";
  console.log(`public text is clean (${what}; ${lists_})`);
}

if (import.meta.main) main(process.argv.slice(2));
