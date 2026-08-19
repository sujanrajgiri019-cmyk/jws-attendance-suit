import { api } from '../api.js';
import { icon, esc, toast, withBusy, todayIso, monthStart } from '../ui.js';

const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

// Every switch on this screen maps to a row in the `rules` table. Adding a new
// one here is all it takes — the backend stores arbitrary keys.
const TOGGLES = {
  penalty: [
    ['late_penalty_enabled', 'Deduct half a day after repeated lateness', 'Counted per Nepali month'],
    ['warn_email_on_3rd_late', 'Email a warning on the third late arrival', 'Sent from the school address'],
    ['exempt_heads_from_late', 'Exempt department heads from the late penalty', ''],
  ],
  punch: [
    ['first_last_punch', 'First scan is the check-in, last is the check-out', 'Anything in between is ignored'],
    ['require_both_punches', 'Flag days that are missing a scan', 'So the office can correct them'],
    ['lone_punch_half_day', 'Treat a single scan as a half day', 'Instead of marking the whole day absent'],
    ['allow_manual_edit', 'Allow the office to correct a day by hand', 'Every change is written to the audit trail'],
    ['lock_after_close', 'Lock records once the month is closed', 'Stops changes after payroll has run'],
  ],
  overtime: [
    ['count_ot', 'Count overtime past the end of duty', ''],
    ['ot_needs_approval', 'Overtime must be approved before it counts', 'By the department head'],
    ['holidays_paid', 'Holidays are paid', 'Excluded from absence counts'],
    ['sandwich_rule', 'Leave either side of a holiday consumes the holiday', ''],
  ],
  automatic: [
    ['email_absentees', 'Email staff who were not marked in', 'Sent each working day'],
    ['daily_summary_principal', 'Daily summary to the Principal', 'With the day’s figures'],
    ['weekly_dept_report', 'Weekly report to each department head', 'Sunday morning'],
    ['recompute_on_rule_change', 'Rebuild the current month when a rule changes', 'Replays the raw scans'],
  ],
};

const NUMBERS = [
  ['late_grace_min', 'Late grace (minutes)', 'Arriving within this many minutes of the start is not late'],
  ['early_grace_min', 'Early-leaving grace (minutes)', ''],
  ['half_day_after_min', 'Half day if late by more than (minutes)', ''],
  ['absent_after_min', 'Absent if late by more than (minutes)', ''],
  ['min_full_day_min', 'Minutes of work needed for a full day', 'Must be achievable within the shift, after the break'],
  ['dedupe_window_sec', 'Ignore repeat scans within (seconds)', 'Stops a double-tap counting twice'],
  ['min_ot_block_min', 'Minimum overtime block (minutes)', 'Shorter spells are not counted'],
  ['late_penalty_count', 'Late arrivals before a penalty', ''],
  ['flag_below_percent', 'Flag staff below this attendance rate (%)', ''],
];

const toggleRow = ([key, label, hint], values) => `
  <div class="rowbox">
    <div class="rt"><b>${esc(label)}</b>${hint ? `<span>${esc(hint)}</span>` : ''}</div>
    <label class="tg"><input type="checkbox" data-rule="${key}"
      ${isOn(values[key]) ? 'checked' : ''}><i></i></label>
  </div>`;

const isOn = (v) => ['1', 'true', 'yes', 'on'].includes(String(v ?? '').toLowerCase());

