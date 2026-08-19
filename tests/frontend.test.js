// Node's built-in test runner — no framework.
//
// These cover the pure helpers in the frontend: the ones where a mistake shows
// up as a wrong number on a printed report rather than a crash.
//
//   node --test tests/

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// esbuild is not involved here; the helpers are plain ES modules.
const ui = await import('../src/js/ui.js');

test('esc neutralises HTML in names from the database', () => {
  assert.equal(ui.esc('<script>alert(1)</script>'), '&lt;script&gt;alert(1)&lt;/script&gt;');
  assert.equal(ui.esc("O'Brien & Co"), 'O&#39;Brien &amp; Co');
  assert.equal(ui.esc('"quoted"'), '&quot;quoted&quot;');
  assert.equal(ui.esc(null), '');
  assert.equal(ui.esc(undefined), '');
  assert.equal(ui.esc(0), '0', 'zero must not be swallowed');
});

test('person cell escapes the name it renders', () => {
  const html = ui.person('<img src=x onerror=1>', 'sub', 1);
  assert.ok(!html.includes('<img'), 'raw tag must not reach the DOM');
  assert.ok(html.includes('&lt;img'));
});

test('duration formats minutes the way a payroll sheet expects', () => {
  assert.equal(ui.duration(0), '0:00');
  assert.equal(ui.duration(5), '0:05');
  assert.equal(ui.duration(60), '1:00');
  assert.equal(ui.duration(390), '6:30');
  assert.equal(ui.duration(1439), '23:59');
});

test('duration never prints a negative or NaN time', () => {
  assert.equal(ui.duration(-30), '0:00');
  assert.equal(ui.duration(NaN), '0:00');
  assert.equal(ui.duration(undefined), '0:00');
  assert.equal(ui.duration(null), '0:00');
});

test('hhmm trims seconds and handles a missing time', () => {
  assert.equal(ui.hhmm('08:55:00'), '08:55');
  assert.equal(ui.hhmm(null), '—');
  assert.equal(ui.hhmm(''), '—');
});

test('initials cope with one word, extra spaces and empty input', () => {
  assert.equal(ui.initials('Sarita Maharjan'), 'SM');
  assert.equal(ui.initials('Sarita'), 'S');
  assert.equal(ui.initials('  Bikash   Shrestha '), 'BS');
  assert.equal(ui.initials(''), '?');
  assert.equal(ui.initials(null), '?');
});

test('avatar colour is stable for the same person', () => {
  assert.equal(ui.colourFor(41), ui.colourFor(41));
  assert.ok(ui.PALETTE.includes(ui.colourFor(41)));
  assert.ok(ui.PALETTE.includes(ui.colourFor(0)));
  assert.ok(ui.PALETTE.includes(ui.colourFor(-5)), 'negative ids must not fall off the palette');
  assert.ok(ui.PALETTE.includes(ui.colourFor(undefined)));
});

test('date helpers roll across month and year boundaries', () => {
  assert.equal(ui.addDays('2026-08-19', 1), '2026-08-20');
  assert.equal(ui.addDays('2026-08-31', 1), '2026-09-01');
  assert.equal(ui.addDays('2026-01-01', -1), '2025-12-31');
  assert.equal(ui.addDays('2024-02-28', 1), '2024-02-29', '2024 is a leap year');
  assert.equal(ui.addDays('2026-02-28', 1), '2026-03-01', '2026 is not');
  assert.equal(ui.monthStart('2026-08-19'), '2026-08-01');
});

test('todayIso is zero-padded and parseable', () => {
  const t = ui.todayIso();
  assert.match(t, /^\d{4}-\d{2}-\d{2}$/);
  assert.ok(!Number.isNaN(Date.parse(t)));
});

test('status tags map to the right tone and a readable label', () => {
  assert.equal(ui.statusTone('Present'), 'g');
  assert.equal(ui.statusTone('Absent'), 'r');
  assert.equal(ui.statusTone('Late'), 'y');
  assert.equal(ui.statusTone('Leave'), 'v');
  assert.equal(ui.statusTone('SomethingNew'), 'n', 'unknown status must not break rendering');

  assert.equal(ui.statusLabel('HalfDay'), 'Half day');
  assert.equal(ui.statusLabel('WeeklyOff'), 'Weekly off');
  assert.equal(ui.statusLabel('Present'), 'Present');
  assert.ok(ui.statusTag('HalfDay').includes('Half day'));
});

