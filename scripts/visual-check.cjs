#!/usr/bin/env node
'use strict';

// Optional browser QA; no packages are added to the renderer's dependencies.
//   node scripts/visual-check.cjs [/tmp/tgw-visual]
// Set PLAYWRIGHT_MODULE to an installed Playwright module/package path and
// BROWSER_EXECUTABLE to a Chromium-family executable when using local installs.

const fs = require('node:fs/promises');
const path = require('node:path');

function escapeHtml(value) {
  return value.replace(/[&<>"']/g, character => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[character]);
}

async function main(outputDirectory = process.argv[2] || '/tmp/tgw-visual', extras = process.argv.slice(3)) {
  const directory = path.resolve(outputDirectory);
  const fixturesDirectory = path.resolve(__dirname, '../tests/fixtures');
  const names = (await fs.readdir(fixturesDirectory)).filter(name => name.endsWith('.svg')).sort();
  const fixtures = await Promise.all(names.map(async name => ({
    name,
    source: await fs.readFile(path.join(fixturesDirectory, name), 'utf8'),
  })));
  for (const extra of extras) {
    fixtures.push({
      name: path.basename(extra),
      source: await fs.readFile(extra, 'utf8'),
    });
  }
  if (!fixtures.length) throw new Error(`No SVG fixtures in ${fixturesDirectory}`);
  const playwright = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
  const browser = await playwright.chromium.launch({
    headless: true,
    ...(process.env.BROWSER_EXECUTABLE ? { executablePath: process.env.BROWSER_EXECUTABLE } : {}),
  });
  const failures = [];
  const results = [];
  await fs.mkdir(directory, { recursive: true });
  try {
    const validationContext = await browser.newContext();
    const validationPage = await validationContext.newPage();
    const owners = new Map();
    for (const fixture of fixtures) {
      const result = await validationPage.evaluate(source => {
        const document = new DOMParser().parseFromString(source, 'image/svg+xml');
        const parserError = document.querySelector('parsererror');
        if (parserError) return { errors: [parserError.textContent.trim()], ids: [] };
        const svg = document.documentElement;
        if (svg.localName !== 'svg' || svg.namespaceURI !== 'http://www.w3.org/2000/svg') {
          return { errors: ['Root must be an SVG element in the SVG namespace'], ids: [] };
        }
        const errors = [];
        const ids = new Set();
        for (const element of document.querySelectorAll('[id]')) {
          const id = element.id;
          if (ids.has(id)) errors.push(`Duplicate id within document: ${id}`);
          ids.add(id);
        }
        const checkReferences = value => {
          for (const match of value.matchAll(/url\(\s*["']?#([^"')\s]+)["']?\s*\)/g)) {
            if (!ids.has(match[1])) errors.push(`Unresolved local paint/clip reference: #${match[1]}`);
          }
        };
        for (const element of document.querySelectorAll('*')) {
          for (const attribute of element.attributes) {
            checkReferences(attribute.value);
            if (attribute.localName === 'href' && attribute.value.startsWith('#')) {
              if (!ids.has(attribute.value.slice(1))) errors.push(`Unresolved href: ${attribute.value}`);
            }
            if (attribute.localName === 'aria-labelledby') {
              for (const id of attribute.value.trim().split(/\s+/)) {
                if (!ids.has(id)) errors.push(`Unresolved accessible label: #${id}`);
              }
            }
          }
          if (element.localName === 'style') checkReferences(element.textContent);
        }
        return {
          errors,
          ids: [...ids],
          paths: document.querySelectorAll('path').length,
          patterns: document.querySelectorAll('pattern').length,
          elements: document.querySelectorAll('*').length,
        };
      }, fixture.source);
      for (const error of result.errors) failures.push(`${fixture.name}: ${error}`);
      for (const id of result.ids) {
        const owner = owners.get(id);
        if (owner && owner.source !== fixture.source) {
          failures.push(`${fixture.name}: id ${id} collides with different document ${owner.name}`);
        } else if (!owner) {
          owners.set(id, fixture);
        }
      }
      results.push({ name: fixture.name, ...result, bytes: Buffer.byteLength(fixture.source), scales: [] });
    }
    await validationContext.close();

    for (const scale of [1, 2]) {
      const context = await browser.newContext({
        viewport: { width: 1600, height: 1200 },
        deviceScaleFactor: scale,
        colorScheme: 'light',
        reducedMotion: 'reduce',
      });
      const page = await context.newPage();
      for (const [index, fixture] of fixtures.entries()) {
        if (results[index].errors.length) continue;
        await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>html,body{margin:0;padding:0;background:#fff}body>svg{display:block}</style></head><body>${fixture.source}</body></html>`);
        await page.evaluate(() => document.fonts.ready);
        const layout = await page.evaluate(() => {
          const svg = document.querySelector('body > svg');
          const canvas = svg.getBoundingClientRect();
          const tolerance = 1.5; // CSS pixels, independent of device scale.
          const outside = [];
          let checkedText = 0;
          let clippedText = 0;
          for (const text of svg.querySelectorAll('text')) {
            if (!text.textContent.trim()) continue;
            let clipped = false;
            for (let parent = text; parent && parent !== svg; parent = parent.parentElement) {
              if (getComputedStyle(parent).clipPath !== 'none') {
                clipped = true;
                break;
              }
            }
            // Bus/node labels can intentionally be clipped to a waveform
            // viewport. Names, captions, group labels, and ticks must fit.
            if (clipped) {
              clippedText += 1;
              continue;
            }
            const bounds = text.getBoundingClientRect();
            if (!bounds.width || !bounds.height || getComputedStyle(text).visibility === 'hidden') continue;
            checkedText += 1;
            if (bounds.left < canvas.left - tolerance || bounds.top < canvas.top - tolerance ||
                bounds.right > canvas.right + tolerance || bounds.bottom > canvas.bottom + tolerance) {
              outside.push({
                text: text.textContent,
                x: bounds.left - canvas.left,
                y: bounds.top - canvas.top,
                width: bounds.width,
                height: bounds.height,
              });
            }
          }
          return { width: canvas.width, height: canvas.height, checkedText, clippedText, outside };
        });
        for (const text of layout.outside) {
          failures.push(`${fixture.name} at ${scale}x: text outside canvas: ${JSON.stringify(text)}`);
        }
        if (!(layout.width > 0 && layout.height > 0)) {
          failures.push(`${fixture.name} at ${scale}x: empty canvas`);
        } else {
          await page.locator('body > svg').screenshot({
            path: path.join(directory, `${path.basename(fixture.name, '.svg')}@${scale}x.png`),
            animations: 'disabled',
            scale: 'device',
          });
        }
        results[index].scales.push({ scale, ...layout });
      }
      await context.close();
    }

    const galleryContext = await browser.newContext({
      viewport: { width: 1200, height: 1000 },
      deviceScaleFactor: 1,
      colorScheme: 'light',
      reducedMotion: 'reduce',
    });
    const galleryPage = await galleryContext.newPage();
    const gallery = fixtures.map(fixture => `<section><h2>${escapeHtml(fixture.name)}</h2>${fixture.source}</section>`).join('');
    await galleryPage.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>body{margin:24px;background:#eef1f5;font:13px system-ui}section{background:#fff;padding:20px;margin-bottom:20px;border-radius:8px;width:max-content;max-width:calc(100% - 40px)}h2{font-size:12px;letter-spacing:1px;color:#64748b;margin:0 0 18px}section>svg{display:block;max-width:100%;height:auto}</style></head><body>${gallery}</body></html>`);
    await galleryPage.evaluate(() => document.fonts.ready);
    await galleryPage.screenshot({ path: path.join(directory, 'gallery.png'), fullPage: true, animations: 'disabled' });
    await galleryContext.close();
  } finally {
    await browser.close();
  }
  const report = { browser: browser.browserType().name(), fixtures: results, failures };
  await fs.writeFile(path.join(directory, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`${fixtures.length} SVG fixtures checked at 1x and 2x; screenshots and report: ${directory}`);
  if (failures.length) {
    for (const failure of failures) console.error(failure);
    throw new Error(`Visual validation failed with ${failures.length} issue(s)`);
  }
  console.log('XML, fragment references, document IDs, and unclipped text bounds passed.');
  return report;
}

module.exports = { main };
if (require.main === module) {
  main().catch(error => {
    console.error(error.stack || error.message);
    process.exitCode = 1;
  });
}
