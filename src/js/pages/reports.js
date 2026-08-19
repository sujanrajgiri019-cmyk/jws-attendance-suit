// Reports — pick a report, set the filters, read the grid, then print, export
// or email it.
//
// The grid is built from whatever columns the backend says the chosen report
// has, so all eight reports share one renderer and adding a ninth needs no
// change here.

import { api } from '../api.js';
import {
  icon, esc, toast, modal, confirmDialog, withBusy, duration,
  todayIso, monthStart, statusTag,
} from '../ui.js';
import { dateField, wireDateFields, bsPretty, adPretty } from '../nepali.js';

const PAGE_SIZE = 100;

/** Render one cell according to the column's declared kind. */
function cell(value, kind) {
  if (value === null || value === undefined || value === '') return '<span class="dim">—</span>';
  switch (kind) {
    case 'mins': {
      const n = Number(value);
      return Number.isFinite(n) && n !== 0 ? esc(duration(n)) : '<span class="dim">—</span>';
    }
    case 'pct': {
      const n = Number(value);
      if (!Number.isFinite(n)) return '<span class="dim">—</span>';
      const tone = n >= 95 ? 'g' : n >= 85 ? 'y' : 'r';
      return `<span class="tag ${tone}">${n.toFixed(1)}%</span>`;
    }
    case 'status':
      return statusTag(String(value));
    case 'time':
      return `<span class="mono">${esc(String(value).slice(0, 5))}</span>`;
    case 'date': {
      // Nepali first, English underneath: the school works from the BS date.
      const bs = bsPretty(String(value));
      return bs
        ? `<span class="dcell"><b>${esc(bs)}</b><i>${esc(adPretty(String(value)))}</i></span>`
        : `<span class="mono">${esc(value)}</span>`;
    }
    case 'num':
      return `<span class="mono">${esc(value)}</span>`;
    default:
      return esc(value);
  }
}

const isNumeric = (kind) => kind === 'num' || kind === 'mins' || kind === 'pct';