test('icons render as SVG, and an unknown name renders nothing', () => {
  assert.ok(ui.icon('users').startsWith('<svg'));
  assert.ok(ui.icon('trend').includes('<path'), 'the KPI trend icon must exist');
  assert.equal(ui.icon('no-such-icon'), '');
});

test('donut copes with an empty day without producing NaN', () => {
  const svg = ui.donut([{ value: 0, colour: '#000', label: 'None' }], '0%', 'MARKED IN');
  assert.ok(svg.includes('<svg'));
  assert.ok(!svg.includes('NaN'), 'a blank day must not render NaN into the chart');
});

test('bar chart handles an empty series and escapes its tooltips', () => {
  assert.ok(ui.barChart([]).includes('No attendance'));
  const svg = ui.barChart([{ date: '2026-08-19', present: 40, late: 3, absent: 1 }]);
  assert.ok(svg.includes('<svg'));
  assert.ok(!svg.includes('NaN'));
});

test('bar chart does not divide by zero when every value is zero', () => {
  const svg = ui.barChart([{ date: '2026-08-19', present: 0, late: 0, absent: 0 }]);
  assert.ok(!svg.includes('NaN'));
  assert.ok(!svg.includes('Infinity'));
});

// ---------------------------------------------------------------------------
// The Nepali calendar
// ---------------------------------------------------------------------------

test('the generated Nepali table matches the Rust source it came from', async () => {
  const { BS_TABLE } = await import('../src/js/bs-table.generated.js');
  const rs = await fs.readFile(
    path.join(root, 'crates', 'zk-core', 'src', 'calendar.rs'), 'utf8');

  assert.equal(BS_TABLE.min_year, 2000);
  assert.equal(BS_TABLE.max_year, 2090);
  assert.equal(BS_TABLE.months.length, 91);

  // Spot-check a row against the file, so a mangled regex cannot pass silently.
  const line2083 = /\[([0-9,\s]+)\],\s*\/\/ 2083/.exec(rs);
  assert.ok(line2083, '2083 not found in calendar.rs');
  const expected = line2083[1].split(',').map((n) => Number(n.trim())).filter(Boolean);
  assert.deepEqual(BS_TABLE.months[2083 - 2000], expected);

  for (const [i, row] of BS_TABLE.months.entries()) {
    assert.equal(row.length, 12, `year ${2000 + i} does not have twelve months`);
    for (const len of row) {
      assert.ok(len >= 29 && len <= 32, `impossible month length ${len} in ${2000 + i}`);
    }
  }
});

test('Nepali conversion round-trips and agrees with the published reference', async () => {
  const np = await import('../src/js/nepali.js');
  const { BS_TABLE } = await import('../src/js/bs-table.generated.js');
  await np.initCalendar({ bsCalendar: async () => BS_TABLE });

  // Hamro Patro: 19 August 2026 is 3 Bhadra 2083.
  const b = np.toBs('2026-08-19');
  assert.deepEqual(b, { year: 2083, month: 5, day: 3 });
  assert.equal(np.bsPretty('2026-08-19'), '3 Bhadra 2083');
  assert.equal(np.toAd(2083, 5, 3), '2026-08-19');

  // Every month boundary in a decade must survive both conversions.
  for (let y = 2078; y <= 2088; y++) {
    for (let m = 1; m <= 12; m++) {
      const last = np.daysInBsMonth(y, m);
      for (const d of [1, last]) {
        const iso = np.toAd(y, m, d);
        assert.ok(iso, `${y}-${m}-${d} did not convert`);
        assert.deepEqual(np.toBs(iso), { year: y, month: m, day: d }, `${y}-${m}-${d}`);
      }
    }
  }
});

