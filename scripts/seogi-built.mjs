// Checks that the built screens (dist/, what an installed app carries) load
// and draw: the window's first screen and the banner. The browser preview
// runs the sources through the dev server; this runs what `bun run build`
// produced, where a broken chunk or a missing asset shows up only here.
//
//   bun run build && node scripts/seogi-built.mjs
//
// The app's back end is stood in for by the barest answers. A JSON file of
// `{ "command": answer }` may be given to replay real ones:
//   node scripts/seogi-built.mjs answers.json

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { extname, join, normalize, resolve } from "node:path";
import { chromium } from "playwright";

const dist = resolve(import.meta.dirname, "..", "dist");
const given = process.argv[2]
  ? JSON.parse(readFileSync(process.argv[2], "utf8"))
  : {};

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".wav": "audio/wav",
};

const server = createServer(async (request, response) => {
  const path = normalize(
    decodeURIComponent(new URL(request.url, "http://x").pathname),
  );
  const file = join(dist, path.endsWith("/") ? `${path}index.html` : path);
  if (!file.startsWith(dist)) {
    response.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, {
      "content-type": types[extname(file)] ?? "application/octet-stream",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const origin = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium
  .launch({ headless: true })
  .catch(() => chromium.launch({ channel: "chrome", headless: true }));

/** The back end, as little of it as the screens need to start. */
function standIn(given) {
  let next = 0;
  const calls = [];
  window.__calls = calls;
  const answers = {
    seogi_config: {
      library_dir: "",
      watched: [],
      language: "ko",
      diarize: true,
      speech_model: "",
      summary: {
        kind: "ollama",
        base_url: "http://localhost:11434",
        model: "gemma3:12b",
      },
      vocabulary_path: "",
      input_device: "",
      calendar_urls: [],
      remind: true,
      remind_minutes_before: 2,
      set_up: false,
      record_shortcut: "",
      mark_shortcut: "",
      muted_events: [],
      minutes_beside_source: true,
    },
    seogi_templates: [],
    seogi_meetings: [],
    seogi_today: [],
    seogi_recording: null,
    seogi_banner: {
      kind: "ready",
      id: "20261124-140000-0001",
      title: "주간 회의",
      actions: 3,
      to_check: 1,
    },
    ...given,
  };
  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main", windowLabel: "main" },
    },
    transformCallback: () => ++next,
    unregisterCallback() {},
    convertFileSrc: (path) => `asset://localhost/${encodeURIComponent(path)}`,
    invoke(command) {
      calls.push(command);
      if (command in answers) return Promise.resolve(answers[command]);
      if (command.startsWith("plugin:event|")) return Promise.resolve(++next);
      return Promise.resolve(null);
    },
  };
  window.__TAURI_OS_PLUGIN_INTERNALS__ = {
    platform: "macos",
    os_type: "macos",
    family: "unix",
    eol: "\n",
    arch: "aarch64",
    version: "15.0.0",
    exe_extension: "",
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
}

let failed = 0;
async function check(name, path, expectText) {
  const page = await browser.newPage();
  const problems = [];
  page.on("pageerror", (error) =>
    problems.push(String(error.stack || error).slice(0, 400)),
  );
  page.on("response", (response) => {
    if (response.status() >= 400)
      problems.push(`${response.status()} ${response.url()}`);
  });
  page.on("requestfailed", (request) =>
    problems.push(`${request.failure()?.errorText} ${request.url()}`),
  );
  await page.addInitScript(standIn, given);
  await page.goto(`${origin}${path}`, { waitUntil: "load" });
  await page.waitForTimeout(1500);
  const drawn = await page.evaluate(() => ({
    children: document.getElementById("root")?.childElementCount ?? 0,
    text: document.body.innerText,
    background: getComputedStyle(document.body).backgroundColor,
  }));
  if (drawn.children === 0) problems.push("nothing was drawn");
  if (expectText && !drawn.text.includes(expectText)) {
    problems.push(`the text "${expectText}" is not on the page`);
  }
  if (problems.length > 0) failed += 1;
  console.log(`${problems.length === 0 ? "PASS" : "FAIL"}  ${name}`);
  for (const problem of problems) console.log(`      ${problem}`);
  await page.close();
}

await check(
  "the window's first screen",
  "/index.html",
  "회의록 정리를 어디에 맡길까요?",
);
await check(
  "the banner",
  "/src/seogi/banner/index.html",
  "회의록이 준비됐습니다",
);

await browser.close();
server.close();
process.exit(failed === 0 ? 0 : 1);
