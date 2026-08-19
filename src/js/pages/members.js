import { api } from '../api.js';
import {
  icon, esc, table, person, modal, toast, withBusy, confirmDialog,
  loadingTable, readForm,
} from '../ui.js';
import { dateField, wireDateFields } from '../nepali.js';

const PRIVILEGES = [
  [0, 'User'], [2, 'Enroller'], [6, 'Manager'], [14, 'Super Admin'],
];
const privName = (p) => (PRIVILEGES.find(([v]) => v === Number(p)) || [0, 'User'])[1];

let selected = new Set();

export default {
  async mount(host) {
    selected = new Set();

    const depts = await api.listDepartments();
    const timetables = await api.listShifts();

    host.innerHTML = `
      <div class="tbar">
        <div class="srch">${icon('search')}
          <input class="inp" id="q" placeholder="Search name, staff ID, card or enrolment…">
        </div>
        <select class="inp" style="width:180px" id="fDept">
          <option value="">All departments</option>
          ${depts.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join('')}
        </select>
        <select class="inp" style="width:150px" id="fStatus">
          <option value="">All status</option>
          <option>Active</option><option>Inactive</option><option>On Leave</option>
        </select>
        <div class="sp"></div>
        <button class="btn" id="btnExport">${icon('file')} Export CSV</button>
        <button class="btn pri" id="btnAdd">${icon('plus')} Add member</button>
      </div>

      <div class="card">
        <div class="card-h">
          <div class="ht"><h3>Staff Members</h3><p id="count">Loading…</p></div>
          <div class="btn-grp">
            <button class="btn sm" id="bulkDept" disabled>Change department</button>
            <button class="btn sm" id="bulkUpload" disabled>Send to terminal</button>
            <button class="btn sm dan" id="bulkDelete" disabled>${icon('trash')} Delete</button>
          </div>
        </div>
        <div class="tbl-wrap" style="max-height:calc(100vh - 250px)">
          <table class="tbl" id="tbl"></table>
        </div>
      </div>`;

    const tblEl = host.querySelector('#tbl');
    const countEl = host.querySelector('#count');
    let rows = [];

    async function load() {
      loadingTable(tblEl, 10);
      rows = await api.listMembers(
        host.querySelector('#q').value.trim(),
        Number(host.querySelector('#fDept').value) || null,
        host.querySelector('#fStatus').value,
      );
      render();
    }

    function render() {
      countEl.textContent =
        `${rows.length} member${rows.length === 1 ? '' : 's'}` +
        (selected.size ? ` · ${selected.size} selected` : '');

      const allChecked = rows.length > 0 && rows.every((r) => selected.has(r.id));

      table(tblEl, [
        {
          label: '', width: '34px',
          get: (r) => `<input type="checkbox" data-pick="${r.id}" ${selected.has(r.id) ? 'checked' : ''}
            style="width:16px;height:16px;accent-color:#F16522;cursor:pointer">`,
        },
        { label: 'Enroll', cls: 'num mono', get: (r) => r.enroll_no },
        { label: 'Name', get: (r) => person(r.full_name, r.email || r.staff_id || '', r.enroll_no) },
        { label: 'Staff ID', cls: 'mono', get: (r) => esc(r.staff_id || '—') },
        {
          label: 'Department',
          get: (r) => r.dept_name
            ? `<span class="tag n" style="background:${esc(r.dept_colour)}18;color:${esc(r.dept_colour)}">${esc(r.dept_code)}</span>
               <span style="margin-left:5px">${esc(r.dept_name)}</span>`
            : '<span style="color:var(--ink-4)">Unassigned</span>',
        },
        { label: 'Designation', get: (r) => esc(r.designation || '—') },
        { label: 'Card', cls: 'mono', get: (r) => esc(r.card_no || '—') },
        {
          label: 'FP', cls: 'ctr',
          get: (r) => `<span class="tag ${r.fp_count >= 2 ? 'g' : r.fp_count ? 'y' : 'n'}">${r.fp_count}</span>`,
        },
        {
          label: 'Privilege',
          get: (r) => (Number(r.privilege) === 0
            ? '<span style="color:var(--ink-3)">User</span>'
            : `<span class="tag o">${esc(privName(r.privilege))}</span>`),
        },
        {
          label: 'Status',
          get: (r) => `<span class="tag ${r.status === 'Active' ? 'g' : r.status === 'On Leave' ? 'v' : 'n'}">${esc(r.status)}</span>`,
        },
        {
          label: 'Actions', cls: 'ctr',
          get: (r) => `<div style="display:flex;gap:5px;justify-content:center">
            <button class="btn sm ic" data-edit="${r.id}" title="Edit">${icon('edit')}</button>
            <button class="btn sm ic dan" data-del="${r.id}" title="Delete">${icon('trash')}</button>
          </div>`,
        },
      ], rows, {
        empty: 'No members match',
        emptyHint: 'Adjust the search or filters, or add a new member.',
      });

      // Header select-all checkbox.
      const th = tblEl.querySelector('thead th');
      if (th) {
        th.innerHTML = `<input type="checkbox" id="pickAll" ${allChecked ? 'checked' : ''}
          style="width:16px;height:16px;accent-color:#F16522;cursor:pointer">`;
      }

      for (const [id, on] of [['bulkDept', 1], ['bulkUpload', 1], ['bulkDelete', 1]]) {
        host.querySelector(`#${id}`).disabled = selected.size === 0 && on;
      }
    }

    // --- events ---
    let t;
    host.querySelector('#q').addEventListener('input', () => {
      clearTimeout(t);
      t = setTimeout(load, 220);
    });
    host.querySelector('#fDept').addEventListener('change', load);
    host.querySelector('#fStatus').addEventListener('change', load);

    tblEl.addEventListener('change', (e) => {
      if (e.target.id === 'pickAll') {
        rows.forEach((r) => (e.target.checked ? selected.add(r.id) : selected.delete(r.id)));
        render();
        return;
      }
      const pick = e.target.dataset?.pick;
      if (pick) {
        const id = Number(pick);
        e.target.checked ? selected.add(id) : selected.delete(id);
        render();
      }
    });

    tblEl.addEventListener('click', async (e) => {
      const edit = e.target.closest('[data-edit]');
      if (edit) {
        const m = rows.find((r) => r.id === Number(edit.dataset.edit));
        if (await editMember(m, depts, timetables)) await load();
        return;
      }
      const del = e.target.closest('[data-del]');
      if (del) {
        const m = rows.find((r) => r.id === Number(del.dataset.del));
        const ok = await confirmDialog(
          'Delete this member?',
          `${m.full_name} and all their attendance history will be removed, and they will be deleted from the terminal. This cannot be undone.`,
          'Delete member',
        );
        if (!ok) return;
        try {
          await api.deleteMembers([m.id]);
          toast('ok', `${m.full_name} deleted`);
          selected.delete(m.id);
          await load();
        } catch (err) {
          toast('err', err.message);
        }
      }
    });

    host.querySelector('#btnAdd').addEventListener('click', async () => {
      const next = rows.length ? Math.max(...rows.map((r) => r.enroll_no)) + 1 : 1;
      if (await editMember({ enroll_no: next, status: 'Active', privilege: 0 }, depts, timetables)) {
        await load();
      }
    });

    host.querySelector('#bulkDept').addEventListener('click', async () => {
      const picked = await modal({
        title: 'Change department',
        subtitle: `${selected.size} member${selected.size === 1 ? '' : 's'} selected`,
        body: `<div class="fld"><label>Move to</label>
          <select class="inp" name="dept">
            ${depts.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join('')}
          </select></div>`,
        buttons: [
          { label: 'Cancel', value: null },
          {
            label: 'Move', kind: 'pri',
            onClick: (ov) => Number(ov.querySelector('[name=dept]').value),
          },
        ],
      });
      if (!picked) return;
      await api.setMembersDepartment([...selected], picked);
      toast('ok', `${selected.size} moved`);
      selected.clear();
      await load();
    });

    host.querySelector('#bulkUpload').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const n = await api.uploadUsers([...selected]);
        toast('ok', `${n} update${n === 1 ? '' : 's'} queued for the terminal`);
      }).catch(() => {}),
    );

    host.querySelector('#bulkDelete').addEventListener('click', async () => {
      const ok = await confirmDialog(
        `Delete ${selected.size} members?`,
        'Their attendance history will be removed and they will be deleted from the terminal. This cannot be undone.',
        'Delete them',
      );
      if (!ok) return;
      await api.deleteMembers([...selected]);
      toast('ok', `${selected.size} deleted`);
      selected.clear();
      await load();
    });

    host.querySelector('#btnExport').addEventListener('click', () => {
      const head = 'Enrolment,Staff ID,Name,Department,Designation,Mobile,Email,Card,Privilege,Status\n';
      const csv = head + rows.map((r) => [
        r.enroll_no, r.staff_id || '', `"${(r.full_name || '').replace(/"/g, '""')}"`,
        r.dept_name || '', r.designation || '', r.mobile || '', r.email || '',
        r.card_no || '', privName(r.privilege), r.status,
      ].join(',')).join('\n');
      download('jws-members.csv', csv);
      toast('ok', 'members.csv downloaded');
    });

    await load();
  },
};

