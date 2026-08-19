import { defineConfig, devices } from '@playwright/test';
import fs from 'node:fs';

/** The container's own Chromium, if one is installed. */
const chromium = (() => {
  if (process.env.PLAYWRIGHT_CHROMIUM_PATH) return process.env.PLAYWRIGHT_CHROMIUM_PATH;
  const root = process.env.PLAYWRIGHT_BROWSERS_PATH || '/opt/pw-browsers';
  try {
    const dir = fs.readdirSync(root).find((d) => /^chromium-\d+$/.test(d));
    const exe = dir && `${root}/${dir}/chrome-linux/chrome`;
    return exe && fs.existsSync(exe) ? exe : null;
  } catch {
    return null;
  }
})();

// The end-to-end suite drives the built frontend against the in-memory demo
// backend. That is enough to catch the failure mode that matters most here —
// a screen that throws and renders nothing — without needing a Windows box or
// a fingerprint terminal on the desk.

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: true,
  reporter: process.env.CI ? 'list' : [['list']],
  timeout: 30_000,
  use: {
    baseURL: 'http://localhost:5174',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1600, height: 1000 },
        // The interface animates deliberately. Those animations are decorative,
        // and asking the browser to skip them makes the suite deterministic
        // instead of racing element-stability checks against a fade.
        reducedMotion: 'reduce',
        // Use the Chromium that ships with the container image when present.
        // Playwright looks for a headless-shell build that the image does not
        // carry, so point it at the full browser that is actually installed
        // rather than asking every developer to re-download one.
        launchOptions: chromium ? { executablePath: chromium } : {},
      },
    },
  ],
  webServer: {
    command: 'node tests/e2e/serve.mjs',
    url: 'http://localhost:5174',
    reuseExistingServer: !process.env.CI,
    timeout: 20_000,
  },
});
