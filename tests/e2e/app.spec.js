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

test('every report type renders its own columns', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await expect(page.locator('#repGrid table.grid')).toBeVisible();

  const kinds = page.locator('#repList [data-key]');
  const n = await kinds.count();
  expect(n).toBe(8);

  const seen = new Set();
  for (let i = 0; i < n; i++) {
    await kinds.nth(i).click();
    await expect(page.locator('#repMeta b')).not.toBeEmpty();
    // Each report must bring its own shape, not reuse the last one's columns.
    const heads = await page.locator('#repGrid thead th').allInnerTexts();
    expect(heads.length).toBeGreaterThan(3);
    seen.add(heads.join('|'));
  }
  expect(seen.size).toBeGreaterThan(5);
});

test('report grid sorts on a column and keeps its totals', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await page.click('#repList [data-key="daily_stat"]');
  await expect(page.locator('#repGrid tfoot')).toBeVisible();

  const nameCol = page.locator('#repGrid thead th', { hasText: 'Name' }).first();
  await nameCol.click();
  const first = await page.locator('#repGrid tbody tr td').nth(1).innerText();
  await nameCol.click();
  const reversed = await page.locator('#repGrid tbody tr td').nth(1).innerText();
  expect(first).not.toBe(reversed);
  // Sorting must not disturb the totals row.
  await expect(page.locator('#repGrid tfoot')).toContainText('Total');
});

test('report rejects a backwards date range', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  // Drive the Nepali picker rather than typing: walking the "To" date back two
  // Nepali months puts it before "From", which starts at the month's beginning.
  await page.click('[data-np="to"]');
  await expect(page.locator('.nppanel')).toBeVisible();
  await page.click('.nppanel [data-step="-1"]');
  await page.click('.nppanel [data-step="-1"]');
  await page.locator('.nppanel .np-c:not(.pad)').first().click();
  await page.click('#btnRun');
  await expect(page.locator('.toast.err')).toContainText('before the start');
});

test('an unknown employee in the filter is refused rather than ignored', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await page.fill('#fWho', 'Nobody At All');
  await page.click('#btnRun');
  // Silently reporting on everyone would be worse than refusing.
  await expect(page.locator('.toast.err')).toContainText('No member of staff');
});

test('the recipient book rejects an address with no @', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await page.click('#repBook');
  await expect(page.locator('.mdl')).toContainText('Report recipients');
  await page.click('#rcpAdd');
  await page.fill('.ov:last-child [name=name]', 'Test Official');
  await page.fill('.ov:last-child [name=email]', 'not-an-address');
  await page.locator('.ov:last-child .mdl-f .btn.pri').click();
  await expect(page.locator('.toast.err')).toContainText('missing an @');
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

test('attendance rules has four sub-tabs and each one renders', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  const tabs = page.locator('#subtabs button[data-tab]');
  await expect(tabs).toHaveCount(4);
  for (let i = 0; i < 4; i++) {
    await tabs.nth(i).click();
    await expect(page.locator('#tabBody .card').first()).toBeVisible();
    await expect(page.locator('#tabBody')).not.toContainText('undefined');
  }
});

test('an edit on one rules tab survives switching to another', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=calc]');
  await page.fill('[data-key=late_after_min]', '17');
  // The draft lives in memory, not in the DOM, so moving away and back must
  // not silently discard what was typed.
  await page.click('#subtabs [data-tab=weekend]');
  await page.click('#subtabs [data-tab=calc]');
  await expect(page.locator('[data-key=late_after_min]')).toHaveValue('17');
  await expect(page.locator('#dirty')).toBeVisible();
});

test('rules screen refuses a grading order that produces no half days', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=calc]');
  await page.fill('[data-key=half_day_after_min]', '300');
  await page.fill('[data-key=late_to_absent_min]', '240');
  await page.click('#btnSave');
  await expect(page.locator('.toast.err')).toContainText('half day');
});

test('rules screen refuses making every day a weekend', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=weekend]');
  for (let d = 0; d < 7; d++) await page.check(`[data-key=weekend_${d}]`);
  await page.click('#btnSave');
  await expect(page.locator('.toast.err')).toContainText('weekend');
});

test('rules screen saves a sensible value', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=calc]');
  await page.fill('[data-key=late_after_min]', '15');
  await page.click('#btnSave');
  await expect(page.locator('.toast.ok')).toBeVisible();
  await expect(page.locator('#dirty')).toBeHidden();
});

