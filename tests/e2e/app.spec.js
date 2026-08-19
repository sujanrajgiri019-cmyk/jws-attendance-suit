import { test, expect } from '@playwright/test';

// Fail any test that logs a page error — a screen that silently throws and
// renders an empty card is the failure mode most likely to reach the school.
test.beforeEach(async ({ page }) => {
  page.on('pageerror', (e) => {
    throw new Error(`Uncaught page error: ${e.message}`);
  });
  await page.goto('/');
  await page.waitForSelector('#nav a', { timeout: 15_000 });
});

const PAGES = [
  ['dashboard', 'Dashboard'],
  ['devices', 'Devices'],
  ['data', 'Data Transfer'],
  ['members', 'Members'],
  ['departments', 'Departments'],
  ['database', 'Database'],
  ['rules', 'Attendance Rules'],
  ['timetables', 'Timetables'],
  ['reports', 'Reports'],
  ['settings', 'Settings'],
];

for (const [id, title] of PAGES) {
  test(`${id} screen loads and shows content`, async ({ page }) => {
    await page.click(`#nav a[data-page="${id}"]`);
    await expect(page.locator('#pgTitle')).toHaveText(title);
    // Every screen must render at least one card; an empty page means the
    // module threw during mount.
    await expect(page.locator('#page .card').first()).toBeVisible({ timeout: 10_000 });
    await expect(page.locator('#page')).not.toBeEmpty();
  });
}

test('dashboard shows the five key figures with real numbers', async ({ page }) => {
  await page.click('#nav a[data-page="dashboard"]');
  const tiles = page.locator('.kpi');
  await expect(tiles).toHaveCount(5);
  for (let i = 0; i < 5; i++) {
    const value = await tiles.nth(i).locator('.kv').textContent();
    expect(value.trim()).not.toBe('');
    expect(value).not.toContain('NaN');
    expect(value).not.toContain('undefined');
  }
});

test('no screen renders NaN or undefined anywhere', async ({ page }) => {
  for (const [id] of PAGES) {
    await page.click(`#nav a[data-page="${id}"]`);
    await page.waitForTimeout(500);
    const text = await page.locator('#page').innerText();
    expect(text, `${id} shows NaN`).not.toContain('NaN');
    expect(text, `${id} shows undefined`).not.toContain('undefined');
    expect(text, `${id} shows [object Object]`).not.toContain('[object Object]');
  }
});

test('member search filters the table', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  await expect(page.locator('#tbl tbody tr').first()).toBeVisible();
  const before = await page.locator('#tbl tbody tr').count();
  expect(before).toBeGreaterThan(5);

  const firstName = (await page.locator('#tbl tbody tr').first().locator('.pt b').textContent()).trim();
  await page.fill('#q', firstName);
  await page.waitForTimeout(500);

  const after = await page.locator('#tbl tbody tr').count();
  expect(after).toBeLessThanOrEqual(before);
  expect(after).toBeGreaterThan(0);
  await expect(page.locator('#count')).toContainText('member');
});

test('member search with no matches shows an empty state, not a broken table', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  await page.fill('#q', 'zzzz-no-such-person');
  await page.waitForTimeout(500);
  await expect(page.locator('#tbl .empty')).toBeVisible();
});

test('selecting members enables the bulk actions', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  await expect(page.locator('#bulkDelete')).toBeDisabled();
  await page.locator('#tbl tbody input[type=checkbox]').first().check();
  await expect(page.locator('#bulkDelete')).toBeEnabled();
  await expect(page.locator('#count')).toContainText('1 selected');
});

test('adding a member validates the enrolment number', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  await page.click('#btnAdd');
  await expect(page.locator('.mdl')).toBeVisible();

  await page.fill('.mdl [name=full_name]', 'Test Person');
  await page.fill('.mdl [name=enroll_no]', '70000');
  await page.click('.mdl .btn.pri');

  // The dialog stays open and the reason is shown.
  await expect(page.locator('.toast.err')).toContainText('65535');
  await expect(page.locator('.mdl')).toBeVisible();
});

test('a member can be added and appears in the list', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  // Wait for the real table, not the loading skeleton, before counting.
  await expect(page.locator('#count')).toContainText('members');
  await expect(page.locator('#tbl tbody .person').first()).toBeVisible();
  const before = await page.locator('#tbl tbody tr').count();
  expect(before).toBeGreaterThan(5);

  await page.click('#btnAdd');
  await page.fill('.mdl [name=full_name]', 'Kiran Shakya');
  await page.click('.mdl .btn.pri');
  await expect(page.locator('.mdl')).toBeHidden();
  await page.waitForTimeout(600);

  await expect(page.locator('#count')).toContainText(`${before + 1} members`);
  await page.fill('#q', 'Kiran Shakya');
  await page.waitForTimeout(500);
  await expect(page.locator('#tbl tbody')).toContainText('Kiran Shakya');
});

