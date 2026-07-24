// Probe: the DFC back-face flip on the real Android webview, over CDP.
//
// The Android leg of the DFC back-face task (specs/TODO.md). The flip control
// rides the card art on the detail page and inside the touch sheet; this
// drives both on the actual WebView, where `(pointer: coarse)` and the tap
// pipeline are real rather than emulated.
//
// Prereqs — the android-smoke skill's dev-attach recipe:
//   1. app running on the emulator (`cargo tauri android dev` from repo root)
//   2. socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | head -1)
//   3. adb forward tcp:9222 localabstract:$socket
//
// Usage: node android-dfc-check.mjs [port]
import { chromium } from "@playwright/test";

const port = process.argv[2] ?? "9222";
const ORIGIN = "http://tauri.localhost";
const failures = [];

function check(name, ok, detail = "") {
  console.log(`${ok ? "ok  " : "FAIL"} — ${name}${detail ? `: ${detail}` : ""}`);
  if (!ok) failures.push(name);
}

// Card ids resolved at runtime through the dev server the app itself proxies
// (adb reverse makes them the same server), so a re-ingested catalog can't
// rot this probe.
async function firstCard(q) {
  const res = await fetch(
    `http://127.0.0.1:3000/api/search_catalog?q=${encodeURIComponent(q)}`,
  );
  const { cards } = await res.json();
  if (!cards?.length) {
    console.error(`ANDROID DFC FAIL: no catalog hit for "${q}"`);
    process.exit(1);
  }
  return cards[0];
}

const dfc = await firstCard("Agadeem's Awakening");
const adventure = await firstCard("Brazen Borrower");
if (dfc.faces?.length !== 2) {
  console.error("ANDROID DFC FAIL: search projection carries no flip faces");
  process.exit(1);
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
  timeout: 15000,
});
const page = browser.contexts().flatMap((c) => c.pages())[0];
if (!page) {
  console.error("ANDROID DFC FAIL: no page in the webview");
  process.exit(1);
}

// Always goto an explicit URL — one shared page/context across runs, and JS
// location.href races the CDP session (ui-work-loop Findings).
async function go(path) {
  await page.goto(`${ORIGIN}${path}`, { waitUntil: "domcontentloaded" });
  await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
}

// --- detail page: flip swaps heading, art, and oracle text
await go(`/cards/${dfc.oracle_id}`);
const h1 = page.getByTestId("card-name");
check(
  "detail heading shows the front face name",
  (await h1.textContent()) === dfc.faces[0].name,
  await h1.textContent(),
);
check(
  "the combined name stays as the subtitle",
  (await page.getByTestId("card-combined-name").textContent()) === dfc.name,
);
const art = page.locator("img").first();
const frontSrc = await art.getAttribute("src");
// Scryfall URLs encode the face position (/front/ vs /back/) — the pairing
// proof, not just "the src changed" (Codex mutation pass).
check("the front art really is the front face", /\/front\//.test(frontSrc));
const frontOracle = await page.getByTestId("card-oracle-text").textContent();

await page.getByTestId("card-flip").click();
await page.waitForTimeout(300);
check(
  "flip swaps the heading to the back face",
  (await h1.textContent()) === dfc.faces[1].name,
  await h1.textContent(),
);
check(
  "flip swaps the art to the back face",
  (await art.getAttribute("src")) !== frontSrc &&
    /^https:\/\/cards\.scryfall\.io\/.*\/back\//.test(
      await art.getAttribute("src"),
    ),
);
check(
  "flip swaps the oracle text",
  (await page.getByTestId("card-oracle-text").textContent()) !== frontOracle,
);
await page.getByTestId("card-flip").click();
await page.waitForTimeout(300);
check(
  "a second flip cycles back to the front",
  (await h1.textContent()) === dfc.faces[0].name,
);

// --- catalog tile tap → sheet → flip inside the sheet
await go(`/catalog?q=${encodeURIComponent("Agadeem's Awakening")}`);
await page.getByTestId("card-preview-trigger").first().click();
const sheet = page.locator("[data-testid=card-preview-sheet][role=dialog]");
await sheet.waitFor({ state: "attached", timeout: 5000 }).catch(() => {});
await page.waitForTimeout(400);
check(
  "tapping the DFC tile opens the sheet",
  (await sheet.getAttribute("data-state")) === "open",
  await sheet.getAttribute("data-state"),
);
check(
  "the sheet opens on the front face",
  (await sheet.textContent()).includes(dfc.faces[0].name),
);
await sheet.getByTestId("card-flip").click();
await page.waitForTimeout(300);
check(
  "flipping inside the sheet shows the back face",
  (await sheet.textContent()).includes(dfc.faces[1].name),
);
check(
  "the flip tap neither closed the sheet nor navigated",
  (await sheet.getAttribute("data-state")) === "open" &&
    (await page.evaluate(() => location.pathname)) === "/catalog",
);

// --- an adventure (two faces, one image) gets no flip control
await go(`/cards/${adventure.oracle_id}`);
check(
  "an adventure detail page has no flip control",
  (await page.getByTestId("card-flip").count()) === 0,
);

console.log(
  failures.length
    ? `ANDROID DFC FAIL: ${failures.length} failed`
    : "ANDROID DFC CHECK PASS",
);
process.exit(failures.length ? 1 : 0);
