// Frontend build: esbuild for the JavaScript, Tailwind CLI for the stylesheet,
// plus a plain copy of index.html and the logo assets into dist/.
//
// Deliberately small — no bundler config to maintain, and `node build.mjs`
// produces exactly what Tauri ships.

import * as esbuild from 'esbuild';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import { fileURLToPath } from 'node:url';

const run = promisify(execFile);

// fileURLToPath, not `new URL(...).pathname`. On Windows the latter yields
// "/D:/JWS%20Attendance%20System/..." — a leading slash and percent-encoded
// spaces — which path.join then turns into "D:\D:\JWS%20Attendance...".
const root = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(root, 'src');
const OUT = path.join(root, 'dist');

const watch = process.argv.includes('--watch');
const serve = process.argv.includes('--serve');
const dev = watch || serve;

async function copyStatic() {
  await fs.mkdir(path.join(OUT, 'assets'), { recursive: true });
  await fs.copyFile(path.join(SRC, 'index.html'), path.join(OUT, 'index.html'));
  for (const f of await fs.readdir(path.join(SRC, 'assets'))) {
    await fs.copyFile(path.join(SRC, 'assets', f), path.join(OUT, 'assets', f));
  }
}

async function buildCss() {
  // Invoke Tailwind's JS entry point through node rather than the wrapper in
  // node_modules/.bin. On Windows that wrapper is a .cmd, which recent Node
  // refuses to spawn without a shell; on Linux it is an extensionless script.
  // Going straight to cli.js sidesteps both.
  const cli = path.join(root, 'node_modules', 'tailwindcss', 'lib', 'cli.js');
  const args = [cli, '-i', path.join(SRC, 'styles.css'), '-o', path.join(OUT, 'app.css')];
  if (!dev) args.push('--minify');
  try {
    await run(process.execPath, args);
  } catch (e) {
    // Tailwind writes its progress banner to stderr, so only a non-zero exit
    // with no output file is a real failure.
    try {
      await fs.access(path.join(OUT, 'app.css'));
    } catch {
      throw new Error(`Tailwind build failed:\n${e.stderr || e.message}`);
    }
  }
}

const jsOptions = {
  entryPoints: [path.join(SRC, 'js', 'main.js')],
  bundle: true,
  format: 'iife',
  target: ['chrome110'],
  outfile: path.join(OUT, 'app.js'),
  sourcemap: dev,
  minify: !dev,
  logLevel: 'info',
  legalComments: 'none',
};

await fs.rm(OUT, { recursive: true, force: true });
await copyStatic();
await buildCss();

if (watch) {
  const ctx = await esbuild.context(jsOptions);
  await ctx.watch();

  // Tailwind and the static files do not have a watcher of their own; polling
  // the source directory is plenty for a project this size.
  let last = 0;
  setInterval(async () => {
    const stat = await fs.stat(path.join(SRC, 'styles.css')).catch(() => null);
    const html = await fs.stat(path.join(SRC, 'index.html')).catch(() => null);
    const newest = Math.max(stat?.mtimeMs ?? 0, html?.mtimeMs ?? 0);
    if (newest > last) {
      last = newest;
      await copyStatic();
      await buildCss();
    }
  }, 700);

  if (serve) {
    const port = 5173;
    const types = {
      '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
      '.png': 'image/png', '.svg': 'image/svg+xml', '.map': 'application/json',
    };
    http
      .createServer(async (req, res) => {
        const url = decodeURIComponent((req.url || '/').split('?')[0]);
        const file = path.join(OUT, url === '/' ? 'index.html' : url);
        try {
          const body = await fs.readFile(file);
          res.writeHead(200, { 'Content-Type': types[path.extname(file)] || 'application/octet-stream' });
          res.end(body);
        } catch {
          // Single-page app: unknown paths fall back to the shell.
          const body = await fs.readFile(path.join(OUT, 'index.html'));
          res.writeHead(200, { 'Content-Type': 'text/html' });
          res.end(body);
        }
      })
      .listen(port, () => console.log(`dev server on http://localhost:${port}`));
  }
} else {
  await esbuild.build(jsOptions);
  console.log('frontend built to dist/');
}
