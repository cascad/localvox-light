// Render the actual ConPTY captures from the preview integration tests.
// Uses the existing local xterm/Playwright/font dependencies; never downloads them.
import { createRequire } from 'node:module';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { startFixture } from '../promo/fixture.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const require = createRequire(path.join(root, 'ui/package.json'));
const { chromium } = require('@playwright/test');
const output = path.join(root, 'target/review/installer-preview');
const captures = path.join(output, 'captures');
const frames = path.join(output, 'frames');
await mkdir(frames, { recursive: true });
const fixture = await startFixture();
let browser;
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1240, height: 930 }, deviceScaleFactor: 1 });
  const steps = [
    ['fresh-welcome', 'Enter', 'Оставь рекомендуемую установку или выбери компоненты'],
    ['components-wide', '↑ ↓ / Пробел', 'Дополнительные возможности выбираются галочками'],
    ['summary-source', '↑ ↓ / Enter', 'Ollama, собственный сервер или готовый файл модели'],
    ['folders', 'Enter', 'Папки предложены заранее; при желании можно изменить'],
    ['custom-paths-review', 'Enter / Esc', 'Перед действиями — итоговый план или отмена'],
  ];
  await page.goto(fixture.address + '/terminal?cols=110&rows=36');
  await page.evaluate(() => window.ready);
  await page.evaluate(() => {
    document.querySelector('.brand').textContent = 'localvox / установка';
    document.querySelector('.demo').textContent = 'Безопасное демо · ничего не устанавливает';
    document.querySelector('.chrome span:nth-child(2)').textContent = 'localvox-setup-preview';
  });
  for (let index = 0; index < steps.length; index++) {
    const [name, keys, caption] = steps[index];
    const bytes = [...await readFile(path.join(captures, name + '.ansi'))];
    await page.evaluate(async ({ bytes, keys, caption }) => {
      window.term.reset();
      await window.writeTerminal(bytes);
      document.getElementById('keys').textContent = keys;
      document.getElementById('caption').textContent = caption;
    }, { bytes, keys, caption });
    await writeFile(path.join(output, name + '.rendered.txt'), await page.evaluate(() =>
      Array.from({ length: term.rows }, (_, row) => term.buffer.active.getLine(row)?.translateToString(true)).join('\n')));
    await page.screenshot({ path: path.join(output, name + '.png') });
    await page.screenshot({ path: path.join(frames, String(index).padStart(2, '0') + '.png') });
  }
  await promisify(execFile)('ffmpeg', [
    '-hide_banner', '-loglevel', 'error', '-y', '-framerate', '1/2',
    '-i', path.join(frames, '%02d.png'),
    '-filter_complex', '[0:v]split[a][b];[a]palettegen=stats_mode=diff:max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=3',
    '-loop', '0', path.join(output, 'installer-preview.gif'),
  ], { windowsHide: true, timeout: 60000 });
  console.log('Rendered 5 terminal screens and installer-preview.gif in ' + output);
} finally {
  await browser?.close();
  await fixture.close();
}
