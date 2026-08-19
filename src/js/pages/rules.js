// Attendance Rules — four sub-tabs over one saved row.
//
// The whole rule set is loaded once into `draft`, every control writes into
// that object, and Save sends it back in one piece. That is what makes the
// four tabs behave as one form: switching tabs cannot lose an edit, because
// nothing is ever read back off the DOM.
//
// The backend validates before writing, so a rule set that grades backwards is
// refused with a message rather than quietly corrupting a month of records.

import { api } from '../api.js';
import { icon, esc, toast, withBusy, confirmDialog } from '../ui.js';

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

const TABS = [
  { id: 'basic', label: 'Basic Settings', icon: 'cog' },
  { id: 'calc', label: 'Calculation', icon: 'shield' },
  { id: 'stat', label: 'Statistic Items', icon: 'chart' },
  { id: 'weekend', label: 'Weekend Set', icon: 'clock' },
];

// --- small control builders -------------------------------------------------
// Each returns markup and relies on one delegated listener, so adding a field
// never means adding a listener.

// A stepper rather than a bare number box: the browser's own spin arrows are
// two tiny hover-only targets, and a screen full of them flickers as the mouse
// crosses it.
const num = (key, label, hint, { min = 0, max = 100000, step = 1, suffix = 'mins' } = {}) => `
  <div class="fld">
    <label>${esc(label)}</label>
    <div class="stepper">
      <button type="button" class="stp" data-step="-1" data-for="${key}"
              tabindex="-1" aria-label="Decrease">−</button>
      <input class="inp" type="number" data-key="${key}"
             min="${min}" max="${max}" step="${step}" inputmode="decimal">
      <button type="button" class="stp" data-step="1" data-for="${key}"
              tabindex="-1" aria-label="Increase">+</button>
      ${suffix ? `<span class="suffix">${esc(suffix)}</span>` : ''}
    </div>
    ${hint ? `<div class="hint">${esc(hint)}</div>` : ''}
  </div>`;

const text = (key, label, hint, { width } = {}) => `
  <div class="fld"${width ? ` style="max-width:${width}px"` : ''}>
    <label>${esc(label)}</label>
    <input class="inp" type="text" data-key="${key}">
    ${hint ? `<div class="hint">${esc(hint)}</div>` : ''}
  </div>`;

const select = (key, label, options, hint) => `
  <div class="fld">
    <label>${esc(label)}</label>
    <select class="inp" data-key="${key}">
      ${options.map((o) => `<option value="${esc(o.v)}">${esc(o.l)}</option>`).join('')}
    </select>
    ${hint ? `<div class="hint">${esc(hint)}</div>` : ''}
  </div>`;

const radios = (key, label, options, hint) => `
  <div class="fld">
    <label>${esc(label)}</label>
    <div class="radios">
      ${options.map((o) => `
        <label class="rd">
          <input type="radio" name="r_${key}" data-key="${key}" value="${esc(o.v)}">
          <span>${esc(o.l)}</span>
        </label>`).join('')}
    </div>
    ${hint ? `<div class="hint">${esc(hint)}</div>` : ''}
  </div>`;

const check = (key, title, sub) => `
  <label class="cb">
    <input type="checkbox" data-key="${key}">
    <div class="ct"><b>${esc(title)}</b>${sub ? `<span>${esc(sub)}</span>` : ''}</div>
  </label>`;

const colour = (key, label, hint) => `
  <div class="fld">
    <label>${esc(label)}</label>
    <div class="colr">
      <input type="color" data-key="${key}" data-colour="1">
      <input class="inp mono" type="text" data-key="${key}" data-colour="1" maxlength="7">
    </div>
    ${hint ? `<div class="hint">${esc(hint)}</div>` : ''}
  </div>`;

const card = (title, sub, body) => `
  <div class="card">
    <div class="card-h"><div class="ht"><h3>${esc(title)}</h3><p>${esc(sub)}</p></div></div>
    <div class="card-b">${body}</div>
  </div>`;

// --- the four tabs ----------------------------------------------------------

