// Ad-hoc reset-flow UI drive (not a test): open /login, enter the reset
// card, request a code, and assert the OTP step renders — the backend cycle
// is verified separately at the API level. Debugging aid; safe to delete.
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:3456';
const email = process.argv[3] ?? 'dylan.goings+reset-test-0713@atomicobject.com';

const browser = await chromium.launch();
const page = await browser.newPage();
const errors = [];
page.on('console', (m) => m.type() === 'error' && errors.push(m.text().slice(0, 200)));

await page.goto(`${base}/login`, { waitUntil: 'networkidle' });
await page.click('text=Forgot password?');
await page.waitForSelector('text=Reset password', { timeout: 5000 });
console.log('reset card rendered: YES');

await page.fill('input[name=email]', email);
await page.click('text=Email me a reset code');
await page.waitForSelector(`text=We sent a reset code to`, { timeout: 10000 });
const step2 =
  (await page.isVisible('input[name=otp]')) && (await page.isVisible('input[name=password]'));
console.log('otp + new-password step rendered:', step2 ? 'YES' : 'NO');

await page.click('text=← Back to sign in');
await page.waitForSelector('text=Sign in', { timeout: 5000 });
console.log('back-to-sign-in works: YES');

console.log('console errors:', errors.length ? errors : 'none');
await browser.close();
process.exit(errors.length ? 1 : 0);
