// Ad-hoc hydration-error detector (not a test): loads each URL, waits for the
// wasm to hydrate, and prints any console errors/warnings. Used while
// debugging the auth-page hydration regression; safe to delete.
import { chromium } from 'playwright';

const urls = process.argv.slice(2);
const browser = await chromium.launch();
for (const url of urls) {
  const page = await browser.newPage();
  const messages = [];
  page.on('console', (m) => {
    if (m.type() === 'error' || m.type() === 'warning') {
      messages.push(`${m.type()}: ${m.text().slice(0, 300)}`);
    }
  });
  page.on('pageerror', (e) => messages.push(`pageerror: ${String(e).slice(0, 300)}`));
  try {
    await page.goto(url, { waitUntil: 'networkidle', timeout: 20000 });
    await page.waitForTimeout(1500);
  } catch (e) {
    messages.push(`nav-error: ${String(e).slice(0, 200)}`);
  }
  console.log(`\n=== ${url}`);
  console.log(messages.length ? messages.join('\n') : 'CLEAN — no console errors/warnings');
  await page.close();
}
await browser.close();