function basicTab() {
  return `<div class="g2">
    ${card('Identity', 'How the school is named on reports', `
      ${text('unit_name', 'Unit name', 'Printed at the top of every report')}
      ${text('unit_abbr', 'Abbreviation', 'Used where space is short', { width: 200 })}`)}

    ${card('Cycle anchors', 'Where a week and a month begin', `
      ${select('week_start', 'Start a week from',
        DAYS.map((d, i) => ({ v: String(i), l: d })),
        'Changes how weekly reports are grouped')}
      ${num('month_start_day', 'Start a month from day',
        'Set this to 26 if the school pays from the 26th of the previous month',
        { min: 1, max: 31, suffix: '' })}`)}

    ${card('Cross-day shifts', 'Which day owns a night duty', `
      ${radios('cross_day_belongs_to_first', 'A shift spanning two days counts as',
        [{ v: 'first', l: '1st day shift' }, { v: 'second', l: '2nd day shift' }],
        'A guard arriving 19:00 Monday and leaving 06:00 Tuesday is recorded against the day chosen here')}`)}

    ${card('Time zone limits', 'What is and is not a plausible working period', `
      ${num('longest_zone_min', 'Longest time zone under',
        'Two scans further apart than this are a missed punch, not a long day')}
      ${num('shortest_zone_min', 'Shortest time zone exceeds',
        'A shorter gap is someone testing the sensor')}
      ${num('least_shift_interval_min', 'Least minutes of shift interval',
        'Two blocks of duty closer than this become one duty with a break')}`)}

    ${card('State handling', 'What to do with the keys staff press on the terminal', `
      ${radios('out_state', 'Out state',
        [{ v: 'ignore', l: 'Ignore the state' }, { v: 'as_out', l: 'As Out' },
         { v: 'as_business_out', l: 'As Business Out' }, { v: 'audit', l: 'Audit it' }],
        'Staff routinely press the wrong key; ignoring it makes the time windows decide instead')}
      ${radios('ot_state', 'OT state',
        [{ v: 'ignore', l: 'Ignore the state' }, { v: 'as_out', l: 'As OT' },
         { v: 'as_business_out', l: 'As Business Out' }, { v: 'audit', l: 'Audit it' }])}`)}
  </div>`;
}

function calcTab() {
  return `<div class="g2">
    ${card('Baseline', 'What one full day is worth', `
      ${num('workday_minutes', 'One workday is', 'Clock minutes from on duty to off duty')}
      ${num('min_full_day_min', 'Full day needs at least',
        'Worked minutes below this earn half a day, however punctual the arrival')}
      ${num('dedupe_secs', 'Ignore repeat scans within',
        'Two taps at the sensor are one scan', { suffix: 'secs' })}`)}

    ${card('Grace periods', 'How much lateness is overlooked', `
      ${num('late_after_min', 'Clock-in over', 'minutes past on duty counts as late')}
      ${num('early_after_min', 'Clock-out over', 'minutes before off duty counts as early')}
      <div class="note b">${icon('info')}<div>
        A timetable that sets its own grace overrides these. These fill in for
        timetables that leave it at zero.
      </div></div>`)}

    ${card('Missing punches', 'When only one scan was recorded', `
      ${check('no_clock_in_enabled', 'Handle a missing clock-in', 'Otherwise the day is graded on what is there')}
      <div class="row2">
        ${select('no_clock_in_as', 'Count as', [
          { v: 'Late', l: 'Late' }, { v: 'Absent', l: 'Absent' }])}
        ${num('no_clock_in_min', 'Charge', '', { suffix: 'mins' })}
      </div>
      ${check('no_clock_out_enabled', 'Handle a missing clock-out')}
      <div class="row2">
        ${select('no_clock_out_as', 'Count as', [
          { v: 'EarlyLeave', l: 'Early leave' }, { v: 'Absent', l: 'Absent' }])}
        ${num('no_clock_out_min', 'Charge', '', { suffix: 'mins' })}
      </div>
      ${check('lone_punch_half_day', 'A single scan is half a day',
        'Switch off to record it as an unresolved exception instead')}`)}

    ${card('Absence escalation', 'When lateness stops being lateness', `
      ${num('half_day_after_min', 'Late over', 'minutes becomes a half day')}
      ${check('late_to_absent_enabled', 'Late beyond a limit counts as absent')}
      ${num('late_to_absent_min', 'As late exceeds', 'minutes, count as absent')}
      ${check('early_to_absent_enabled', 'Early leave beyond a limit counts as absent')}
      ${num('early_to_absent_min', 'As early leave exceeds', 'minutes, count as absent')}
      <div class="note y">${icon('warn')}<div>
        These have to run in order — half day before absent — or nobody is ever
        recorded as a half day. The app will refuse a set that grades backwards.
      </div></div>`)}

    ${card('Overtime triggers', 'What earns overtime', `
      ${check('ot_after_shift_enabled', 'Staying past off duty counts as OT')}
      ${num('ot_after_shift_min', 'Interval of leaving counts as OT after')}
      ${check('ot_before_shift_enabled', 'Arriving before on duty counts as OT')}
      ${num('ot_before_shift_min', 'Early arrival counts as OT after')}
      ${num('ot_max_daily_min', 'Most OT in one day',
        'Beyond this the scan is wrong, not the effort heroic')}`)}
  </div>`;
}

