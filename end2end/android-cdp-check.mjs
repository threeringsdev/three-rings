// Probe: attach Playwright to the Tauri Android debug webview over CDP.
//
// Prereqs (recipe in specs/ui-work-loop.md Findings / the android-smoke skill):
//   1. app running on the emulator (`cargo tauri android dev` from the repo root)
//   2. socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | head -1)
//   3. adb forward tcp:9222 localabstract:$socket
//
// Usage: node android-cdp-check.mjs [port]   (default 9222)
// Exits 0 iff attach + evaluate work and at least one page is present.
import { chromium } from "@playwright/test";

const port = process.argv[2] ?? "9222";
try {
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
    timeout: 15000,
  });
  const pages = browser.contexts().flatMap((c) => c.pages());
  for (const p of pages) {
    console.log(`page: ${p.url()} — "${await p.title().catch(() => "?")}"`);
  }
  if (pages.length === 0) throw new Error("no pages in the webview");
  const ua = await pages[0].evaluate(() => navigator.userAgent);
  const hydrated = await pages[0].evaluate(
    () => (document.body?.innerHTML?.length ?? 0) > 0,
  );
  console.log(`userAgent: ${ua}`);
  console.log(`body non-empty: ${hydrated}`);
  await browser.close();
  if (!hydrated) throw new Error("webview body is empty");
  console.log("ANDROID CDP CHECK PASS");
} catch (e) {
  console.error(`ANDROID CDP CHECK FAIL: ${e.message}`);
  process.exit(1);
}
