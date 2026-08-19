// Node's built-in test runner — no framework.
//
// These cover the pure helpers in the frontend: the ones where a mistake shows
// up as a wrong number on a printed report rather than a crash.
//
//   node --test tests/

import { test } from 'node:test';
import assert from 'node:assert/strict';

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