function statTab() {
  const sym = (key, label) => `
    <div class="symrow">
      <span class="sl">${esc(label)}</span>
      <input class="inp mono sym" type="text" data-key="${key}" maxlength="4">
    </div>`;

  return `<div class="g2">
    ${card('Symbols in reports', 'One or two characters per state', `
      <div class="symgrid">
        ${sym('sym_normal', 'Normal')}
        ${sym('sym_late', 'Late')}
        ${sym('sym_early', 'Early')}
        ${sym('sym_half_day', 'Half day')}
        ${sym('sym_absent', 'Absent')}
        ${sym('sym_ot', 'Overtime')}
        ${sym('sym_leave', 'Leave')}
        ${sym('sym_holiday', 'Holiday')}
        ${sym('sym_missing', 'Missing punch')}
      </div>`)}

    ${card('Rounding control', 'How part-days are reported', `
      <div class="row2">
        ${num('min_unit', 'Min. unit', '', { min: 0.01, max: 8, step: 0.05, suffix: '' })}
        ${select('min_unit_basis', 'of a', [
          { v: 'workday', l: 'Workday' }, { v: 'hours', l: 'Hours' }])}
      </div>
      ${radios('rounding', 'Round-off control', [
        { v: 'down', l: 'Round down' }, { v: 'off', l: 'Round off' }, { v: 'up', l: 'Round up' }])}
      <div class="note b">${icon('info')}<div id="roundEg"></div></div>`)}

    ${card('Accumulation', 'How the totals are built', `
      ${check('acc_by_times', 'Acc. by times', 'Count occurrences as well as minutes')}
      ${check('round_at_acc', 'Round at Acc.',
        'Round once at the end of the period. Rounding every day and then adding is how a month drifts by hours')}
      ${check('group_by_periods', 'Group by time periods',
        'Break the report down by each block of duty rather than by day')}`)}
  </div>`;
}

function weekendTab() {
  return `<div class="g2">
    ${card('Select the days that are weekend', 'A day ticked here is a rest day for everyone',
      DAYS.map((d, i) => check(`weekend_${i}`, d)).join(''))}

    ${card('Weekend logic and formatting', 'How rest days appear and are paid', `
      ${check('weekend_as_ot', 'Weekend count as OT',
        'Scans on a rest day are recorded as weekend overtime, kept apart from ordinary overtime')}
      ${text('weekend_symbol', 'Weekend symbol in the reports', '', { width: 160 })}
      ${colour('weekend_colour', 'Weekend colour in the reports',
        'Used to shade rest days on the roster calendar and printed sheets')}
      <div class="wkprev" id="wkPrev"></div>`)}
  </div>`;
}

// --- page -------------------------------------------------------------------

