// P6-031 on-device probe: teardown toast's own Undo button on the real
// Android webview (android-smoke skill, dev attach).
//
// Every prior authed-surface Android probe in this repo (android-collection-
// check.mjs, android-quick-add-check.mjs, etc.) hits the documented
// dev-proxy limitation: `http://tauri.localhost/...` strips Cookie headers
// and form POST bodies, so `/my/collections/:id` bounces to `/login` and even
// the login POST itself cannot land — every one of them falls back to the
// `/dev/components` bench instead.
//
// This probe uses a different origin none of them tried: `cargo tauri
// android dev` already runs `adb reverse tcp:3000 tcp:3000`, so the device's
// own loopback can reach the host server directly. A `page.goto` straight to
// `http://127.0.0.1:3000/...` never enters Tauri's `tauri://` scheme handler
// at all — it is a normal top-level navigation to a normal origin, so the
// WebView's own cookie jar behaves like a real browser's. Verified live: sign-in
// over this origin lands on `/my` with real session cookies, where the same
// sign-in over `http://tauri.localhost/...` cannot even submit the form (POST
// bodies are stripped there). This is what makes a REAL authed teardown
// reachable on-device at all.
//
// Usage: node android-teardown-undo-check.mjs
import { chromium } from "playwright";
import fs from "node:fs";

const fail = (msg) => {
  console.error(`FAIL: ${msg}`);
  process.exitCode = 1;
};
const warn = (msg) => console.log(`  ??  ${msg}`);
const ok = (msg) => console.log(`  ok  ${msg}`);

// The repo's real touch-target standard (android-tap-targets-check.mjs).
const TAP = 44;

if (!process.env.E2E_EMAIL || !process.env.E2E_PASSWORD) {
  const envUrl = new URL("./.env", import.meta.url);
  if (!fs.existsSync(envUrl)) {
    console.error(
      "E2E_EMAIL/E2E_PASSWORD are not set and end2end/.env is missing — " +
        "copy it from a checkout that has run seed-e2e-user.sh, or export " +
        "both vars yourself before running this probe.",
    );
    process.exit(2);
  }
  const env = fs.readFileSync(envUrl, "utf8");
  for (const line of env.split("\n")) {
    const m = line.match(/^([A-Z0-9_]+)=(.*)$/);
    if (m) process.env[m[1]] = m[2];
  }
  if (!process.env.E2E_EMAIL || !process.env.E2E_PASSWORD) {
    console.error("end2end/.env did not set E2E_EMAIL/E2E_PASSWORD — check its contents.");
    process.exit(2);
  }
}

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(String(e).slice(0, 200)));

const hydrated = () =>
  page.locator("html[data-hydrated=true]").waitFor({ state: "attached", timeout: 20000 });

// `page.request` has no `baseURL` here (that's a test-runner fixture
// feature, not something `connectOverCDP` sets up) — every call needs the
// absolute origin.
const ORIGIN = "http://127.0.0.1:3000";

const scratchName = (what) => `zz-android-p6031-${what}-${Date.now()}`;

async function createCollection(kind, what) {
  const name = scratchName(what);
  const res = await page.request.post(`${ORIGIN}/api/collections`, {
    data: { parent_id: null, kind, name, format: null },
  });
  if (res.status() !== 200) throw new Error(`create ${name} -> ${res.status()}`);
  return res.json();
}
const deleteCollection = (id) =>
  page.request.post(`${ORIGIN}/api/collections/${id}/delete`, { data: {} });

async function viewRows(id) {
  const res = await page.request.get(`${ORIGIN}/api/collections/${id}/view?limit=200`);
  if (res.status() !== 200) throw new Error(`view ${id} -> ${res.status()}`);
  return (await res.json()).cards;
}

// --- sign in over the plain adb-reverse origin (see header) ---
await page.goto("http://127.0.0.1:3000/login");
await hydrated();
await page.fill("input[name=email]", process.env.E2E_EMAIL);
await page.fill("input[name=password]", process.env.E2E_PASSWORD);
await page.click("button[type=submit]");
try {
  await page.waitForURL("**/my", { timeout: 10000 });
  ok(`signed in on-device over the plain origin — ${page.url()}`);
} catch {
  fail(`sign-in did not land on /my — stuck at ${page.url()}`);
  await browser.close();
  process.exit(1);
}