export default {
  async mount(host) {
    const values = await api.getRules();
    const workingDays = String(values.working_days || '0,1,2,3,4,5').split(',').map(Number);

    host.innerHTML = `
      <div class="tbar">
        <div class="sp"></div>
        <button class="btn" id="btnReset">Reload saved</button>
        <button class="btn pri" id="btnSave">${icon('save')} Save rules</button>
      </div>

      <div class="g2">
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Working Days</h3>
            <p>Which days of the week staff are expected</p></div></div>
          <div class="card-b">
            <div style="display:flex;gap:6px;flex-wrap:wrap" id="days">
              ${DAYS.map((d, i) => `<button type="button" class="tag ${workingDays.includes(i) ? 'o' : 'n'}"
                data-day="${i}" style="height:32px;padding:0 14px;border-radius:8px;font-size:12.5px;
                border:0;cursor:pointer">${d}</button>`).join('')}
            </div>
            <div class="hint" style="margin-top:8px">
              Saturday is the weekly holiday at JWS, so it is off by default.
              A day switched off here is recorded as a weekly off, not an absence.
            </div>
            <div class="note b" style="margin-top:14px">${icon('info')}<div>
              Individual shift times live under <b>Timetables</b>. These rules apply on top of whichever
              shift a member of staff is scheduled for.
            </div></div>
          </div>
        </div>

        <div class="card">
          <div class="card-h"><div class="ht"><h3>Thresholds</h3>
            <p>The numbers the calculation uses</p></div></div>
          <div class="card-b">
            <div class="grid2">
              ${NUMBERS.map(([key, label, hint]) => `
                <div class="fld"><label>${esc(label)}</label>
                  <input class="inp" type="number" min="0" data-rule="${key}"
                         value="${esc(values[key] ?? '0')}">
                  ${hint ? `<div class="hint">${esc(hint)}</div>` : ''}</div>`).join('')}
            </div>
          </div>
        </div>
      </div>

      <div class="g2 mt14">
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Punch Handling</h3>
            <p>How raw scans become one day's record</p></div></div>
          <div class="card-b">${TOGGLES.punch.map((t) => toggleRow(t, values)).join('')}</div>
        </div>
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Lateness &amp; Penalty</h3>
            <p>What happens when someone is repeatedly late</p></div></div>
          <div class="card-b">${TOGGLES.penalty.map((t) => toggleRow(t, values)).join('')}</div>
        </div>
      </div>

      <div class="g2 mt14">
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Overtime &amp; Leave</h3>
            <p>Extra hours and absence categories</p></div></div>
          <div class="card-b">${TOGGLES.overtime.map((t) => toggleRow(t, values)).join('')}</div>
        </div>
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Automatic Actions</h3>
            <p>What the system does without being asked</p></div></div>
          <div class="card-b">${TOGGLES.automatic.map((t) => toggleRow(t, values)).join('')}</div>
        </div>
      </div>`;

    // --- working day chips ---
    host.querySelector('#days').addEventListener('click', (e) => {
      const b = e.target.closest('[data-day]');
      if (!b) return;
      const on = b.classList.toggle('o');
      b.classList.toggle('n', !on);
    });

    host.querySelector('#btnReset').addEventListener('click', () => {
      toast('inf', 'Reloading saved rules');
      import('../main.js').then((m) => m.go('rules'));
    });

    host.querySelector('#btnSave').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const payload = {};
        host.querySelectorAll('[data-rule]').forEach((el) => {
          payload[el.dataset.rule] = el.type === 'checkbox'
            ? (el.checked ? '1' : '0')
            : String(el.value ?? '');
        });
        payload.working_days = [...host.querySelectorAll('#days .o')]
          .map((b) => b.dataset.day).join(',');

        // Guard the one setting that can silently break every record. A
        // full-day threshold longer than the main shift can produce would mark
        // every punctual member of staff as a half day.
        //
        // Only the shift most staff are on is checked: a deliberately short
        // shift like "Half Day" has a small capacity by design, and each shift
        // carries its own threshold anyway. The Timetables screen flags any
        // individual shift whose own threshold is unachievable.
        const minFull = Number(payload.min_full_day_min);
        if (Number.isFinite(minFull) && minFull > 0) {
          const shifts = await api.listShifts();
          const main = shifts
            .filter((s) => s.used_in > 0)
            .sort((a, b) => b.used_in - a.used_in)[0];
          if (main) {
            const capacity = minutesBetween(main.start_time, main.end_time) - main.break_min;
            if (capacity < minFull) {
              throw new Error(
                `A full day of ${minFull} minutes is longer than the ${main.name} shift can produce ` +
                `(${capacity} minutes once the ${main.break_min}-minute break is deducted). ` +
                `Everyone on that shift would be recorded as a half day. ` +
                `Lower this value or lengthen the shift.`,
              );
            }
          }
        }

        await api.setRules(payload);

        if (isOn(payload.recompute_on_rule_change)) {
          const n = await api.recompute(monthStart(todayIso()), todayIso());
          toast('ok', `Rules saved · ${n} day records rebuilt`);
        } else {
          toast('ok', 'Rules saved');
        }
      }).catch(() => {}),
    );
  },
};

function minutesBetween(start, end) {
  const m = (t) => {
    const [h, mm] = String(t).split(':').map(Number);
    return h * 60 + mm;
  };
  let d = m(end) - m(start);
  if (d <= 0) d += 24 * 60;
  return d;
}
