// The Bikram Sambat calendar, for a school that runs on it.
//
// Every date the app *stores* stays ISO Gregorian — 'YYYY-MM-DD' — because the
// database, the terminal and the reports all agree on that already, and
// changing it would mean migrating history for no gain. What changes is what a
// person sees and types: Nepali first, English underneath.
//
// The month-length table is not duplicated here. It is fetched once from the
// Rust side, which is the same table every report converts against, so a date
// chosen in the picker and a date printed on a payroll sheet cannot drift apart.

import { esc, icon } from './ui.js';

let T = null; // the table from the backend

/** Load the calendar table once. Safe to call repeatedly. */
export async function initCalendar(api) {
  if (T) return T;
  T = await api.bsCalendar();
  return T;
}

export const ready = () => !!T;

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/** Days since 1970-01-01 for a Gregorian date. Howard Hinnant's algorithm. */
function daysFromCivil(y, m, d) {
  y -= m <= 2 ? 1 : 0;
  const era = Math.floor((y >= 0 ? y : y - 399) / 400);
  const yoe = y - era * 400;
  const doy = Math.floor((153 * (m + (m > 2 ? -3 : 9)) + 2) / 5) + d - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}

function civilFromDays(z) {
  z += 719468;
  const era = Math.floor((z >= 0 ? z : z - 146096) / 146097);
  const doe = z - era * 146097;
  const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524)
    - Math.floor(doe / 146096)) / 365);
  const y = yoe + era * 400;
  const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
  const mp = Math.floor((5 * doy + 2) / 153);
  const d = doy - Math.floor((153 * mp + 2) / 5) + 1;
  const m = mp + (mp < 10 ? 3 : -9);
  return [y + (m <= 2 ? 1 : 0), m, d];
}

const parseIso = (iso) => String(iso || '').split('-').map(Number);

/** Days since the epoch for 1 Baisakh of the table's first year. */
function anchorDays() {
  const [y, m, d] = parseIso(T.anchor_ad);
  return daysFromCivil(y, m, d);
}

/** Length of a BS month, or 0 if outside the table. */
export function daysInBsMonth(year, month) {
  if (!T) return 0;
  const row = T.months[year - T.min_year];
  return row ? row[month - 1] || 0 : 0;
}

/**
 * Gregorian ISO date to BS.
 * Returns `null` outside the table rather than extrapolating — a wrong date on
 * a payroll report is worse than a missing one.
 */
export function toBs(iso) {
  if (!T || !iso) return null;
  const [y, m, d] = parseIso(iso);
  if (!y || !m || !d) return null;

  let remaining = daysFromCivil(y, m, d) - anchorDays();
  if (remaining < 0) return null;

  let year = T.min_year;
  while (year <= T.max_year) {
    const row = T.months[year - T.min_year];
    const inYear = row.reduce((a, b) => a + b, 0);
    if (remaining < inYear) break;
    remaining -= inYear;
    year++;
  }
  if (year > T.max_year) return null;

  const row = T.months[year - T.min_year];
  let month = 1;
  while (month <= 12 && remaining >= row[month - 1]) {
    remaining -= row[month - 1];
    month++;
  }
  return { year, month, day: remaining + 1 };
}

