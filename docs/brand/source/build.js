// Renders the Pindu launcher artwork from scene.html, the app icon in data/icons/ and two of the README's
// screenshots in docs/screenshots/.
//
//   node build.js                 the files kept in docs/brand/
//   node build.js --extras <dir>  a link-preview image, an article cover and two lockups, into <dir>
//
// Needs Node.js with Playwright (Chromium) and the Inter and Inter Display fonts installed.
// See ../README.md.
const { chromium } = require('playwright');
const path = require('path');
const fs = require('fs');

const SUB = 'Play all your games';
const LINE = 'Every PC game on your machine in one library,\nat your desk or from the couch';

// w and h are CSS pixels; the file is w*scale by h*scale. `cls` selects a layout in scene.html, and `screens`
// adds the desktop window and the television.
const brand = [
  { out: 'readme-banner.webp', w: 1280, h: 400, scale: 2, cls: 'banner rounded', sub: SUB, line: LINE, screens: 1, transparent: true },
  { out: 'social-preview.png', w: 1280, h: 640, scale: 1, cls: 'social', sub: SUB, line: LINE, screens: 1 },
];
const extras = [
  { out: 'og-image.png', w: 1200, h: 630, scale: 1, cls: 'social og', sub: SUB, line: LINE, screens: 1 },
  { out: 'article-cover.png', w: 1280, h: 720, scale: 1.5, cls: 'social cover', sub: SUB, line: LINE, screens: 1 },
  { out: 'lockup-for-light.png', w: 1160, h: 320, scale: 1, cls: 'lockup', nobg: 1, transparent: true },
  { out: 'lockup-for-dark.png', w: 1160, h: 320, scale: 1, cls: 'lockup dark', nobg: 1, transparent: true },
];

// Chromium screenshots are PNG; it re-encodes one as WebP, alpha included, on a canvas.
async function toWebp(browser, png) {
  const page = await browser.newPage();
  const dataUrl = await page.evaluate(async (src) => {
    const img = new Image();
    img.src = src;
    await img.decode();
    const canvas = document.createElement('canvas');
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    canvas.getContext('2d').drawImage(img, 0, 0);
    return canvas.toDataURL('image/webp', 0.92);
  }, 'data:image/png;base64,' + png.toString('base64'));
  await page.close();
  return Buffer.from(dataUrl.split(',')[1], 'base64');
}

(async () => {
  const extrasAt = process.argv.indexOf('--extras');
  const jobs = extrasAt === -1 ? brand : extras;
  const outDir = extrasAt === -1 ? path.join(__dirname, '..') : process.argv[extrasAt + 1];
  if (!outDir) throw new Error('--extras needs a directory');
  fs.mkdirSync(outDir, { recursive: true });

  const browser = await chromium.launch();
  for (const job of jobs) {
    const page = await browser.newPage({
      viewport: { width: job.w, height: job.h },
      deviceScaleFactor: job.scale,
    });
    const query = new URLSearchParams({ w: job.w, h: job.h, cls: job.cls });
    for (const key of ['sub', 'line', 'screens', 'nobg']) {
      if (job[key]) query.set(key, job[key]);
    }
    await page.goto('file://' + path.join(__dirname, 'scene.html') + '?' + query);
    await page.evaluate(() => document.fonts.ready);
    let image = await page.screenshot({ omitBackground: Boolean(job.transparent) });
    await page.close();
    if (job.out.endsWith('.webp')) image = await toWebp(browser, image);
    fs.writeFileSync(path.join(outDir, job.out), image);
    console.log(job.out);
  }
  await browser.close();
})();
