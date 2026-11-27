import { test, expect } from "@playwright/test";

// Every Seogi screen opens in the browser preview (src/seogi/preview), on
// made-up data and without the backend, showing something and with no error
// in the console. Run with `bun run test:playwright`; it starts the dev
// server itself, but outside CI it reuses one already on port 1420, so stop
// another checkout's `bun run dev` first or the test checks that code.

// "waiting" is left out: the preview has no meeting behind the one being
// worked on, so that screen logs "no such meeting in the preview".
const screens = [
  "home",
  "home-going",
  "home-many",
  "minutes",
  "recording",
  "onboarding",
  "onboarding&step=2",
  "onboarding&step=3",
  "onboarding&step=3&os=windows",
  "settings",
  "settings-dictation",
  "banner-starting",
  "banner-ready",
  "card",
  "card-empty",
  "mic-idle",
  "mic-listening",
  "mic-working",
  "indicator-listening",
  "indicator-tidying",
  "indicator-note",
  "indicator-nowhere",
];

for (const screen of screens) {
  test(`${screen} opens without errors`, async ({ page }) => {
    const problems: string[] = [];
    page.on("pageerror", (error) => problems.push(String(error)));
    page.on("console", (message) => {
      // A missing file is told by its address below, not by this line.
      if (
        message.type() === "error" &&
        !message.text().startsWith("Failed to load resource")
      )
        problems.push(message.text());
    });
    page.on("response", (response) => {
      // The preview page has no icon; the browser asks for one anyway.
      const url = new URL(response.url());
      if (response.status() >= 400 && url.pathname !== "/favicon.ico")
        problems.push(`${response.status()} ${url.pathname}`);
    });
    await page.goto(`/src/seogi/preview/index.html?screen=${screen}`, {
      waitUntil: "networkidle",
    });
    await expect(page.locator("#root")).not.toBeEmpty();
    await expect(page.locator("#root")).not.toHaveText("");
    expect(problems).toEqual([]);
  });
}
