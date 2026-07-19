// Ad-hoc auth UI drive (not a test): sign in through the real /login form,
// assert the shell user menu, then sign out. Debugging aid; safe to delete.
import { chromium } from 'playwright';

const [base, email, password] = process.argv.slice(2);
const browser = await chromium.launch();
const page = await browser.newPage();
const errors = [];
page.on('console', (m) => m.type() === 'error' && errors.push(m.text().slice(0, 200)));

await page.goto(`${base}/login`, { waitUntil: 'networkidle' });
await page.fill('input[name=email]', email);
await page.fill('input[name=password]', password);
await page.click('button[type=submit]');
// Sign-in lands on / whose redirect sends authed sessions to /my.
await page.waitForURL(`${base}/my`, { timeout: 10000 });
await page.click('button[aria-label="Account menu"]');
await page.waitForSelector(`text=Signed in as`, { timeout: 10000 });
const body = await page.textContent('body');
const signedIn = body.includes(`Signed in as ${email}`);
console.log('signed-in user menu:', signedIn ? 'YES' : `NO — body: ${body.slice(-200)}`);

await page.click('text=Sign out');
// Sign-out navigates to /catalog; the top bar then offers the sign-in link.
await page.waitForURL(`${base}/catalog`, { timeout: 10000 });
const after = await page.textContent('header');
console.log('after sign-out shows sign-in link:', after.includes('Sign in') ? 'YES' : 'NO');
console.log('console errors:', errors.length ? errors.join(' | ') : 'none');
await browser.close();