test('the weekend preview follows the checkboxes', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=weekend]');
  // Saturday is the school's rest day out of the box.
  await expect(page.locator('#wkPrev .wkd.off')).toHaveCount(1);
  await page.check('[data-key=weekend_5]');
  await expect(page.locator('#wkPrev .wkd.off')).toHaveCount(2);
});

test('timetable tabs all render', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  const tabs = page.locator('#ttTabs button[data-tab]');
  await expect(tabs).toHaveCount(3);
  for (let i = 0; i < 3; i++) {
    await tabs.nth(i).click();
    await page.waitForTimeout(450);
    await expect(page.locator('#ttBody .card').first()).toBeVisible();
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

// ---------------------------------------------------------------------------
// The three-tier roster
// ---------------------------------------------------------------------------

test('timetable maintenance shows a list and an editing form together', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=blocks]');
  await expect(page.locator('#ttList .pane-item').first()).toBeVisible();
  await expect(page.locator('#ttForm [name=name]')).toBeVisible();
  // Selecting a different block must load it into the form, not blank it.
  await page.locator('#ttList .pane-item').nth(1).click();
  await expect(page.locator('#ttForm [name=name]')).not.toHaveValue('');
  await expect(page.locator('#ttForm [name=on_duty]')).not.toHaveValue('');
});

test('the timetable form previews the block on a 24-hour bar', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=blocks]');
  const block = page.locator('#ttForm .daybar .blk').first();
  await expect(block).toBeVisible();
  const before = await block.getAttribute('style');
  await page.fill('#ttForm [name=on_duty]', '14:00');
  await expect(page.locator('#ttForm .daybar .blk').first()).not.toHaveAttribute('style', before);
});

test('adding a new timetable starts from a blank form', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=blocks]');
  await page.click('#ttNew');
  await expect(page.locator('#ttForm [name=name]')).toHaveValue('');
  await expect(page.locator('#ttForm')).toContainText('New timetable');
});

test('shift management draws a seven day grid of coloured blocks', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=shifts]');
  await expect(page.locator('#shList .pane-item').first()).toBeVisible();
  // Seven rows for a one-week cycle, whatever is on them.
  await expect(page.locator('.grow-row')).toHaveCount(7);
  await expect(page.locator('.grow-row .daybar').first()).toBeVisible();
  // A day with no timetable must say so rather than render an empty bar.
  await expect(page.locator('.growgrid')).toContainText('Rest day');
});

test('selecting a day in the shift grid highlights only that day', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=shifts]');
  await page.locator('.grow-row').nth(3).click();
  await expect(page.locator('.grow-row.on')).toHaveCount(1);
  await expect(page.locator('.grow-row').nth(3)).toHaveClass(/\bon\b/);
});

test('employee schedule shows the department tree and roster', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=roster]');
  await expect(page.locator('#tree .pane-item').first()).toBeVisible();
  await expect(page.locator('#rsTable tbody tr').first()).toBeVisible();
  await expect(page.locator('#rsTable thead')).toContainText('TempShift');
});

test('arranging shifts without a selection is refused', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=roster]');
  await page.click('#rsArrange');
  await expect(page.locator('.toast.err')).toContainText('Select at least one');
});

test('the roster calendar draws a row per day with rest days shaded', async ({ page }) => {
  await page.click('#nav a[data-page="timetables"]');
  await page.click('#ttTabs [data-tab=roster]');
  await page.locator('#rsTable [data-cal]').first().click();
  await expect(page.locator('.cal-row').first()).toBeVisible();
  // A fortnight of the school week contains at least one Saturday.
  await expect(page.locator('.cal-row.off').first()).toBeVisible();
});

test('no new screen renders NaN or undefined', async ({ page }) => {
  for (const [nav, sub] of [
    ['rules', '#subtabs button[data-tab]'],
    ['timetables', '#ttTabs button[data-tab]'],
  ]) {
    await page.click(`#nav a[data-page="${nav}"]`);
    const tabs = page.locator(sub);
    for (let i = 0; i < await tabs.count(); i++) {
      await tabs.nth(i).click();
      await page.waitForTimeout(400);
      const text = await page.locator('#page').innerText();
      expect(text).not.toContain('undefined');
      expect(text).not.toContain('NaN');
      expect(text).not.toContain('[object Object]');
    }
  }
});


// ---------------------------------------------------------------------------
// The Nepali calendar
// ---------------------------------------------------------------------------

