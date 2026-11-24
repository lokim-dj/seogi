// The version is written in three files, and the Release workflow needs the
// notes of that version (docs/release-notes/v<version>.md) or it stops halfway.
//   bun scripts/check-version.ts
import fs from "fs";

const read = (p: string) => fs.readFileSync(p, "utf8");
const versions: Record<string, string | undefined> = {
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json"))
    .version,
  // The first `version =` line is the [package] one.
  "src-tauri/Cargo.toml": read("src-tauri/Cargo.toml").match(
    /^version\s*=\s*"([^"]+)"/m,
  )?.[1],
};

const problems: string[] = [];
const unique = [...new Set(Object.values(versions))];
if (unique.length !== 1 || !unique[0])
  problems.push(
    `the version differs between files: ${JSON.stringify(versions)}`,
  );
const notes = `docs/release-notes/v${versions["package.json"]}.md`;
if (!fs.existsSync(notes))
  problems.push(`missing ${notes} (the Release workflow reads it)`);

if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`version ${unique[0]} in all three files, ${notes} exists`);