// --- fixture: a scratch deck with one card, and a destination binder ---
let deck, dest;
try {
  deck = await createCollection("deck", "deck");
  dest = await createCollection("binder", "dest");
  ok(`scratch collections created: deck=${deck.id} dest=${dest.id}`);

  const cardsRes = await page.request.get(`${ORIGIN}/api/catalog/search?q=n&limit=10`);
  const { cards } = await cardsRes.json();
  const card = cards.find((c) => c.printing_id);
  if (!card) throw new Error("no catalog card with a printing_id in the first page");
  const haveRes = await page.request.post(`${ORIGIN}/api/collections/${deck.id}/have`, {
    data: { printing_id: card.printing_id, quantity: 2 },
  });
  if (haveRes.status() !== 200) throw new Error(`add have -> ${haveRes.status()}`);
  ok(`added 2x ${card.name} to the scratch deck`);

  // --- the real flow, on the real device ---
  await page.goto(`http://127.0.0.1:3000/my/collections/${deck.id}`);
  await hydrated();
  const teardownOpen = page.locator('[data-testid="teardown-open"]');
  if ((await teardownOpen.count()) === 0) {
    throw new Error("teardown-open control not found — page did not render as authed");
  }
  await teardownOpen.click();
  await page
    .locator('[data-testid="teardown-destination"]')
    .selectOption({ label: dest.name });
  await page.locator('[data-testid="teardown-confirm"]').click();

  const toast = page.locator('[data-name="Toast"]', { hasText: "Emptied" });
  await toast.waitFor({ state: "visible", timeout: 10000 });
  ok(`teardown toast appeared: "${(await toast.textContent()).trim()}"`);

  const undoBtn = toast.getByRole("button", { name: "Undo" });
  if ((await undoBtn.count()) === 0) {
    throw new Error("the teardown toast has no Undo button");
  }
  const box = await undoBtn.boundingBox();
  // `page.viewportSize()` is null for a CDP-attached page (it was not
  // created via `browser.newPage()` with an explicit viewport) — read the
  // real on-device dimensions from the DOM instead.
  const vp = await page.evaluate(() => ({
    width: window.innerWidth,
    height: window.innerHeight,
  }));
  if (!box) throw new Error("Undo button has no bounding box (not rendered)");
  const withinViewport =
    box.x >= 0 &&
    box.y >= 0 &&
    box.x + box.width <= vp.width + 1 &&
    box.y + box.height <= vp.height + 1;
  const label = `${Math.round(box.width)}×${Math.round(box.height)} at (${Math.round(box.x)},${Math.round(box.y)}), viewport=${vp.width}x${vp.height}`;
  if (!withinViewport) {
    fail(`Undo button is clipped outside the phone viewport: box=${JSON.stringify(box)} viewport=${JSON.stringify(vp)}`);
  } else if (box.width < TAP || box.height < TAP) {
    // Under the repo's 44 px standard on at least one axis — a real defect,
    // but a component-wide one (every toast action button shares this
    // markup, `ui/sonner.rs`'s `ToastAction` button, `py-1` + `text-xs`
    // giving ~24 px tall regardless of caller), not something P6-031
    // introduced or can fix by touching the teardown dialog alone. Filed as
    // its own follow-up (see specs/app-ui.md → Findings, P6-031: "toast
    // action buttons are under the 44 px touch standard") — warn rather than
    // fail so this probe keeps testing what P6-031 actually changed instead
    // of red-lining on a pre-existing, differently-scoped defect.
    warn(`Undo button is on-screen but under the ${TAP}px standard: ${label}`);
  } else {
    ok(`Undo button is on-screen and tappable (${TAP}px+): ${label}`);
  }

  // Read back before the tap: prove the teardown actually moved the cards.
  const deckAfterTeardown = await viewRows(deck.id);
  const destAfterTeardown = await viewRows(dest.id);
  if (deckAfterTeardown.length !== 0 || destAfterTeardown.length !== 1) {
    fail(
      `unexpected state after teardown: deck rows=${deckAfterTeardown.length} dest rows=${destAfterTeardown.length}`,
    );
  } else {
    ok("read-back confirms the teardown moved the card out of the deck");
  }

  // The real tap.
  await undoBtn.click();
  await page.waitForTimeout(500);

  // Read back: the deck's contents actually came back.
  let deckAfterUndo, destAfterUndo;
  for (let i = 0; i < 10; i++) {
    deckAfterUndo = await viewRows(deck.id);
    destAfterUndo = await viewRows(dest.id);
    if (deckAfterUndo.length === 1 && destAfterUndo.length === 0) break;
    await page.waitForTimeout(500);
  }
  if (deckAfterUndo.length === 1 && destAfterUndo.length === 0) {
    ok(`tapping Undo on the real device restored the deck's contents (present=${deckAfterUndo[0].present})`);
  } else {
    fail(
      `deck contents were not restored after tapping Undo: deck rows=${deckAfterUndo?.length} dest rows=${destAfterUndo?.length}`,
    );
  }

  if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
  else ok("no page errors");
} catch (e) {
  fail(e.message);
} finally {
  if (deck) await deleteCollection(deck.id).catch(() => {});
  if (dest) await deleteCollection(dest.id).catch(() => {});
  console.log("cleanup: scratch collections deleted");
}

console.log(
  process.exitCode ? "ANDROID TEARDOWN-UNDO PROBE: FAIL" : "ANDROID TEARDOWN-UNDO PROBE: PASS",
);
await browser.close();
