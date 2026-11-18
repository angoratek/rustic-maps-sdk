// One-off visual check of the playground page: loads it, pans/tilts, and
// saves a screenshot so the tile overlay can be verified by eye.
import { chromium } from '@playwright/test';

const PAGE_URL = 'http://127.0.0.1:8642/';
const SCREENSHOT_PATH = 'tmp/playground-check.png';
const READY_TIMEOUT_MS = 15000;

const browser = await chromium.launch({ channel: 'chromium' });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
await page.goto(PAGE_URL);
await page.waitForFunction(() => window.mapTestReady, null, { timeout: READY_TIMEOUT_MS });
await page.evaluate(() => window.mapTestReady);

await page.fill('#zoom-control', '12');
await page.dispatchEvent('#zoom-control', 'input');
await page.fill('#pitch-control', '45');
await page.dispatchEvent('#pitch-control', 'input');
await page.fill('#bearing-control', '30');
await page.dispatchEvent('#bearing-control', 'input');
const PAN_START = { x: 640, y: 400 };
const PAN_END = { x: 740, y: 460 };
await page.mouse.move(PAN_START.x, PAN_START.y);
await page.mouse.down();
await page.mouse.move(PAN_END.x, PAN_END.y, { steps: 5 });
await page.mouse.up();
await page.screenshot({ path: SCREENSHOT_PATH });
await browser.close();
console.log('saved', SCREENSHOT_PATH);