/** BS to Gregorian ISO, or null if the date does not exist. */
export function toAd(year, month, day) {
  if (!T || year < T.min_year || year > T.max_year) return null;
  if (month < 1 || month > 12) return null;
  if (day < 1 || day > daysInBsMonth(year, month)) return null;

  let days = 0;
  for (let y = T.min_year; y < year; y++) {
    days += T.months[y - T.min_year].reduce((a, b) => a + b, 0);
  }
  const row = T.months[year - T.min_year];
  for (let m = 1; m < month; m++) days += row[m - 1];
  days += day - 1;

  const [ay, am, ad] = civilFromDays(anchorDays() + days);
  return `${ay}-${String(am).padStart(2, '0')}-${String(ad).padStart(2, '0')}`;
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/** Western digits to Nepali. */
export function nepaliDigits(v) {
  if (!T) return String(v);
  return String(v).replace(/\d/g, (c) => T.digits[Number(c)]);
}

export const bsMonthName = (m) => (T ? T.month_names[m - 1] || '' : '');

/** `3 Bhadra 2083`. */
export function bsPretty(iso) {
  const b = toBs(iso);
  return b ? `${b.day} ${bsMonthName(b.month)} ${b.year}` : '';
}

/** `३ भदौ २०८३` — for places where the Nepali script reads better. */
export function bsNepali(iso) {
  const b = toBs(iso);
  if (!b) return '';
  return `${nepaliDigits(b.day)} ${bsMonthName(b.month)} ${nepaliDigits(b.year)}`;
}

/** `2083-05-03`. */
export function bsIso(iso) {
  const b = toBs(iso);
  return b ? `${b.year}-${String(b.month).padStart(2, '0')}-${String(b.day).padStart(2, '0')}` : '';
}

/** The English date, short: `19 Aug 2026`. */
export function adPretty(iso) {
  if (!iso) return '';
  const d = new Date(`${iso}T00:00:00`);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
}

/** Both, the way this app shows a date everywhere: BS first, AD underneath. */
export function bothPretty(iso) {
  const bs = bsPretty(iso);
  return bs ? `${bs} <span class="ad">${esc(adPretty(iso))}</span>` : esc(adPretty(iso));
}

// ---------------------------------------------------------------------------
// The picker
// ---------------------------------------------------------------------------

/**
 * Markup for a Nepali date field.
 *
 * The real value lives in a hidden input holding the ISO Gregorian date, so
 * every screen that reads `[name=x].value` keeps working unchanged.
 */
export function dateField(name, label, isoValue, { id, small } = {}) {
  const idAttr = id ? ` id="${esc(id)}"` : '';
  return `<div class="fld npfld">
      ${label ? `<label>${esc(label)}</label>` : ''}
      <button type="button" class="npbtn${small ? ' sm' : ''}" data-np="${esc(name)}">
        <span class="np-t">
          <b>${esc(bsPretty(isoValue) || 'Choose a date')}</b>
          <span>${esc(adPretty(isoValue))}</span>
        </span>
        ${icon('clock')}
      </button>
      <input type="hidden" name="${esc(name)}"${idAttr} value="${esc(isoValue || '')}">
    </div>`;
}

/** Refresh the label on a field after its value changed programmatically. */
export function refreshField(root, name) {
  const hidden = root.querySelector(`input[name="${name}"]`);
  const btn = root.querySelector(`[data-np="${name}"] .np-t`);
  if (!hidden || !btn) return;
  btn.innerHTML = `<b>${esc(bsPretty(hidden.value) || 'Choose a date')}</b>
    <span>${esc(adPretty(hidden.value))}</span>`;
}

let openPanel = null;

function closePicker() {
  openPanel?.remove();
  openPanel = null;
  document.removeEventListener('click', onOutside, true);
}

function onOutside(e) {
  if (!openPanel) return;
  if (openPanel.contains(e.target) || e.target.closest('[data-np]')) return;
  closePicker();
}

/**
 * Open the calendar over a field.
 *
 * The grid is a real Nepali month: the weekday a month starts on comes from
 * converting its first day, so the columns line up the way a printed patro does.
 */
function openPicker(btn, root, onPick) {
  closePicker();
  const name = btn.dataset.np;
  const hidden = root.querySelector(`input[name="${name}"]`);
  if (!hidden) return;

  const todayIso = new Date().toISOString().slice(0, 10);
  const start = toBs(hidden.value) || toBs(todayIso);
  if (!start) return;

  let { year, month } = start;

  const panel = document.createElement('div');
  panel.className = 'nppanel';
  document.body.appendChild(panel);
  openPanel = panel;

  // Keep the panel on screen when the field sits low in the window.
  const r = btn.getBoundingClientRect();
  panel.style.left = `${Math.min(r.left, window.innerWidth - 330)}px`;
  const below = window.innerHeight - r.bottom;
  if (below < 380 && r.top > 380) panel.style.bottom = `${window.innerHeight - r.top + 6}px`;
  else panel.style.top = `${r.bottom + 6}px`;

  function paint() {
    const len = daysInBsMonth(year, month);
    const firstIso = toAd(year, month, 1);
    const firstWeekday = firstIso ? new Date(`${firstIso}T00:00:00`).getDay() : 0;
    const chosen = toBs(hidden.value);
    const nowBs = toBs(todayIso);

    const cells = [];
    for (let i = 0; i < firstWeekday; i++) cells.push('<span class="np-c pad"></span>');
    for (let d = 1; d <= len; d++) {
      const iso = toAd(year, month, d);
      const isToday = nowBs && nowBs.year === year && nowBs.month === month && nowBs.day === d;
      const isSel = chosen && chosen.year === year && chosen.month === month && chosen.day === d;
      const dow = new Date(`${iso}T00:00:00`).getDay();
      cells.push(`<button type="button" class="np-c${isSel ? ' on' : ''}${
        isToday ? ' today' : ''}${dow === 6 ? ' sat' : ''}" data-iso="${iso}">
          <b>${nepaliDigits(d)}</b><i>${new Date(`${iso}T00:00:00`).getDate()}</i>
        </button>`);
    }

    const years = [];
    for (let y = T.min_year; y <= T.max_year; y++) {
      years.push(`<option value="${y}" ${y === year ? 'selected' : ''}>${nepaliDigits(y)} (${y})</option>`);
    }

    panel.innerHTML = `
      <div class="np-h">
        <button type="button" class="np-nav" data-step="-1">‹</button>
        <select class="np-sel" data-sel="month">
          ${T.month_names.map((n, i) =>
            `<option value="${i + 1}" ${i + 1 === month ? 'selected' : ''}>${esc(n)}</option>`).join('')}
        </select>
        <select class="np-sel" data-sel="year">${years.join('')}</select>
        <button type="button" class="np-nav" data-step="1">›</button>
      </div>
      <div class="np-dow">${T.day_names_short.map((d) => `<span>${esc(d)}</span>`).join('')}</div>
      <div class="np-grid">${cells.join('')}</div>
      <div class="np-f">
        <button type="button" class="btn sm" data-iso="${todayIso}">Today · ${esc(bsPretty(todayIso))}</button>
        <span>${esc(adPretty(toAd(year, month, 1)))} – ${esc(adPretty(toAd(year, month, len)))}</span>
      </div>`;
  }

  panel.addEventListener('click', (e) => {
    const step = e.target.closest('[data-step]');
    if (step) {
      month += Number(step.dataset.step);
      if (month < 1) { month = 12; year--; }
      if (month > 12) { month = 1; year++; }
      // Walking past either end of the table would render a blank month.
      year = Math.max(T.min_year, Math.min(T.max_year, year));
      paint();
      return;
    }
    const cell = e.target.closest('[data-iso]');
    if (cell) {
      hidden.value = cell.dataset.iso;
      refreshField(root, name);
      hidden.dispatchEvent(new Event('change', { bubbles: true }));
      closePicker();
      onPick?.(cell.dataset.iso, name);
    }
  });

  panel.addEventListener('change', (e) => {
    const sel = e.target.closest('[data-sel]');
    if (!sel) return;
    if (sel.dataset.sel === 'month') month = Number(sel.value);
    else year = Number(sel.value);
    paint();
  });

  paint();
  setTimeout(() => document.addEventListener('click', onOutside, true), 0);
}

/**
 * Make every Nepali date field inside `root` open a picker.
 *
 * One delegated listener, so fields added later still work.
 */
export function wireDateFields(root, onPick) {
  root.addEventListener('click', (e) => {
    const btn = e.target.closest('[data-np]');
    if (btn) {
      e.preventDefault();
      openPicker(btn, root, onPick);
    }
  });
}