test('report type switches change the table shape', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await expect(page.locator('#repTitle')).toContainText('General Attendance');
  await expect(page.locator('#tbl tbody tr').first()).toBeVisible();

  await page.locator('#types input[value=daywise]').check();
  await page.waitForTimeout(700);
  await expect(page.locator('#repTitle')).toContainText('Day-wise');

  await page.locator('#types input[value=monthly]').check();
  await page.waitForTimeout(900);
  await expect(page.locator('#repTitle')).toContainText('Monthly Grid');
  await expect(page.locator('.mcell').first()).toBeVisible();
});

test('individual report requires a member and then renders detail', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await page.locator('#types input[value=individual]').check();
  await page.waitForTimeout(800);
  await expect(page.locator('#memWrap')).toBeVisible();
  await expect(page.locator('#summary')).not.toBeEmpty();
});

test('report rejects a backwards date range', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await page.fill('#from', '2026-08-20');
  await page.fill('#to', '2026-08-10');
  await page.click('#btnRun');
  await expect(page.locator('.toast.err')).toContainText('before the start');
});

test('database tabs each render a table', async ({ page }) => {
  await page.click('#nav a[data-page="database"]');
  const tabs = page.locator('#tabs button');
  const n = await tabs.count();
  expect(n).toBeGreaterThan(5);
  for (let i = 0; i < n; i++) {
    await tabs.nth(i).click();
    await page.waitForTimeout(400);
    // Either rows, or an honest empty state — never a blank panel.
    const hasRows = await page.locator('#tbl tbody tr').count();
    expect(hasRows).toBeGreaterThan(0);
  }
});

test('rules screen refuses an unachievable full-day threshold', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.fill('[data-rule=min_full_day_min]', '900');
  await page.click('#btnSave');
  // 900 minutes cannot be worked in any seeded shift, so this must be caught
  // before it silently turns every member of staff into a half day.
  await expect(page.locator('.toast.err')).toContainText('half day');
});

test('rules screen saves a sensible value', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.fill('[data-rule=late_grace_min]', '15');
  await page.click('#btnSave');
  await expect(page.locator('.toast.ok')).toBeVisible();
});

test('working day chips toggle', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  const sat = page.locator('#days [data-day="6"]');
  await expect(sat).toHaveClass(/\bn\b/);
  await sat.click();
  await expect(sat).toHaveClass(/\bo\b/);
});

test('timetable tabs all render', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  const tabs = page.locator('#tabs button');
  for (let i = 0; i < await tabs.count(); i++) {
    await tabs.nth(i).click();
    await page.waitForTimeout(450);
    await expect(page.locator('#body .card').first()).toBeVisible();
  }
});

test('settings tabs all render and the default password is flagged', async ({ page }) => {
  await page.click('#nav a[data-page="settings"]');
  await expect(page.locator('#defaultPwBanner')).toBeVisible();

  const tabs = page.locator('#tabs button');
  for (let i = 0; i < await tabs.count(); i++) {
    await tabs.nth(i).click();
    await page.waitForTimeout(450);
    await expect(page.locator('#body .card').first()).toBeVisible();
  }
});

test('changing the password requires the two new entries to match', async ({ page }) => {
  await page.click('#nav a[data-page="settings"]');
  await page.locator('#tabs button[data-tab=security]').click();
  await page.fill('#pwCur', 'Attendance@123');
  await page.fill('#pwNew', 'NewPassword1');
  await page.fill('#pwNew2', 'DifferentOne1');
  await page.click('#btnPw');
  await expect(page.locator('.toast.err')).toContainText('do not match');
});

test('device errors are reported to the user, not swallowed', async ({ page }) => {
  await page.click('#nav a[data-page="devices"]');
  await page.locator('[data-ping]').first().click();
  // No terminal is reachable from a browser; the point is that the user is told.
  await expect(page.locator('.toast.err')).toBeVisible({ timeout: 10_000 });
});

test('a department holding staff cannot be deleted', async ({ page }) => {
  await page.click('#nav a[data-page="departments"]');
  await page.locator('[data-del]').first().click();
  await expect(page.locator('.mdl')).toBeVisible();
  await page.locator('.mdl .btn.dan').click();
  await expect(page.locator('.toast.err')).toContainText('still has');
});

test('modals close on Escape', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  await page.click('#btnAdd');
  await expect(page.locator('.mdl')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.mdl')).toBeHidden();
});

test('navigation keeps working after opening and closing a dialog', async ({ page }) => {
  await page.click('#nav a[data-page="members"]');
  await page.click('#btnAdd');
  await page.keyboard.press('Escape');
  await page.click('#nav a[data-page="dashboard"]');
  await expect(page.locator('#pgTitle')).toHaveText('Dashboard');
  await expect(page.locator('.kpi').first()).toBeVisible();
});
