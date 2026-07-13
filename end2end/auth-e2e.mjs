// Ad-hoc auth UI drive (not a test): sign in through the real /login form,
// assert the footer status, then sign out. Debugging aid; safe to delete.
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
await page.waitForURL(`${base}/`, { timeout: 10000 });
await page.waitForSelector(`text=Signed in as`, { timeout: 10000 });
const footer = await page.textContent('body');
const signedIn = footer.includes(`Signed in as ${email}`);
console.log('signed-in footer:', signedIn ? 'YES' : `NO — footer: ${footer.slice(-200)}`);

await page.click('text=Sign out');
await page.waitForTimeout(1500);
const after = await page.textContent('body');
console.log('after sign-out shows links:', after.includes('Sign in') && after.includes('Sign up') ? 'YES' : 'NO');
console.log('console errors:', errors.length ? errors.join(' | ') : 'none');
await browser.close();
