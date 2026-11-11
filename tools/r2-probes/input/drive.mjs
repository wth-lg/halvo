import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright';

const [url, mode] = process.argv.slice(2);
if (!url || !['playwright', 'hold'].includes(mode)) {
  console.error('usage: node drive.mjs URL playwright|hold');
  process.exit(2);
}
const version = JSON.parse(
  readFileSync(new URL('./node_modules/playwright/package.json', import.meta.url), 'utf8'),
).version;
const browser = await chromium.launch({
  headless: false,
  args: ['--window-position=1160,40', '--window-size=740,620'],
});
const page = await (await browser.newContext({ viewport: null })).newPage();
await page.goto(url);
await page.bringToFront();
await page.locator('#box').focus();
const geometry = await page.evaluate(() => {
  const border = (window.outerWidth - window.innerWidth) / 2;
  const top = window.outerHeight - window.innerHeight - border;
  const centre = (selector) => {
    const rect = document.querySelector(selector).getBoundingClientRect();
    const scale = window.devicePixelRatio;
    return {
      screenX: Math.round((window.screenX + border + rect.left + rect.width / 2) * scale),
      screenY: Math.round((window.screenY + top + rect.top + rect.height / 2) * scale),
    };
  };
  return { hasFocus: document.hasFocus(), box: centre('#box'), button: centre('#button') };
});
console.log(JSON.stringify({ phase: 'geometry', version, ...geometry }));
if (mode === 'playwright') {
  for (let index = 0; index < 20; index += 1) {
    await page.keyboard.press('a', { delay: 20 });
  }
  for (let index = 0; index < 5; index += 1) {
    await page.locator('#button').click({ delay: 20 });
  }
} else {
  await new Promise((resolve) => createInterface({ input: process.stdin }).once('line', resolve));
}
console.log(JSON.stringify({ phase: 'counts', ...(await page.evaluate(() => window.counted)) }));
await browser.close();
process.exit(0);