test('Nepali conversion refuses dates it cannot vouch for', async () => {
  const np = await import('../src/js/nepali.js');
  const { BS_TABLE } = await import('../src/js/bs-table.generated.js');
  await np.initCalendar({ bsCalendar: async () => BS_TABLE });

  // Before the table's anchor, and after its last year.
  assert.equal(np.toBs('1900-01-01'), null);
  assert.equal(np.toAd(1999, 1, 1), null);
  assert.equal(np.toAd(2091, 1, 1), null);
  // A day that does not exist in that month.
  assert.equal(np.toAd(2083, 5, 33), null);
  assert.equal(np.toAd(2083, 13, 1), null);
  // Rubbish in must not produce a confident answer.
  assert.equal(np.toBs(''), null);
  assert.equal(np.toBs('not a date'), null);
});

test('Nepali digits and both-calendar formatting', async () => {
  const np = await import('../src/js/nepali.js');
  const { BS_TABLE } = await import('../src/js/bs-table.generated.js');
  await np.initCalendar({ bsCalendar: async () => BS_TABLE });

  assert.equal(np.nepaliDigits('2083'), '२०८३');
  assert.equal(np.nepaliDigits(3), '३');
  assert.equal(np.bsNepali('2026-08-19'), '३ Bhadra २०८३');
  assert.equal(np.bsIso('2026-08-19'), '2083-05-03');
  assert.equal(np.adPretty('2026-08-19'), '19 Aug 2026');
  // Both together, with the English date marked up as the secondary line.
  assert.match(np.bothPretty('2026-08-19'), /3 Bhadra 2083.*class="ad".*19 Aug 2026/);
});

test('a Nepali date field carries the Gregorian value the backend expects', async () => {
  const np = await import('../src/js/nepali.js');
  const { BS_TABLE } = await import('../src/js/bs-table.generated.js');
  await np.initCalendar({ bsCalendar: async () => BS_TABLE });

  const html = np.dateField('from', 'From', '2026-08-19');
  // What the user reads is Nepali; what is submitted stays ISO Gregorian, so
  // every screen and query that already reads this field keeps working.
  assert.match(html, /3 Bhadra 2083/);
  assert.match(html, /19 Aug 2026/);
  assert.match(html, /<input type="hidden" name="from"[^>]*value="2026-08-19"/);
});

test('the stepper does not reuse the busy spinner class name', async () => {
  // `.spin` is the rotating busy indicator on a button. A stepper that shares
  // the class inherits `animation: rot ... infinite` and every number field on
  // the rules screen turns on its own axis — which is exactly what shipped
  // once and is the kind of thing nobody thinks to look for in CSS.
  const css = await fs.readFile(path.join(root, 'src', 'styles.css'), 'utf8');
  const rules = await fs.readFile(path.join(root, 'src', 'js', 'pages', 'rules.js'), 'utf8');

  assert.ok(!/class="spin"/.test(rules), 'rules.js still uses the spinner class');
  assert.ok(/class="stepper"/.test(rules), 'the stepper markup is missing');

  // Whatever carries an infinite animation must not also be a layout class.
  const spinBlock = /\n\.spin\{([^}]*)\}/.exec(css);
  assert.ok(spinBlock, '.spin rule not found');
  assert.match(spinBlock[1], /animation:\s*rot/, '.spin should be the busy indicator');
  assert.ok(!/display:\s*flex/.test(spinBlock[1]), '.spin must not double as a layout container');
});

test('the header clock is pinned to a fixed width', async () => {
  // Nepali numerals are not tabular, so a clock ticking in Devanagari changes
  // width every second. In a flex header that reflows the row and makes the
  // date beside it wrap and unwrap, shifting the whole page once a second.
  const css = await fs.readFile(path.join(root, 'src', 'styles.css'), 'utf8');
  const clock = [...css.matchAll(/\n\.clock\{([^}]*)\}/g)].at(-1);
  assert.ok(clock, '.clock rule not found');
  assert.match(clock[1], /min-width/, 'the clock needs a floor on its width');
  assert.match(css, /\.clock b\{[^}]*white-space:nowrap/, 'the time must not wrap');
});
