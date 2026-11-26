// Known security advisories in the parts Seogi is built from.
//   bun scripts/check-deps.ts          (needs cargo-audit: cargo install cargo-audit)
//
// Asks two public advisory databases: bun audit (npm packages in bun.lock) and
// cargo audit (RustSec, crates in src-tauri/Cargo.lock). A high or critical
// advisory fails the check unless KNOWN below lists it with a reason.
// Everything lower is printed as a warning, and so is a Rust advisory whose
// score this script cannot work out (RustSec gives no score, or a CVSS 4.0
// one). Unmaintained or unsound crates are warnings too.
import { spawnSync } from "child_process";

type Severity = "critical" | "high" | "moderate" | "low" | "unscored";
type Finding = {
  source: "npm" | "cargo";
  id: string;
  pkg: string;
  severity: Severity;
  title: string;
};

// High or critical advisories already looked at. Each says why it may stay for
// now; remove the line once the package is upgraded (the check says when a
// line no longer matches anything). A new advisory never goes here just to
// turn the check green: upgrade first, and list it only with a real reason.
const buildOnly =
  "build or lint tool only (eslint, vite, tailwind, babel, fonts): runs on the developer's computer and in CI, not in the installed app; upgrade in its own PR";
const KNOWN: Record<string, string> = {
  // npm, all reached only through devDependencies or the build (vite).
  "GHSA-3jxr-9vmj-r5cp": buildOnly, // brace-expansion
  "GHSA-6j4f-fj2g-mc7p": buildOnly, // brace-expansion
  "GHSA-mh99-v99m-4gvg": buildOnly, // brace-expansion
  "GHSA-qhr7-859c-m2p7": buildOnly, // brace-expansion
  "GHSA-rgw5-rvv9-x895": buildOnly, // brace-expansion
  "GHSA-73wf-gq98-2v4g": buildOnly, // browserslist
  "GHSA-c83g-rgw3-j3cx": buildOnly, // browserslist
  "GHSA-25h7-pfq9-p65f": buildOnly, // flatted
  "GHSA-rf6f-7fwh-wjgh": buildOnly, // flatted
  "GHSA-2883-xcg3-v3hh": buildOnly, // js-yaml
  "GHSA-r5fr-rjxr-66jc": buildOnly, // lodash
  "GHSA-23c5-xmqv-rm74": buildOnly, // minimatch
  "GHSA-3ppc-4f35-3m26": buildOnly, // minimatch
  "GHSA-7r86-cg39-jmmj": buildOnly, // minimatch
  "GHSA-28wg-ghj8-5hjv": buildOnly, // nanoid (postcss)
  "GHSA-2v37-7h3g-55p8": buildOnly, // nanoid (postcss)
  "GHSA-xwg4-73v4-xw9w": buildOnly, // nanoid (postcss)
  "GHSA-c2c7-rcm5-vvqj": buildOnly, // picomatch
  "GHSA-6g55-p6wh-862q": buildOnly, // postcss
  "GHSA-r28c-9q8g-f849": buildOnly, // postcss
  "GHSA-mw96-cpmx-2vgc": buildOnly, // rollup
  "GHSA-68fv-2mgg-jv7q": buildOnly, // source-map-js
  "GHSA-587h-7wmj-h5pm": buildOnly, // vite (dev server)
  "GHSA-p9ff-h696-f583": buildOnly, // vite (dev server)
  "GHSA-mrhx-c7xc-m3w5":
    "zod is listed in package.json but nothing imports it, so it is not in the app's bundle; drop or upgrade it in its own PR",
  // Rust.
  "RUSTSEC-2026-0194":
    "quick-xml through plist (tauri): reads the app's own property lists, not files from outside; upgrade with tauri in its own PR",
  "RUSTSEC-2026-0195":
    "quick-xml through plist (tauri): reads the app's own property lists, not files from outside; upgrade with tauri in its own PR",
  "RUSTSEC-2026-0367":
    "zip through tauri-plugin-updater: unpacks only an update whose signature has already been checked against Seogi's key; upgrade the updater in its own PR",
  "RUSTSEC-2026-0369":
    "zip through tauri-plugin-updater: unpacks only an update whose signature has already been checked against Seogi's key; upgrade the updater in its own PR",
  "RUSTSEC-2026-0429":
    "zip through tauri-plugin-updater: unpacks only an update whose signature has already been checked against Seogi's key; upgrade the updater in its own PR",
};

