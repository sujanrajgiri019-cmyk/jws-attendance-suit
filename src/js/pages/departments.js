import { api } from '../api.js';
import {
  icon, esc, table, modal, toast, confirmDialog, readForm,
  PALETTE, initials, colourFor, todayIso,
} from '../ui.js';

export default {
  async mount(host) {
    host.innerHTML = `
      <div class="tbar">
        <div class="sp"></div>
        <button class="btn pri" id="btnAdd">${icon('plus')} New department</button>
      </div>
      <div class="g3" id="cards"></div>
      <div class="card mt14">
        <div class="card-h"><div class="ht"><h3>Department List</h3>
          <p>Head, default timetable and headcount</p></div></div>
        <div class="tbl-wrap"><table class="tbl" id="tbl"></table></div>
      </div>`;

    async function load() {
      const today = todayIso();
      const [depts, stats, members, timetables] = await Promise.all([
        api.listDepartments(), api.departmentStats(today), api.listMembers(), api.listTimetables(),
      ]);
      const statFor = (id) => stats.find((s) => s.id === id) || { present: 0, total: 0, rate: 0 };

      host.querySelector('#cards').innerHTML = depts.map((d) => {
        const s = statFor(d.id);
        const mem = members.filter((m) => m.dept_id === d.id);
        return `<div class="card">
          <div class="card-h">
            <div style="width:36px;height:36px;border-radius:9px;background:${esc(d.colour)}1a;
              display:grid;place-items:center;color:${esc(d.colour)};font-weight:700;font-size:12px;flex:none">
              ${esc(d.code)}</div>
            <div class="ht"><h3>${esc(d.name)}</h3>
              <p>${d.member_count} member${d.member_count === 1 ? '' : 's'} · Head: ${esc(d.head_name || 'not set')}</p></div>
          </div>
          <div class="card-b" style="padding-top:12px">
            <div style="display:flex;justify-content:space-between;font-size:12px;margin-bottom:6px">
              <span style="color:var(--ink-3)">Marked in today</span>
              <b>${s.present}/${s.total} · ${s.rate.toFixed(0)}%</b></div>
            <div class="bar" style="margin-bottom:13px">
              <i style="width:${s.rate}%;background:${esc(d.colour)}"></i></div>
            <div style="display:flex;min-height:28px">
              ${mem.slice(0, 7).map((m, i) => `<div title="${esc(m.full_name)}"
                style="width:28px;height:28px;border-radius:50%;background:${colourFor(m.enroll_no)};
                color:#fff;display:grid;place-items:center;font-size:10px;font-weight:700;
                border:2px solid #fff;margin-left:${i ? '-8px' : '0'}">${esc(initials(m.full_name))}</div>`).join('')}
              ${mem.length > 7 ? `<div style="width:28px;height:28px;border-radius:50%;background:#EEF0F2;
                color:#5A5F66;display:grid;place-items:center;font-size:10px;font-weight:700;
                border:2px solid #fff;margin-left:-8px">+${mem.length - 7}</div>` : ''}
              ${mem.length === 0 ? '<span style="font-size:12px;color:var(--ink-4);align-self:center">No staff assigned</span>' : ''}
            </div>
          </div>
          <div class="card-f" style="display:flex;gap:7px">
            <button class="btn sm" style="flex:1" data-edit="${d.id}">${icon('edit')} Edit</button>
            <button class="btn sm ic dan" data-del="${d.id}" title="Delete">${icon('trash')}</button>
          </div>
        </div>`;
      }).join('');

      table(host.querySelector('#tbl'), [
        {
          label: 'Code',
          get: (d) => `<span class="tag n" style="background:${esc(d.colour)}18;color:${esc(d.colour)}">${esc(d.code)}</span>`,
        },
        { label: 'Department', get: (d) => `<b>${esc(d.name)}</b>` },
        { label: 'Head of department', get: (d) => esc(d.head_name || '—') },
        { label: 'Default timetable', get: (d) => esc(d.timetable_name || '—') },
        { label: 'Members', cls: 'num', get: (d) => d.member_count },
        { label: 'Marked in today', cls: 'num', get: (d) => statFor(d.id).present },
        {
          label: 'Rate', width: '150px',
          get: (d) => {
            const s = statFor(d.id);
            return `<div style="display:flex;align-items:center;gap:9px">
              <div class="bar" style="flex:1"><i style="width:${s.rate}%;background:${esc(d.colour)}"></i></div>
              <b style="font-size:12px;width:38px;text-align:right">${s.rate.toFixed(0)}%</b></div>`;
          },
        },
      ], depts, { empty: 'No departments yet' });

      host.querySelector('#cards').onclick = async (e) => {
        const edit = e.target.closest('[data-edit]');
        if (edit) {
          const d = depts.find((x) => x.id === Number(edit.dataset.edit));
          if (await dialog(d, members, timetables)) await load();
          return;
        }
        const del = e.target.closest('[data-del]');
        if (del) {
          const d = depts.find((x) => x.id === Number(del.dataset.del));
          if (!(await confirmDialog('Delete this department?',
            `${d.name} will be removed. Staff must be moved elsewhere first.`, 'Delete'))) return;
          try {
            await api.deleteDepartment(d.id);
            toast('ok', `${d.name} deleted`);
            await load();
          } catch (err) {
            toast('err', err.message);
          }
        }
      };

      host.querySelector('#btnAdd').onclick = async () => {
        if (await dialog({ colour: PALETTE[0] }, members, timetables)) await load();
      };
    }

    await load();
  },
};

function dialog(d, members, timetables) {
  const isNew = !d.id;
  return modal({
    title: isNew ? 'New department' : `Edit ${d.name}`,
    subtitle: 'Used for grouping, scheduling and reports',
    body: `
      <div class="grid2">
        <div class="fld"><label class="req">Department name</label>
          <input class="inp" name="name" value="${esc(d.name || '')}" placeholder="Science Faculty"></div>
        <div class="fld"><label>Short code</label>
          <input class="inp" name="code" maxlength="5" value="${esc(d.code || '')}" placeholder="SCI"></div>
      </div>
      <div class="fld"><label>Head of department</label>
        <select class="inp" name="head_member_id">
          <option value="">Not assigned</option>
          ${members.map((m) => `<option value="${m.id}" ${d.head_member_id === m.id ? 'selected' : ''}>${esc(m.full_name)}</option>`).join('')}
        </select></div>
      <div class="fld"><label>Default timetable</label>
        <select class="inp" name="default_timetable_id">
          <option value="">Not set</option>
          ${timetables.map((t) => `<option value="${t.id}" ${d.default_timetable_id === t.id ? 'selected' : ''}>${esc(t.name)}</option>`).join('')}
        </select>
        <div class="hint">Applies to any member without their own timetable.</div></div>
      <div class="fld"><label>Colour</label>
        <div style="display:flex;gap:8px;flex-wrap:wrap" id="swatches">
          ${PALETTE.map((c) => `<span data-colour="${c}" style="width:28px;height:28px;border-radius:7px;
            background:${c};cursor:pointer;border:2px solid ${c === (d.colour || PALETTE[0]) ? 'var(--ink)' : 'transparent'}"></span>`).join('')}
        </div>
        <input type="hidden" name="colour" value="${esc(d.colour || PALETTE[0])}"></div>`,
    buttons: [
      { label: 'Cancel', value: null },
      {
        label: isNew ? 'Create' : 'Save', kind: 'pri',
        onClick: async (ov) => {
          const f = readForm(ov);
          if (!f.name?.trim()) throw new Error('Department name is required.');
          await api.saveDepartment({
            id: d.id ?? null,
            name: f.name.trim(),
            code: (f.code || f.name.slice(0, 3)).toUpperCase(),
            colour: f.colour,
            head_member_id: f.head_member_id ? Number(f.head_member_id) : null,
            default_timetable_id: f.default_timetable_id ? Number(f.default_timetable_id) : null,
          });
          toast('ok', isNew ? 'Department created' : 'Department saved');
          return true;
        },
      },
    ],
  }).then((r) => {
    return r;
  });
}

// Colour swatch selection is wired globally because the modal body is static
// HTML; this keeps the dialog markup declarative.
document.addEventListener('click', (e) => {
  const sw = e.target.closest('[data-colour]');
  if (!sw) return;
  const wrap = sw.closest('#swatches');
  if (!wrap) return;
  wrap.querySelectorAll('[data-colour]').forEach((s) => (s.style.border = '2px solid transparent'));
  sw.style.border = '2px solid var(--ink)';
  const input = wrap.parentElement.querySelector('[name=colour]');
  if (input) input.value = sw.dataset.colour;
});
