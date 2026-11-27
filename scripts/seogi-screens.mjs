// Takes a picture of every Seogi screen from the browser preview
// (src/seogi/preview), without the app or its backend:
//
//   bun run dev            # in another terminal
//   node scripts/seogi-screens.mjs [out-dir]
//
// The pictures are for checking a change by eye; they are not kept in the
// repository.

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "@playwright/test";

const out = process.argv[2] ?? "seogi-screens";
const base = "http://localhost:1420/src/seogi/preview/index.html?screen=";
const screens = [
  ["home", 1280, 800, false],
  ["home-going", 1280, 800, false],
  ["home-many", 1280, 800, false],
  ["waiting", 1280, 800, false],
  ["minutes", 1280, 800, true],
  ["recording", 1280, 800, false],
  ["onboarding", 1280, 800, true],
  ["onboarding&step=2", 1280, 800, true, "onboarding-2"],
  ["onboarding&step=3", 1280, 800, true, "onboarding-3"],
  ["onboarding&step=3&os=windows", 1280, 800, true, "onboarding-3-windows"],
  ["settings", 1280, 800, true],
  ["banner-starting", 584, 76, false],
  ["banner-ready", 312, 112, false],
  ["home", 920, 700, true, "home-narrow"],
  ["card", 1280, 980, false],
  ["card-empty", 1280, 980, false],
];

mkdirSync(out, { recursive: true });
// Playwright's own browser if it has been installed, else the Chrome that
// is on this computer (run without a window, with a profile of its own).
const browser = await chromium
  .launch()
  .catch(() => chromium.launch({ channel: "chrome" }));
for (const [screen, width, height, fullPage, name] of screens) {
  const page = await browser.newPage({
    viewport: { width, height },
    deviceScaleFactor: 1,
    locale: "ko-KR",
  });
  const problems = [];
  page.on("pageerror", (error) => problems.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(message.text());
  });
  await page.goto(base + screen, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  // The app scrolls inside its own frame; let a full-page picture see it all.
  if (fullPage) {
    await page.addStyleTag({
      content:
        "html,body,#root,#root>div{height:auto!important;overflow:visible!important}",
    });
    await page.waitForTimeout(100);
  }
  const file = join(out, `${name ?? screen}.png`);
  await page.screenshot({ path: file, fullPage });
  console.log(
    file,
    problems.length ? `PROBLEMS: ${problems.slice(0, 3).join(" | ")}` : "ok",
  );
  await page.close();
}
await browser.close();
