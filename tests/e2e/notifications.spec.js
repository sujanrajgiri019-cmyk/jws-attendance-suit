import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (e) => { throw new Error('Uncaught: ' + e.message); });
  await page.goto('/');
  await page.waitForSelector('#nav a', { timeout: 15000 });
});

test('bell opens and closes the activity tray', async ({ page }) => {
  await expect(page.locator('.notif-panel')).toHaveCount(0);
  await page.click('#btnBell');
  await expect(page.locator('.notif-panel')).toBeVisible();
  await expect(page.locator('.notif-panel')).toContainText('Activity');
  await page.click('#btnBell');
  await expect(page.locator('.notif-panel')).toHaveCount(0);
});

test('tray shows an empty state before anything happens', async ({ page }) => {
  await page.click('#btnBell');
  await expect(page.locator('.notif-panel .empty')).toBeVisible();
  await expect(page.locator('.notif-panel')).toContainText('No activity yet');
});

test('notifications appear in the tray and raise a badge', async ({ page }) => {
  await expect(page.locator('#bellDot')).toBeHidden();

  await page.evaluate(() => {
    window.__jwsNotify('punch', 'Sarita Maharjan checked in', '08:55');
    window.__jwsNotify('punch', 'Bikash Shrestha checked in', '08:57');
    window.__jwsNotify('device', 'Terminal connected', 'GED7253800740');
  });

  const dot = page.locator('#bellDot');
  await expect(dot).toBeVisible();
  await expect(dot).toHaveText('3');

  await page.click('#btnBell');
  await expect(page.locator('.notif')).toHaveCount(3);
  await expect(page.locator('.notif-panel')).toContainText('Sarita Maharjan checked in');
  await expect(page.locator('.notif-panel')).toContainText('GED7253800740');

  // Opening the tray marks everything as seen.
  await expect(dot).toBeHidden();
});

test('a burst of scans does not flood the screen with toasts', async ({ page }) => {
  // The whole point of the tray: forty people at the gate must not cover the
  // interface with forty pop-ups.
  await page.evaluate(() => {
    for (let i = 1; i <= 40; i++) {
      window.__jwsNotify('punch', `Enrolment ${i} checked in`, '08:0' + (i % 10));
    }
  });
  await expect(page.locator('.toast')).toHaveCount(0);
  await expect(page.locator('#bellDot')).toHaveText('40');

  await page.click('#btnBell');
  await expect(page.locator('.notif')).toHaveCount(40);
});

test('notification text is escaped, not injected', async ({ page }) => {
  await page.evaluate(() => {
    window.__jwsNotify('punch', '<img src=x onerror=alert(1)> checked in', 'now');
  });
  await page.click('#btnBell');
  await expect(page.locator('.notif-panel img')).toHaveCount(0);
  await expect(page.locator('.notif-panel')).toContainText('<img src=x');
});

test('tray closes when clicking elsewhere', async ({ page }) => {
  await page.click('#btnBell');
  await expect(page.locator('.notif-panel')).toBeVisible();
  await page.click('#pgTitle');
  await expect(page.locator('.notif-panel')).toHaveCount(0);
});

test('clear button empties the tray', async ({ page }) => {
  await page.click('#btnBell');
  await page.click('#notifClear');
  await expect(page.locator('.notif-panel .empty')).toBeVisible();
});
