// Checks app/engine/qr.h against the `qrcode` npm package (an independent encoder): same text, level M, every mask ->
// identical modules. Usage: node qr_compare.mjs <qr_dump.exe> [path to a node_modules folder containing qrcode]
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const [exe, modules] = process.argv.slice(2);
const require = createRequire(resolve(modules ?? '.', 'x.js'));
const QRCode = require('qrcode');
const texts = [
  'https://pokemon-reloaded.blogspot.com/',
  'a',
  'https://example.org/a/very/long/path/to/make/the/code/grow/into/a/bigger/version/of/the/symbol?x=abcdefghij',
  'x'.repeat(120),
  'y'.repeat(190),
];
let failures = 0;
for (const text of texts) {
  const out = execFileSync(exe, [text], { encoding: 'utf8' }).trim().split('\n');
  let line = 0;
  for (let mask = 0; mask < 8; mask++) {
    const [, , , size] = out[line++].split(' ');
    const rows = out.slice(line, line + Number(size));
    line += Number(size);
    const reference = QRCode.create(text, { errorCorrectionLevel: 'M', maskPattern: mask });
    const refSize = reference.modules.size;
    let same = refSize === Number(size);
    for (let y = 0; same && y < refSize; y++)
      for (let x = 0; x < refSize; x++)
        if ((rows[y][x] === '1') !== !!reference.modules.get(y, x)) { same = false; break; }
    if (!same) { failures++; console.log(`DIFF len=${text.length} mask=${mask} ours=${size} ref=${refSize}`); }
  }
  console.log(`len ${text.length}: version ${(Number(out[0].split(' ')[3]) - 17) / 4}, 8 masks compared`);
}
console.log(failures ? `${failures} differences` : 'qr.h matches the reference encoder');
process.exit(failures ? 1 : 0);
