// Guards on the build script itself.
//
// These exist because of a real failure: build.mjs resolved its own location
// with `new URL(import.meta.url).pathname`, which on Windows yields
// "/D:/JWS%20Attendance%20System/..." — a leading slash and percent-encoded
// spaces. path.join then produced "D:\D:\JWS%20Attendance..." and the build
// died with ENOENT on a school PC. It worked perfectly on Linux, so only a
// test that pins the behaviour keeps it fixed.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';

const run = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('build.mjs does not resolve its own path via URL.pathname', async () => {
  const src = await fs.readFile(path.join(root, 'build.mjs'), 'utf8');
  assert.ok(
    src.includes('fileURLToPath'),
    'build.mjs must use fileURLToPath to locate itself',
  );
  assert.ok(
    !/new URL\(import\.meta\.url\)\.pathname/.test(src),
    'URL.pathname is percent-encoded and prefixed with / on Windows — use fileURLToPath',
  );
});

test('no source file resolves paths via URL.pathname', async () => {
  // Same trap, anywhere else in the project.
  const files = [];
  async function walk(dir) {
    for (const e of await fs.readdir(dir, { withFileTypes: true })) {
      if (['node_modules', 'target', 'dist', '.git'].includes(e.name)) continue;
      // This file names the pattern in order to forbid it.
      if (e.name === 'build.test.js') continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) await walk(p);
      else if (/\.(mjs|js)$/.test(e.name)) files.push(p);
    }
  }
  await walk(root);

  const offenders = [];
  for (const f of files) {
    const src = await fs.readFile(f, 'utf8');
    if (/new URL\([^)]*import\.meta\.url[^)]*\)\.pathname/.test(src)) {
      offenders.push(path.relative(root, f));
    }
  }
  assert.deepEqual(offenders, [], 'these break on Windows paths with spaces');
});

test('build runs from a different working directory', async () => {
  // The original bug only showed when paths were joined wrongly. Running from
  // elsewhere proves the script anchors to its own location, not the cwd.
  const elsewhere = await fs.mkdtemp(path.join(os.tmpdir(), 'jws-build-'));
  try {
    await run(process.execPath, [path.join(root, 'build.mjs')], { cwd: elsewhere });

    for (const f of ['index.html', 'app.js', 'app.css', 'assets/crest.png']) {
      const p = path.join(root, 'dist', f);
      await assert.doesNotReject(fs.access(p), `dist/${f} should exist`);
    }
    // Nothing may be written next to the caller.
    const leaked = await fs.readdir(elsewhere);
    assert.deepEqual(leaked, [], 'build must not write into the working directory');
  } finally {
    await fs.rm(elsewhere, { recursive: true, force: true });
  }
});

test('the built page references the bundles it needs', async () => {
  const html = await fs.readFile(path.join(root, 'dist', 'index.html'), 'utf8');
  assert.ok(html.includes('app.css'));
  assert.ok(html.includes('app.js'));
  assert.ok(html.includes('assets/crest.png'));
});

test('the stylesheet carries the school colours', async () => {
  const css = await fs.readFile(path.join(root, 'dist', 'app.css'), 'utf8');
  // Tailwind lowercases hex values, so compare case-insensitively.
  assert.match(css, /#f16522/i, 'brand orange must survive the Tailwind build');
  assert.ok(css.length > 5000, 'stylesheet looks truncated');
});

test('every file that carries a version agrees', async () => {
  // npm ci fails outright when package.json and package-lock.json disagree,
  // and the updater compares the tauri.conf.json version against what a PC
  // already has. A mismatch between any of these breaks either the build or
  // the update check, both silently enough to waste an afternoon.
  const pkg  = JSON.parse(await fs.readFile(path.join(root, 'package.json'), 'utf8'));
  const lock = JSON.parse(await fs.readFile(path.join(root, 'package-lock.json'), 'utf8'));
  const conf = JSON.parse(await fs.readFile(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const cargo = await fs.readFile(path.join(root, 'Cargo.toml'), 'utf8');
  const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

  assert.equal(lock.version, pkg.version, 'package-lock.json is out of step with package.json');
  assert.equal(lock.packages[''].version, pkg.version, 'lockfile root package version differs');
  assert.equal(conf.version, pkg.version, 'tauri.conf.json version differs from package.json');
  assert.equal(cargoVersion, pkg.version, 'Cargo.toml version differs from package.json');
});

test('updater is configured to actually produce its artifacts', async () => {
  // Tauri 2 emits no .sig and no latest.json unless this is switched on, and
  // the app then reports "could not fetch a valid release JSON" forever.
  const conf = JSON.parse(await fs.readFile(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  assert.equal(conf.bundle.createUpdaterArtifacts, true, 'updater artifacts are disabled');

  const pubkey = conf.plugins?.updater?.pubkey ?? '';
  assert.ok(pubkey.length > 40, 'updater public key is missing');
  assert.ok(!pubkey.includes('REPLACE'), 'updater still has the placeholder public key');

  const endpoint = conf.plugins?.updater?.endpoints?.[0] ?? '';
  assert.ok(endpoint.includes('sujanrajgiri019-cmyk/jws-attendance-suit'), 'endpoint points elsewhere');
  assert.ok(endpoint.endsWith('latest.json'), 'endpoint must point at latest.json');
});