/** CVSS 3.x base score of a vector string, or undefined for anything else. */
export function cvss3Score(vector: string): number | undefined {
  if (!/^CVSS:3\.[01]\//.test(vector)) return undefined;
  const m = Object.fromEntries(
    vector
      .split("/")
      .slice(1)
      .map((part) => part.split(":")),
  );
  const changed = m.S === "C";
  const av = { N: 0.85, A: 0.62, L: 0.55, P: 0.2 }[m.AV as string];
  const ac = { L: 0.77, H: 0.44 }[m.AC as string];
  const pr = (
    changed ? { N: 0.85, L: 0.68, H: 0.5 } : { N: 0.85, L: 0.62, H: 0.27 }
  )[m.PR as string];
  const ui = { N: 0.85, R: 0.62 }[m.UI as string];
  const cia = (k: string) => ({ H: 0.56, L: 0.22, N: 0 })[m[k] as string];
  const [c, i, a] = [cia("C"), cia("I"), cia("A")];
  if ([av, ac, pr, ui, c, i, a].some((x) => x === undefined)) return undefined;
  const iss = 1 - (1 - c!) * (1 - i!) * (1 - a!);
  const impact = changed
    ? 7.52 * (iss - 0.029) - 3.25 * Math.pow(iss - 0.02, 15)
    : 6.42 * iss;
  if (impact <= 0) return 0;
  const exploitability = 8.22 * av! * ac! * pr! * ui!;
  const raw = Math.min((changed ? 1.08 : 1) * (impact + exploitability), 10);
  // The specification's round-up to one decimal, safe from float noise.
  const int = Math.round(raw * 100000);
  return int % 10000 === 0 ? int / 100000 : (Math.floor(int / 10000) + 1) / 10;
}

function severityOf(score: number | undefined): Severity {
  if (score === undefined) return "unscored";
  if (score >= 9) return "critical";
  if (score >= 7) return "high";
  if (score >= 4) return "moderate";
  return "low";
}

function run(cmd: string, args: string[], cwd = "."): string {
  const r = spawnSync(cmd, args, { cwd, encoding: "utf8" });
  if (r.error) {
    console.error(`could not run ${cmd}: ${r.error.message}`);
    if (cmd === "cargo")
      console.error("install it with: cargo install cargo-audit --locked");
    process.exit(2);
  }
  // Both tools exit non-zero when they find something; the JSON says what.
  if (!r.stdout.trim()) {
    console.error(`${cmd} ${args.join(" ")} printed nothing:\n${r.stderr}`);
    process.exit(2);
  }
  return r.stdout;
}

type NpmAdvisory = { url: string; title: string; severity: string };
function npmFindings(): Finding[] {
  const report = JSON.parse(run("bun", ["audit", "--json"])) as Record<
    string,
    NpmAdvisory[]
  >;
  return Object.entries(report).flatMap(([pkg, list]) =>
    list.map((a) => ({
      source: "npm" as const,
      id: a.url.split("/").pop() ?? a.url,
      pkg,
      severity: (["critical", "high", "moderate", "low"].includes(a.severity)
        ? a.severity
        : "unscored") as Severity,
      title: a.title,
    })),
  );
}

type CargoEntry = {
  advisory: { id: string; title: string; cvss: string | null } | null;
  package: { name: string; version: string };
  kind?: string;
};
function cargoFindings(): { vulns: Finding[]; warnings: string[] } {
  const report = JSON.parse(run("cargo", ["audit", "--json"], "src-tauri")) as {
    vulnerabilities: { list: CargoEntry[] };
    warnings: Record<string, CargoEntry[]>;
  };
  const vulns = report.vulnerabilities.list.map((v) => ({
    source: "cargo" as const,
    id: v.advisory!.id,
    pkg: `${v.package.name} ${v.package.version}`,
    severity: severityOf(cvss3Score(v.advisory!.cvss ?? "")),
    title: v.advisory!.title,
  }));
  const warnings = Object.entries(report.warnings ?? {}).flatMap(
    ([kind, list]) =>
      list.map(
        (w) =>
          `${kind}: ${w.package.name} ${w.package.version}${w.advisory ? ` (${w.advisory.id})` : ""}`,
      ),
  );
  return { vulns, warnings };
}

if (import.meta.main) {
  const npm = npmFindings();
  const cargo = cargoFindings();
  const all = [...npm, ...cargo.vulns];
  const serious = (f: Finding) =>
    f.severity === "high" || f.severity === "critical";

  const failing = all.filter((f) => serious(f) && !KNOWN[f.id]);
  const known = all.filter((f) => serious(f) && KNOWN[f.id]);
  const lower = all.filter((f) => !serious(f));
  const line = (f: Finding) =>
    `${f.severity.padEnd(8)} ${f.source} ${f.pkg} ${f.id}: ${f.title}`;

  if (lower.length) {
    console.log(`\nLower or unscored (warning only), ${lower.length}:`);
    lower.forEach((f) => console.log(`  ${line(f)}`));
  }
  if (cargo.warnings.length) {
    console.log(
      `\nRust crates unmaintained or unsound (warning only), ${cargo.warnings.length}:`,
    );
    cargo.warnings.forEach((w) => console.log(`  ${w}`));
  }
  if (known.length) {
    console.log(`\nHigh or critical, already known (KNOWN), ${known.length}:`);
    known.forEach((f) => console.log(`  ${line(f)}\n      ${KNOWN[f.id]}`));
  }
  const seen = new Set(all.map((f) => f.id));
  const stale = Object.keys(KNOWN).filter((id) => !seen.has(id));
  if (stale.length)
    console.log(
      `\nNo longer found, remove from KNOWN in scripts/check-deps.ts: ${stale.join(", ")}`,
    );
  if (lower.length || cargo.warnings.length)
    console.log(
      `::warning title=Dependencies::${lower.length} lower advisories and ${cargo.warnings.length} unmaintained or unsound crates (see the log)`,
    );

  if (failing.length) {
    console.log(`\nHigh or critical, not yet looked at, ${failing.length}:`);
    failing.forEach((f) =>
      console.log(`::error title=${f.source} ${f.pkg}::${line(f)}`),
    );
    console.log(
      "\nUpgrade the package (bun update / cargo update -p <crate>). Only if it cannot reach the app, add it to KNOWN with the reason.",
    );
    process.exit(1);
  }
  console.log(
    `\nno high or critical advisory outside KNOWN (${npm.length} npm, ${cargo.vulns.length} Rust advisories in all)`,
  );
}