function download(name, text) {
  const blob = new Blob([text], { type: 'text/csv;charset=utf-8' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 2000);
}

async function editMember(m, depts, timetables) {
  const isNew = !m.id;
  const opt = (list, value, labelKey = 'name') =>
    list.map((x) => `<option value="${x.id}" ${Number(value) === x.id ? 'selected' : ''}>${esc(x[labelKey])}</option>`).join('');

  return modal({
    title: isNew ? 'Add member' : 'Edit member',
    subtitle: 'Fields match what a ZKTeco terminal accepts',
    wide: true,
    body: `
      <div class="g2" style="gap:0 24px">
        <div>
          <h4 style="font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:var(--ink-3);margin-bottom:12px">Identity</h4>
          <div class="grid2">
            <div class="fld"><label class="req">Enrolment No.</label>
              <input class="inp" name="enroll_no" type="number" min="1" max="65535" value="${esc(m.enroll_no ?? '')}">
              <div class="hint">The number typed on the terminal keypad.</div></div>
            <div class="fld"><label>Staff ID</label>
              <input class="inp" name="staff_id" value="${esc(m.staff_id || '')}" placeholder="JWS-041"></div>
          </div>
          <div class="fld"><label class="req">Full name</label>
            <input class="inp" name="full_name" value="${esc(m.full_name || '')}"></div>
          <div class="fld"><label>Name shown on the terminal</label>
            <input class="inp" name="device_name" maxlength="24" value="${esc(m.device_name || '')}"
                   placeholder="Left blank, the first 24 characters are used">
            <div class="hint">The K40 Pro screen cuts off after 24 characters.</div></div>
          <div class="grid2">
            <div class="fld"><label>Gender</label>
              <select class="inp" name="gender">
                <option value="">—</option>
                ${['Male', 'Female', 'Other'].map((g) => `<option ${m.gender === g ? 'selected' : ''}>${g}</option>`).join('')}
              </select></div>
            ${dateField('dob', 'Date of birth', m.dob || '')}
          </div>
          <div class="grid2">
            <div class="fld"><label>Mobile</label>
              <input class="inp" name="mobile" value="${esc(m.mobile || '')}" placeholder="98########"></div>
            <div class="fld"><label>Email</label>
              <input class="inp" name="email" type="email" value="${esc(m.email || '')}">
              <div class="hint">Absence notices go here.</div></div>
          </div>
        </div>
        <div>
          <h4 style="font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:var(--ink-3);margin-bottom:12px">Employment</h4>
          <div class="grid2">
            <div class="fld"><label>Department</label>
              <select class="inp" name="dept_id"><option value="">Unassigned</option>${opt(depts, m.dept_id)}</select></div>
            <div class="fld"><label>Designation</label>
              <input class="inp" name="designation" value="${esc(m.designation || '')}" placeholder="Sr. Teacher"></div>
          </div>
          <div class="grid2">
            ${dateField('joined_on', 'Date joined', m.joined_on || '')}
            <div class="fld"><label>Status</label>
              <select class="inp" name="status">
                ${['Active', 'Inactive', 'On Leave'].map((s) => `<option ${m.status === s ? 'selected' : ''}>${s}</option>`).join('')}
              </select></div>
          </div>
          <div class="fld"><label>Shift</label>
            <select class="inp" name="shift_id">
              <option value="">Use the department default</option>
              ${opt(timetables, m.shift_id)}
            </select>
            <div class="hint">Changing this closes the current assignment today and opens a
              new one, so last month still recomputes against the shift they were actually on.
            </div></div>

          <div class="divider"></div>
          <h4 style="font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:var(--ink-3);margin-bottom:12px">Terminal credentials</h4>
          <div class="grid2">
            <div class="fld"><label>Privilege</label>
              <select class="inp" name="privilege">
                ${PRIVILEGES.map(([v, l]) => `<option value="${v}" ${Number(m.privilege) === v ? 'selected' : ''}>${l}</option>`).join('')}
              </select></div>
            <div class="fld"><label>Card number</label>
              <input class="inp mono" name="card_no" value="${esc(m.card_no || '')}"></div>
          </div>
          <div class="fld"><label>Terminal password</label>
            <input class="inp" name="device_password" maxlength="8" value="${esc(m.device_password || '')}"
                   placeholder="Optional, up to 8 digits"></div>
          <div class="note b">${icon('info')}<div>
            Fingerprints are enrolled on the terminal itself. Saving here sends the
            record to every registered terminal automatically.
          </div></div>
        </div>
      </div>`,
    buttons: [
      { label: 'Cancel', value: null },
      {
        label: isNew ? 'Add member' : 'Save changes', kind: 'pri',
        onClick: async (ov) => {
          const f = readForm(ov);
          const payload = {
            id: m.id ?? null,
            enroll_no: Number(f.enroll_no),
            staff_id: f.staff_id || null,
            full_name: (f.full_name || '').trim(),
            device_name: f.device_name || null,
            dept_id: f.dept_id ? Number(f.dept_id) : null,
            designation: f.designation || null,
            gender: f.gender || null,
            dob: f.dob || null,
            mobile: f.mobile || null,
            email: f.email || null,
            card_no: f.card_no || null,
            privilege: Number(f.privilege) || 0,
            device_password: f.device_password || null,
            status: f.status || 'Active',
            joined_on: f.joined_on || null,
            shift_id: f.shift_id ? Number(f.shift_id) : null,
            access_group: Number(m.access_group) || 1,
          };
          await api.saveMember(payload);
          toast('ok', isNew ? 'Member added and queued for the terminal' : 'Member saved');
          return true;
        },
      },
    ],
  });
}