test('dates are shown in Bikram Sambat with the English date underneath', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  // The field itself.
  const from = page.locator('[data-np="from"]');
  await expect(from.locator('b')).toContainText(/Baisakh|Jestha|Ashadh|Shrawan|Bhadra|Ashwin|Kartik|Mangsir|Poush|Magh|Falgun|Chaitra/);
  await expect(from.locator('.np-t span')).toContainText(/\d{4}/);

  // And the grid's date column.
  await page.click('#repList [data-key="general"]');
  await expect(page.locator('#repGrid .dcell b').first()).toBeVisible();
  await expect(page.locator('#repGrid .dcell i').first()).toContainText(/\d{4}/);
});

test('the date picker shows a real Nepali month grid', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  await page.click('[data-np="from"]');
  const panel = page.locator('.nppanel');
  await expect(panel).toBeVisible();

  // Seven weekday headings, and a month of between 29 and 32 days.
  await expect(panel.locator('.np-dow span')).toHaveCount(7);
  const days = await panel.locator('.np-c:not(.pad)').count();
  expect(days).toBeGreaterThanOrEqual(29);
  expect(days).toBeLessThanOrEqual(32);

  // Each cell carries the Nepali numeral and the Gregorian day beneath it.
  const first = panel.locator('.np-c:not(.pad)').first();
  await expect(first.locator('b')).toContainText(/[०-९]/);
  await expect(first.locator('i')).toContainText(/\d/);
});

test('picking a date updates the field and closes the calendar', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  const before = await page.locator('[data-np="from"] b').innerText();
  await page.click('[data-np="from"]');
  await page.locator('.nppanel .np-c:not(.pad)').nth(9).click();
  await expect(page.locator('.nppanel')).toHaveCount(0);
  const after = await page.locator('[data-np="from"] b').innerText();
  expect(after).not.toBe(before);
  await expect(page.locator('[data-np="from"] b')).toContainText(/20\d\d/);
});

test('the calendar closes on an outside click without changing the date', async ({ page }) => {
  await page.click('#nav a[data-page="reports"]');
  const before = await page.locator('[data-np="from"] b').innerText();
  await page.click('[data-np="from"]');
  await expect(page.locator('.nppanel')).toBeVisible();
  await page.click('#pgTitle');
  await expect(page.locator('.nppanel')).toHaveCount(0);
  await expect(page.locator('[data-np="from"] b')).toHaveText(before);
});

test('the dashboard hero carries the school and today in Nepali', async ({ page }) => {
  await page.click('#nav a[data-page="dashboard"]');
  await expect(page.locator('.hero-t h2')).toContainText('Janapremi World School');
  await expect(page.locator('.hero-t p')).toContainText('Kaushaltar');
  await expect(page.locator('.hero-logo')).toBeVisible();
  // Nepali numerals in the date and the clock.
  await expect(page.locator('#heroBs')).toContainText(/[०-९]/);
  await expect(page.locator('#heroClock')).toContainText(/[०-९]/);
  await expect(page.locator('#heroAd')).toContainText(/\d{4}/);
});

test('rule steppers change the value without the browser spin arrows', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=basic]');
  const field = page.locator('[data-key=month_start_day]');
  await field.fill('10');
  await page.locator('.stp[data-for=month_start_day][data-step="1"]').click();
  await expect(field).toHaveValue('11');
  await page.locator('.stp[data-for=month_start_day][data-step="-1"]').click();
  await page.locator('.stp[data-for=month_start_day][data-step="-1"]').click();
  await expect(field).toHaveValue('9');
  // The change must have reached the draft, not just the box.
  await expect(page.locator('#dirty')).toBeVisible();
});

test('a stepper will not push a value outside its range', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=basic]');
  const field = page.locator('[data-key=month_start_day]');
  await field.fill('31');
  await page.locator('.stp[data-for=month_start_day][data-step="1"]').click();
  await expect(field).toHaveValue('31', { timeout: 3000 });
  await field.fill('1');
  await page.locator('.stp[data-for=month_start_day][data-step="-1"]').click();
  await expect(field).toHaveValue('1');
});

test('a fractional stepper does not produce floating point noise', async ({ page }) => {
  await page.click('#nav a[data-page="rules"]');
  await page.click('#subtabs [data-tab=stat]');
  const unit = page.locator('[data-key=min_unit]');
  await unit.fill('0.5');
  await page.locator('.stp[data-for=min_unit][data-step="1"]').click();
  // 0.5 + 0.05 must read as 0.55, not 0.5500000000000001.
  await expect(unit).toHaveValue('0.55');
});