export default {
  async mount(host) {
    const [kinds, depts, members] = await Promise.all([
      api.reportKinds(),
      api.listDepartments(),
      api.listMembers(),
    ]);

    let key = kinds[0]?.key || 'general';
    let result = null;
    let sortKey = null;
    let sortDir = 1;
    let page = 0;

    host.innerHTML = `
      <div class="split rep">
        <div class="card pane-l">
          <div class="card-h"><div class="ht"><h3>Reports</h3><p>Choose one</p></div></div>
          <div class="pane-list" id="repList">
            ${kinds.map((k) => `
              <button class="pane-item" data-key="${esc(k.key)}">
                <span class="pi-t"><b>${esc(k.label)}</b></span>
              </button>`).join('')}
          </div>
          <div class="card-b bt">
            <button class="btn" style="width:100%" id="repBook">
              ${icon('users')} Email recipients
            </button>
          </div>
        </div>

        <div class="card pane-r">
          <div class="toolbar wrap">
            ${dateField('from', 'From', monthStart(todayIso()), { id: 'fFrom', small: true })}
            ${dateField('to', 'To', todayIso(), { id: 'fTo', small: true })}
            <div class="tb-f"><label>Department</label>
              <select class="inp sm" id="fDept">
                <option value="">All</option>
                ${depts.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join('')}
              </select></div>
            <div class="tb-f"><label>Employee</label>
              <input class="inp sm" id="fWho" list="whoList" placeholder="Name or AC No.">
              <datalist id="whoList">
                ${members.map((m) =>
                  `<option value="${esc(m.full_name)} (${m.enroll_no})"></option>`).join('')}
              </datalist></div>
            <span class="grow"></span>
            <button class="btn pri sm" id="btnRun">${icon('sync')} Generate</button>
            <button class="btn sm" id="btnPrint">${icon('file')} Print</button>
            <button class="btn sm" id="btnExport">${icon('down')} Create report</button>
            <button class="btn sm" id="btnMail">${icon('mail')} Email</button>
          </div>
          <div id="repMeta" class="rep-meta"></div>
          <div class="grid-wrap" id="repGrid"></div>
          <div class="pager" id="repPager"></div>
        </div>
      </div>`;

    const grid = host.querySelector('#repGrid');
    const meta = host.querySelector('#repMeta');
    const pager = host.querySelector('#repPager');

    /** Turn the toolbar into the filter object the backend expects. */
    function filters() {
      const who = host.querySelector('#fWho').value.trim();
      let memberId = null;
      if (who) {
        // The datalist entries end in "(enrolment)". Match on that first, then
        // fall back to an exact name, so typing a name that two people share
        // does not silently pick one of them.
        const byEnroll = /\((\d+)\)\s*$/.exec(who);
        const found = byEnroll
          ? members.find((m) => String(m.enroll_no) === byEnroll[1])
          : members.find((m) => m.full_name.toLowerCase() === who.toLowerCase());
        if (!found) throw new Error(`No member of staff matches "${who}".`);
        memberId = found.id;
      }
      const dept = host.querySelector('#fDept').value;
      return {
        from: host.querySelector('#fFrom').value,
        to: host.querySelector('#fTo').value,
        dept_id: dept ? Number(dept) : null,
        member_id: memberId,
      };
    }

    function paintList() {
      host.querySelectorAll('#repList [data-key]').forEach((b) =>
        b.classList.toggle('on', b.dataset.key === key));
    }

    function sortedRows() {
      if (!result) return [];
      if (!sortKey) return result.rows;
      const kind = result.columns.find((c) => c.key === sortKey)?.kind;
      const numeric = isNumeric(kind);
      return [...result.rows].sort((a, b) => {
        const x = a[sortKey];
        const y = b[sortKey];
        // Blanks sort last whichever way the column is pointing, so an empty
        // clock-out never displaces a real one at the top.
        if (x === null || x === undefined || x === '') return 1;
        if (y === null || y === undefined || y === '') return -1;
        const cmp = numeric
          ? Number(x) - Number(y)
          : String(x).localeCompare(String(y), undefined, { numeric: true });
        return cmp * sortDir;
      });
    }

    function paintGrid() {
      if (!result) {
        grid.innerHTML = `<div class="empty" style="padding:52px 20px">
          <div class="ei">${icon('chart')}</div>
          <b>Nothing generated yet</b>
          <p>Set the dates and press Generate.</p></div>`;
        meta.innerHTML = '';
        pager.innerHTML = '';
        return;
      }

      const rows = sortedRows();
      const pages = Math.max(1, Math.ceil(rows.length / PAGE_SIZE));
      page = Math.min(page, pages - 1);
      const slice = rows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

      meta.innerHTML = `<b>${esc(result.title)}</b>
        <span>${esc(result.subtitle)} · ${rows.length} row${rows.length === 1 ? '' : 's'}</span>`;

      if (!rows.length) {
        grid.innerHTML = `<div class="empty" style="padding:52px 20px">
          <div class="ei">${icon('search')}</div>
          <b>No records in this period</b>
          <p>Try a wider date range, or check that attendance has been recalculated.</p></div>`;
        pager.innerHTML = '';
        return;
      }

      const head = result.columns.map((c) => `
        <th class="${isNumeric(c.kind) ? 'n' : ''}${sortKey === c.key ? ' sorted' : ''}"
            data-sort="${esc(c.key)}">
          ${esc(c.label)}${sortKey === c.key ? (sortDir === 1 ? ' ▲' : ' ▼') : ''}
        </th>`).join('');

      const bodyRows = slice.map((r) => `<tr>${result.columns.map((c) =>
        `<td class="${isNumeric(c.kind) ? 'n' : ''}">${cell(r[c.key], c.kind)}</td>`).join('')}</tr>`).join('');

      const hasTotals = result.totals && Object.keys(result.totals).length;
      const foot = hasTotals
        ? `<tfoot><tr>${result.columns.map((c, i) =>
            i === 0
              ? '<td><b>Total</b></td>'
              : `<td class="${isNumeric(c.kind) ? 'n' : ''}"><b>${
                  result.totals[c.key] !== undefined ? cell(result.totals[c.key], c.kind) : ''
                }</b></td>`).join('')}</tr></tfoot>`
        : '';

      grid.innerHTML = `<table class="tbl grid">
        <thead><tr>${head}</tr></thead><tbody>${bodyRows}</tbody>${foot}</table>`;

      pager.innerHTML = pages > 1
        ? `<button class="btn sm" id="pPrev" ${page === 0 ? 'disabled' : ''}>Previous</button>
           <span>Page ${page + 1} of ${pages}</span>
           <button class="btn sm" id="pNext" ${page >= pages - 1 ? 'disabled' : ''}>Next</button>`
        : '';
    }

    async function run() {
      const f = filters();
      result = await api.runReport(key, f);
      sortKey = null;
      page = 0;
      paintGrid();
    }

    host.querySelector('#repList').addEventListener('click', async (e) => {
      const b = e.target.closest('[data-key]');
      if (!b) return;
      key = b.dataset.key;
      paintList();
      // Switching report with a grid already on screen should show the new one,
      // not leave the old figures under a new title.
      if (result) {
        try {
          await run();
        } catch (err) {
          toast('err', err.message || String(err));
        }
      }
    });

    // withBusy already reports the failure; catching again here would show the
    // same message twice.
    host.querySelector('#btnRun').addEventListener('click', (e) =>
      withBusy(e.currentTarget, run).catch(() => {}));

    grid.addEventListener('click', (e) => {
      const th = e.target.closest('[data-sort]');
      if (th) {
        const k = th.dataset.sort;
        sortDir = sortKey === k ? -sortDir : 1;
        sortKey = k;
        paintGrid();
        return;
      }
      if (e.target.id === 'pPrev') { page--; paintGrid(); }
      if (e.target.id === 'pNext') { page++; paintGrid(); }
    });
    pager.addEventListener('click', (e) => {
      if (e.target.id === 'pPrev') { page--; paintGrid(); }
      if (e.target.id === 'pNext') { page++; paintGrid(); }
    });

    // --- print ---
    host.querySelector('#btnPrint').addEventListener('click', () => {
      if (!result) return toast('inf', 'Generate the report first.');
      const w = window.open('', '_blank');
      if (!w) return toast('err', 'The print window was blocked.');
      // Reuse the backend's own HTML rendering so the printed sheet and the
      // emailed one are the same document.
      api.exportReport(key, filters(), 'html', tempPath('print', 'html'))
        .then(async (p) => {
          w.document.write(`<p style="font:14px sans-serif">Saved to ${esc(p)} — opening…</p>`);
          await api.openPath(p);
          w.close();
        })
        .catch((e) => {
          w.close();
          toast('err', e.message);
        });
    });

    const tempPath = (name, ext) =>
      `${name}-${key}-${todayIso()}.${ext}`;

    // --- export ---
    host.querySelector('#btnExport').addEventListener('click', async () => {
      if (!result) return toast('inf', 'Generate the report first.');
      const fmt = await modal({
        title: 'Create report',
        subtitle: result.title,
        body: `<div class="fld"><label>Format</label>
            <select class="inp" name="fmt">
              <option value="csv">Excel / CSV</option>
              <option value="html">Web page (HTML)</option>
              <option value="pdf">Printable page (PDF via print dialog)</option>
            </select></div>
          <div class="fld"><label>File name</label>
            <input class="inp" name="path" value="${esc(tempPath('report', 'csv'))}"></div>
          <div class="note b">${icon('info')}<div>
            The file is written next to the app's data folder unless you type a
            full path such as D:\\Reports\\august.csv
          </div></div>`,
        buttons: [
          { label: 'Cancel', value: null },
          {
            label: 'Create', kind: 'pri',
            onClick: async (ov) => {
              const f = ov.querySelector('[name=fmt]').value;
              let p = ov.querySelector('[name=path]').value.trim();
              if (!p) throw new Error('Give the file a name.');
              // Keep the extension honest, so Excel opens what it expects.
              const want = f === 'csv' ? '.csv' : '.html';
              if (!p.toLowerCase().endsWith(want)) p = p.replace(/\.[^.\\/]*$/, '') + want;
              return api.exportReport(key, filters(), f, p);
            },
          },
        ],
      });
      if (fmt) {
        toast('ok', `Saved to ${fmt}`);
        await api.openPath(fmt).catch(() => {});
      }
    });

    // --- email ---
    host.querySelector('#btnMail').addEventListener('click', async () => {
      const people = (await api.listRecipients()).filter((r) => r.active);
      if (!people.length) {
        if (await confirmDialog('No recipients yet',
          'Nobody has been added to the email list. Open the recipient book now?',
          'Open list', 'pri')) openBook();
        return;
      }

      const f = filters();
      const picked = await modal({
        title: 'Email this report',
        subtitle: `${kinds.find((k) => k.key === key)?.label} · ${f.from} to ${f.to}`,
        wide: true,
        body: `
          <div class="fld"><label>Send to</label>
            <div class="picklist">
              ${people.map((p) => `
                <label class="cb">
                  <input type="checkbox" data-rid="${p.id}"
                    ${p.reports.includes(key) ? 'checked' : ''}>
                  <div class="ct"><b>${esc(p.name)}</b>
                    <span>${esc(p.email)}${p.role ? ` · ${esc(p.role)}` : ''}${
                      p.dept_name ? ` · ${esc(p.dept_name)} only` : ''}</span></div>
                </label>`).join('')}
            </div></div>
          <div class="fld"><label>Covering note (optional)</label>
            <textarea class="inp" name="note" rows="3"
              placeholder="Anything you want said above the table"></textarea></div>
          <div class="note b">${icon('info')}<div>
            Anyone tied to a department receives the same report narrowed to their
            own staff. Ticked by default are the people already on this report's list.
          </div></div>`,
        buttons: [
          { label: 'Cancel', value: null },
          {
            label: 'Send', kind: 'pri',
            onClick: async (ov) => {
              const ids = [...ov.querySelectorAll('[data-rid]:checked')]
                .map((c) => Number(c.dataset.rid));
              if (!ids.length) throw new Error('Tick at least one person.');
              const note = ov.querySelector('[name=note]').value;
              return api.sendReportEmail(key, f, ids, note);
            },
          },
        ],
      });

      if (!picked) return;
      if (picked.failed) {
        toast('err', `${picked.sent} sent, ${picked.failed} failed. ${picked.details.at(-1) || ''}`);
      } else {
        toast('ok', `Sent to ${picked.sent} recipient${picked.sent === 1 ? '' : 's'}`);
      }
    });

    // --- the recipient book ---
    async function openBook() {
      const refresh = async () => {
        const people = await api.listRecipients();
        return people.length
          ? people.map((p) => `
              <div class="rcp">
                <div class="rc-t">
                  <b>${esc(p.name)}${p.active ? '' : ' (off)'}</b>
                  <span>${esc(p.email)}</span>
                  <span class="dim">${esc(p.role || '')}${
                    p.dept_name ? ` · ${esc(p.dept_name)}` : ' · whole school'}</span>
                  <span class="dim">${p.reports.length
                    ? `${p.reports.length} report${p.reports.length === 1 ? '' : 's'} on their list`
                    : 'Not on any list'}</span>
                </div>
                <button class="btn sm" data-edit="${p.id}">Edit</button>
                <button class="btn sm dan" data-del="${p.id}">Delete</button>
              </div>`).join('')
          : '<div class="pane-empty">Nobody added yet.</div>';
      };

      await modal({
        title: 'Report recipients',
        subtitle: 'School officials who receive attendance reports',
        wide: true,
        body: `<div id="rcpList">${await refresh()}</div>
          <button class="btn pri" id="rcpAdd" style="margin-top:12px">
            ${icon('plus')} Add recipient</button>`,
        buttons: [{ label: 'Close', value: null }],
        onMount: (ov) => {
          const relist = async () => {
            ov.querySelector('#rcpList').innerHTML = await refresh();
          };

          ov.addEventListener('click', async (e) => {
            const del = e.target.closest('[data-del]');
            if (del) {
              await api.deleteRecipient(Number(del.dataset.del));
              await relist();
              return;
            }
            const ed = e.target.closest('[data-edit]');
            const add = e.target.closest('#rcpAdd');
            if (!ed && !add) return;

            const all = await api.listRecipients();
            const r = ed
              ? all.find((x) => x.id === Number(ed.dataset.edit))
              : { id: null, name: '', email: '', role: '', dept_id: null, reports: [], active: true };

            const saved = await modal({
              title: r.id ? 'Edit recipient' : 'Add recipient',
              subtitle: 'Who receives reports, and which ones',
              body: `
                <div class="grid2">
                  <div class="fld"><label>Name</label>
                    <input class="inp" name="name" value="${esc(r.name)}"></div>
                  <div class="fld"><label>Role</label>
                    <input class="inp" name="role" value="${esc(r.role)}"
                      placeholder="Principal, Accountant…"></div>
                </div>
                <div class="fld"><label>Email address</label>
                  <input class="inp" name="email" type="email" value="${esc(r.email)}"></div>
                <div class="fld"><label>Sees</label>
                  <select class="inp" name="dept">
                    <option value="">The whole school</option>
                    ${depts.map((d) => `<option value="${d.id}" ${
                      r.dept_id === d.id ? 'selected' : ''}>${esc(d.name)} only</option>`).join('')}
                  </select></div>
                <div class="fld"><label>On the list for</label>
                  <div class="picklist">
                    ${kinds.map((k) => `
                      <label class="cb">
                        <input type="checkbox" data-rep="${esc(k.key)}"
                          ${r.reports.includes(k.key) ? 'checked' : ''}>
                        <div class="ct"><b>${esc(k.label)}</b></div>
                      </label>`).join('')}
                  </div></div>
                <label class="cb"><input type="checkbox" name="active" ${r.active ? 'checked' : ''}>
                  <div class="ct"><b>Active</b>
                    <span>Switch off to keep the address without sending to it</span></div></label>`,
              buttons: [
                { label: 'Cancel', value: null },
                {
                  label: 'Save', kind: 'pri',
                  onClick: async (o2) => {
                    const g = (n) => o2.querySelector(`[name=${n}]`);
                    const dept = g('dept').value;
                    return api.saveRecipient({
                      id: r.id,
                      name: g('name').value,
                      email: g('email').value,
                      role: g('role').value,
                      dept_id: dept ? Number(dept) : null,
                      reports: [...o2.querySelectorAll('[data-rep]:checked')]
                        .map((c) => c.dataset.rep),
                      active: g('active').checked,
                    });
                  },
                },
              ],
            });
            if (saved) await relist();
          });
        },
      });
    }

    host.querySelector('#repBook').addEventListener('click', openBook);

    wireDateFields(host);
    paintList();
    paintGrid();
    try {
      await run();
    } catch {
      /* an empty database is not an error worth a toast on first open */
    }
  },
};
