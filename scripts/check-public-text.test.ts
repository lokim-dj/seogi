// bun test scripts/check-public-text.test.ts
//
// The addresses and numbers below are put together at run time, so this file
// itself stays clean for the check it tests.
import { describe, expect, test } from "bun:test";
import { execFileSync, spawnSync } from "child_process";
import fs from "fs";
import os from "os";
import path from "path";
import { findShapes, wordMatcher } from "./check-public-text";

const at = (local: string, domain: string) => [local, domain].join("@");
const digits = (sep: string, ...parts: string[]) => parts.join(sep);

describe("shapes that are caught", () => {
  const caught: [string, string][] = [
    ["a real mail address", at("someone", "gmail.com")],
    ["noreply at a real domain", at("noreply", "mailer.co.kr")],
    ["an example word inside a longer domain", at("a", "mail.example.co.kr")],
    ["a test word inside a longer domain", at("a", "x.test.co.kr")],
    ["a long subdomain", at("first.last", "mail.dept.corp.co.kr")],
    ["a mobile number with no separator", digits("", "010", "1234", "5678")],
    ["a mobile number with spaces", digits(" ", "010", "1234", "5678")],
    ["a mobile number with hyphens", digits("-", "010", "1234", "5678")],
    ["a mobile number with dots", digits(".", "011", "123", "4567")],
    [
      "an international mobile number",
      digits(" ", "+82", "10", "1234", "5678"),
    ],
    ["a GitHub token", ["ghp", "a".repeat(36)].join("_")],
    ["a private key", ["-----BEGIN", "RSA PRIVATE KEY-----"].join(" ")],
  ];
  for (const [what, text] of caught)
    test(what, () => expect(findShapes(`see ${text} here`)).not.toEqual([]));
});

describe("shapes that are allowed", () => {
  const allowed: [string, string][] = [
    ["a reserved example domain", at("me", "work.example")],
    ["example.com", at("a", "b.example.com")],
    [
      "GitHub's private commit address",
      at("1+someone", "users.noreply.github.com"),
    ],
    ["a co-author line", at("noreply", "anthropic.com")],
    ["an ssh remote", `${at("git", "github.com")}:owner/repo.git`],
    ["an image name", at("icon", "2x.png")],
    ["a fake key in a test", ["sk-ant", "secret-key-67890"].join("-")],
    ["a package version", at("react", "18.3.1")],
    [
      "a long number that holds a mobile-like run",
      digits("", "17", "010", "1234", "5678"),
    ],
    ["a timestamp", "2026-11-25 0101 1234"],
  ];
  for (const [what, text] of allowed)
    test(what, () => expect(findShapes(`see ${text} here`)).toEqual([]));
});

describe("private words", () => {
  const re = wordMatcher(["가나다", "가나다라", "acme"])!;
  test("a three-letter Korean name with a particle", () =>
    expect(re.test("가나다님이")).toBe(true));
  test("a three-letter Korean name after a space", () =>
    expect(re.test("담당 가나다")).toBe(true));
  test("a three-letter Korean name inside a word is not one", () =>
    expect(wordMatcher(["가나다"])!.test("마가나다")).toBe(false));
  test("a four-letter Korean name glued to a word", () =>
    expect(re.test("마가나다라")).toBe(true));
  test("a Latin word on its own, any case", () =>
    expect(wordMatcher(["acme"], "i")!.test("see ACME.")).toBe(true));
  test("a Latin word inside a longer word is not one", () =>
    expect(re.test("acmes")).toBe(false));
});

describe("--push", () => {
  const script = path.resolve(import.meta.dir, "check-public-text.ts");
  const run = (cwd: string, ...args: string[]) =>
    spawnSync("bun", [script, ...args], {
      cwd,
      encoding: "utf8",
      env: { ...process.env, SEOGI_PRIVATE_DIR: path.join(cwd, ".private") },
    });

  function repo() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "public-text-"));
    const git = (...a: string[]) =>
      execFileSync(
        "git",
        ["-c", "user.name=t", "-c", `user.email=${at("t", "t.test")}`, ...a],
        {
          cwd: dir,
        },
      )
        .toString()
        .trim();
    git("init", "-q");
    fs.mkdirSync(path.join(dir, ".private", "public-check"), {
      recursive: true,
    });
    fs.writeFileSync(
      path.join(dir, ".private", "public-check", "names.txt"),
      "zorblatco\n",
    );
    fs.writeFileSync(path.join(dir, "a.txt"), "clean\n");
    git("add", "a.txt");
    git("commit", "-qm", "start");
    return { dir, git };
  }

  test("a name added and removed again before the push is caught", () => {
    const { dir, git } = repo();
    const base = git("rev-parse", "HEAD");
    fs.writeFileSync(path.join(dir, "a.txt"), "clean\nby Zorblatco\n");
    git("commit", "-qam", "add");
    fs.writeFileSync(path.join(dir, "a.txt"), "clean\n");
    git("commit", "-qam", "remove");
    const r = run(dir, "--push", base, git("rev-parse", "HEAD"));
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("a.txt:2 (added) a name from the private list");
    expect(r.stderr).not.toContain("orblat"); // never printed
  });

  test("the pushed commit is checked, not the working tree", () => {
    const { dir, git } = repo();
    const base = git("rev-parse", "HEAD");
    fs.writeFileSync(path.join(dir, "b.txt"), "zorblatco\n");
    git("add", "b.txt");
    git("commit", "-qm", "b");
    const tip = git("rev-parse", "HEAD");
    git("checkout", "-q", base); // another branch is checked out here
    expect(run(dir, "--push", base, tip).status).toBe(1);
    expect(run(dir).status).toBe(0);
  });

  test("a file with a Korean name is read", () => {
    const { dir, git } = repo();
    const base = git("rev-parse", "HEAD");
    fs.writeFileSync(
      path.join(dir, "회의 메모.md"),
      `${at("someone", "gmail.com")}\n`,
    );
    git("add", "회의 메모.md");
    git("commit", "-qm", "memo");
    const r = run(dir, "--push", base, git("rev-parse", "HEAD"));
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("회의 메모.md:1");
    const w = run(dir);
    expect(w.status).toBe(1);
    expect(w.stderr).toContain("회의 메모.md:1 email address");
  });

  test("a commit message is checked", () => {
    const { dir, git } = repo();
    const base = git("rev-parse", "HEAD");
    git("commit", "-q", "--allow-empty", "-m", "thanks to zorblatco");
    const r = run(dir, "--push", base, git("rev-parse", "HEAD"));
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("message:1 a name from the private list");
  });
});
