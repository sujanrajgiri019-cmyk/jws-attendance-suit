import { defineConfig, devices } from '@playwright/test';

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
        // Use the Chromium that ships with the container image when present.
        launchOptions: process.env.PLAYWRIGHT_CHROMIUM_PATH
          ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH }
          : {},
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
