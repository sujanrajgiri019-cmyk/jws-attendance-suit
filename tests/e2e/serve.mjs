// Minimal static server for the end-to-end suite. Serves dist/, building it
// first if it is missing so `npm run test:ui` works from a clean checkout.

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const OUT = path.join(root, 'dist');

if (!fs.existsSync(path.join(OUT, 'app.js'))) {
  execFileSync(process.execPath, [path.join(root, 'build.mjs')], { cwd: root, stdio: 'inherit' });
}

const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
  '.png': 'image/png', '.svg': 'image/svg+xml', '.map': 'application/json',
};

http.createServer((req, res) => {
  const url = decodeURIComponent((req.url || '/').split('?')[0]);
  const file = path.join(OUT, url === '/' ? 'index.html' : url);
  fs.readFile(file, (err, body) => {
    if (err) {
      res.writeHead(404, { 'Content-Type': 'text/plain' });
      return res.end('not found');
    }
    res.writeHead(200, { 'Content-Type': TYPES[path.extname(file)] || 'application/octet-stream' });
    res.end(body);
  });
}).listen(5174, () => console.log('e2e server on http://localhost:5174'));