export default {
  async mount(host) {
    let draft = await api.getAttendanceRules();
    let saved = JSON.stringify(draft);
    let tab = 'basic';

    host.innerHTML = `
      <div class="subtabs" id="subtabs">
        ${TABS.map((t) => `
          <button data-tab="${t.id}">${icon(t.icon)}<span>${esc(t.label)}</span></button>`).join('')}
        <div class="grow"></div>
        <span class="dirty" id="dirty" hidden>Unsaved changes</span>
        <button class="btn" id="btnRevert">Revert</button>
        <button class="btn pri" id="btnSave">${icon('check')} Save rules</button>
      </div>
      <div id="tabBody"></div>`;

    const body = host.querySelector('#tabBody');
    const dirtyChip = host.querySelector('#dirty');

    const markDirty = () => {
      dirtyChip.hidden = JSON.stringify(draft) === saved;
    };

    /** Push `draft` into whichever controls are currently on screen. */
    function paint() {
      body.querySelectorAll('[data-key]').forEach((el) => {
        const key = el.dataset.key;
        let v = readDraft(key);

        if (el.type === 'checkbox') {
          el.checked = !!v;
        } else if (el.type === 'radio') {
          // Booleans are offered as a pair of radios with string values.
          const want = key === 'cross_day_belongs_to_first' ? (v ? 'first' : 'second') : String(v);
          el.checked = el.value === want;
        } else {
          el.value = v ?? '';
        }
      });
      paintExamples();
    }

    const readDraft = (key) => {
      const m = key.match(/^weekend_(\d)$/);
      if (m) return draft.weekend_days[Number(m[1])];
      return draft[key];
    };

    const writeDraft = (key, value) => {
      const m = key.match(/^weekend_(\d)$/);
      if (m) draft.weekend_days[Number(m[1])] = !!value;
      else draft[key] = value;
    };

    /** Show what the current rounding setting actually does. */
    function paintExamples() {
      const eg = body.querySelector('#roundEg');
      if (eg) {
        const unit = Number(draft.min_unit) || 0.5;
        const mode = draft.rounding;
        const snap = (x) => {
          const n = x / unit;
          const r = mode === 'down' ? Math.floor(n) : mode === 'up' ? Math.ceil(n) : Math.round(n);
          return (r * unit).toFixed(2).replace(/\.?0+$/, '');
        };
        eg.innerHTML = `With these settings, 0.6 of a day is reported as
          <b>${esc(snap(0.6))}</b> and 0.4 as <b>${esc(snap(0.4))}</b>.`;
      }

      const prev = body.querySelector('#wkPrev');
      if (prev) {
        prev.innerHTML = DAYS.map((d, i) => {
          const off = draft.weekend_days[i];
          const bg = off ? draft.weekend_colour : 'transparent';
          return `<span class="wkd${off ? ' off' : ''}" style="${off ? `background:${esc(bg)}` : ''}">
            ${esc(d.slice(0, 3))}${off ? `<b>${esc(draft.weekend_symbol)}</b>` : ''}</span>`;
        }).join('');
      }
    }

    function show(id) {
      tab = id;
      host.querySelectorAll('#subtabs button[data-tab]').forEach((b) =>
        b.classList.toggle('on', b.dataset.tab === id));
      body.innerHTML =
        id === 'basic' ? basicTab()
        : id === 'calc' ? calcTab()
        : id === 'stat' ? statTab()
        : weekendTab();
      paint();
    }

    host.querySelector('#subtabs').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-tab]');
      if (b) show(b.dataset.tab);
    });

    // The − and + buttons drive the box they sit beside, then let the normal
    // input handler below do the rest, so there is still only one place that
    // knows how a value reaches the draft.
    body.addEventListener('click', (e) => {
      const stp = e.target.closest('.stp');
      if (!stp) return;
      const input = body.querySelector(`[data-key="${stp.dataset.for}"]`);
      if (!input) return;
      const step = Number(input.step) || 1;
      const min = input.min === '' ? -Infinity : Number(input.min);
      const max = input.max === '' ? Infinity : Number(input.max);
      const next = (Number(input.value) || 0) + step * Number(stp.dataset.step);
      // Round to the step's own precision, or 0.1 + 0.2 arithmetic leaves
      // 0.30000000000000004 in a box the office has to read.
      const dp = (String(step).split('.')[1] || '').length;
      input.value = Math.min(max, Math.max(min, Number(next.toFixed(dp))));
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });

    // One listener for every control on every tab.
    body.addEventListener('input', (e) => {
      const el = e.target.closest('[data-key]');
      if (!el) return;
      const key = el.dataset.key;

      if (el.type === 'checkbox') {
        writeDraft(key, el.checked);
      } else if (el.type === 'radio') {
        writeDraft(key, key === 'cross_day_belongs_to_first' ? el.value === 'first' : el.value);
      } else if (el.type === 'number') {
        // An empty box is zero, not NaN — NaN would fail the save with a
        // message about the wire format rather than about the rule.
        const n = Number(el.value);
        writeDraft(key, el.value === '' ? 0 : Number.isFinite(n) ? n : 0);
      } else if (el.dataset.colour) {
        writeDraft(key, el.value);
        // Keep the swatch and the hex box in step without re-rendering.
        body.querySelectorAll(`[data-key="${key}"][data-colour]`).forEach((o) => {
          if (o !== el && /^#[0-9a-f]{6}$/i.test(el.value)) o.value = el.value;
        });
      } else {
        writeDraft(key, el.value);
      }
      markDirty();
      paintExamples();
    });

    host.querySelector('#btnSave').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        await api.saveAttendanceRules(draft);
        saved = JSON.stringify(draft);
        markDirty();
        toast('ok', 'Rules saved. Recalculate to apply them to days already recorded.');
      }).catch(() => {}));

    host.querySelector('#btnRevert').addEventListener('click', async () => {
      if (JSON.stringify(draft) === saved) return;
      if (!(await confirmDialog('Discard changes?',
        'The rules will go back to what was last saved.', 'Discard'))) return;
      draft = JSON.parse(saved);
      show(tab);
      markDirty();
    });

    show('basic');
  },
};
