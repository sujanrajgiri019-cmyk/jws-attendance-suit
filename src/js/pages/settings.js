import { api, isDesktop } from '../api.js';
import { icon, esc, table, toast, withBusy, modal, readForm } from '../ui.js';
import { forgotPasswordFlow } from '../main.js';

const TABS = [
  ['general', 'School Profile'],
  ['security', 'Security'],
  ['email', 'Email & Alerts'],
  ['updates', 'Updates'],
  ['backup', 'Backup'],
];

export default {
  async mount(host) {
    let tab = 'general';
    const settings = await api.getSettings();
    const info = await api.appInfo().catch(() => ({}));

    host.innerHTML = `
      <div class="tabs" id="tabs">
        ${TABS.map(([k, l]) => `<button data-tab="${k}" class="${k === tab ? 'on' : ''}">${esc(l)}</button>`).join('')}
      </div>
      <div id="body"></div>`;

    const body = host.querySelector('#body');

    host.querySelector('#tabs').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-tab]');
      if (!b) return;
      host.querySelectorAll('#tabs button').forEach((x) => x.classList.remove('on'));
      b.classList.add('on');
      tab = b.dataset.tab;
      render();
    });

    const val = (k, d = '') => esc(settings[k] ?? d);

    function render() {
      if (tab === 'general') general();
      else if (tab === 'security') security();
      else if (tab === 'email') email();
      else if (tab === 'updates') updates();
      else backup();
    }

    // ---------------------------------------------------------------------
    function general() {
      body.innerHTML = `
        <div class="g2">
          <div class="card">
            <div class="card-h"><div class="ht"><h3>School Profile</h3>
              <p>Appears on every printed report</p></div></div>
            <div class="card-b">
              <div style="display:flex;align-items:center;gap:14px;padding:12px;
                background:var(--brand-50);border-radius:var(--r-sm);margin-bottom:16px">
                <img src="assets/logo-full.png" style="height:40px" alt="">
                <div style="flex:1;font-size:12px;color:var(--ink-2)">
                  Your school logo is built into the application and used on reports.</div>
              </div>
              <div class="fld"><label class="req">School name</label>
                <input class="inp" data-set="school_name" value="${val('school_name')}"></div>
              <div class="fld"><label>Address</label>
                <input class="inp" data-set="school_address" value="${val('school_address')}"></div>
              <div class="grid3">
                <div class="fld"><label>Phone 1</label>
                  <input class="inp" data-set="school_phone1" value="${val('school_phone1')}"></div>
                <div class="fld"><label>Phone 2</label>
                  <input class="inp" data-set="school_phone2" value="${val('school_phone2')}"></div>
                <div class="fld"><label>Landline</label>
                  <input class="inp" data-set="school_landline" value="${val('school_landline')}"></div>
              </div>
              <div class="fld"><label>Official email</label>
                <input class="inp" data-set="school_email" value="${val('school_email')}"></div>
              <button class="btn pri" data-save>${icon('save')} Save profile</button>
            </div>
          </div>

          <div class="card">
            <div class="card-h"><div class="ht"><h3>Regional &amp; Display</h3>
              <p>Dates, times and start-up behaviour</p></div></div>
            <div class="card-b">
              <div class="fld"><label>Calendar shown on reports</label>
                <select class="inp" data-set="calendar_mode">
                  ${sel('bs_with_ad', settings.calendar_mode, 'Bikram Sambat with English date')}
                  ${sel('bs', settings.calendar_mode, 'Bikram Sambat only')}
                  ${sel('ad', settings.calendar_mode, 'English (Gregorian) only')}
                </select></div>
              <div class="fld"><label>Time format</label>
                <select class="inp" data-set="time_format">
                  ${sel('24', settings.time_format, '24-hour (16:00)')}
                  ${sel('12', settings.time_format, '12-hour (4:00 PM)')}
                </select></div>
              <div class="fld"><label>Weekly holiday</label>
                <select class="inp" data-set="weekly_holiday">
                  ${sel('6', settings.weekly_holiday, 'Saturday')}
                  ${sel('5,6', settings.weekly_holiday, 'Friday and Saturday')}
                  ${sel('0,6', settings.weekly_holiday, 'Saturday and Sunday')}
                </select>
                <div class="hint">Which days count as a weekly off is set per timetable;
                  this is the default for new ones.</div></div>
              <div class="divider"></div>
              <div class="rowbox"><div class="rt"><b>Start automatically with Windows</b>
                <span>So the listener is running before staff arrive</span></div>
                <label class="tg"><input type="checkbox" data-set-bool="start_with_windows"
                  ${on(settings.start_with_windows) ? 'checked' : ''}><i></i></label></div>
              <div class="rowbox"><div class="rt"><b>Keep running in the system tray</b>
                <span>Closing the window leaves the listener collecting punches</span></div>
                <label class="tg"><input type="checkbox" data-set-bool="minimise_to_tray"
                  ${on(settings.minimise_to_tray) ? 'checked' : ''}><i></i></label></div>
              <button class="btn pri" data-save style="margin-top:6px">${icon('save')} Save</button>
            </div>
          </div>
        </div>`;
      wireSave();
    }

    // ---------------------------------------------------------------------
    function security() {
      body.innerHTML = `
        <div class="g2">
          <div class="card">
            <div class="card-h"><div class="ht"><h3>Administrator Password</h3>
              <p>Used to sign in to this application</p></div></div>
            <div class="card-b">
              ${info.password_is_default ? `<div class="note y" style="margin-bottom:16px">${icon('warn')}<div>
                This computer is still using the default password <b>Attendance@123</b>.
                Anyone who knows it can open the attendance records. Change it now.
              </div></div>` : `<div class="note g" style="margin-bottom:16px">${icon('check')}<div>
                A custom password is set.
              </div></div>`}
              <div class="fld"><label>Username</label>
                <input class="inp" value="${val('admin_username', 'admin')}" disabled></div>
              <div class="fld"><label class="req">Current password</label>
                <input class="inp" type="password" id="pwCur" autocomplete="current-password"></div>
              <div class="fld"><label class="req">New password</label>
                <input class="inp" type="password" id="pwNew" autocomplete="new-password"
                       placeholder="At least 8 characters, with letters and numbers"></div>
              <div class="fld"><label class="req">Confirm new password</label>
                <input class="inp" type="password" id="pwNew2" autocomplete="new-password"></div>
              <button class="btn pri" id="btnPw">${icon('lock')} Change password</button>
            </div>
          </div>

          <div>
            <div class="card">
              <div class="card-h"><div class="ht"><h3>Password Recovery</h3>
                <p>If the password is forgotten</p></div></div>
              <div class="card-b">
                <div class="fld"><label>Recovery email address</label>
                  <input class="inp" data-set="recovery_email" value="${val('recovery_email')}"></div>
                <p style="font-size:12.5px;color:var(--ink-2);line-height:1.6;margin-bottom:14px">
                  A six-digit code is emailed to this address and stops working after 15 minutes.
                  This needs the Gmail app password saved under <b>Email &amp; Alerts</b>.
                </p>
                <div class="btn-grp">
                  <button class="btn" data-save>${icon('save')} Save address</button>
                  <button class="btn" id="btnTestReset">${icon('mail')} Try the recovery flow</button>
                </div>
              </div>
            </div>

            <div class="card mt14">
              <div class="card-h"><div class="ht"><h3>This Installation</h3>
                <p>Where the data lives</p></div></div>
              <div class="card-b">
                <div class="stat-row"><span class="sl">Version</span>
                  <span class="sv">${esc(info.version || '—')}</span></div>
                <div class="stat-row"><span class="sl">Database file</span>
                  <span class="sv mono" style="font-size:11px;max-width:280px;overflow:hidden;
                    text-overflow:ellipsis">${esc(info.db_path || '—')}</span></div>
                <div class="stat-row"><span class="sl">Database size</span>
                  <span class="sv">${esc(info.db_size_kb ?? 0)} KB</span></div>
                <div class="stat-row"><span class="sl">Schema version</span>
                  <span class="sv">${esc(info.schema_version ?? '—')}</span></div>
              </div>
            </div>
          </div>
        </div>`;

      wireSave();

      body.querySelector('#btnPw').addEventListener('click', (e) =>
        withBusy(e.currentTarget, async () => {
          const cur = body.querySelector('#pwCur').value;
          const a = body.querySelector('#pwNew').value;
          const b = body.querySelector('#pwNew2').value;
          if (!cur) throw new Error('Enter the current password.');
          if (a !== b) throw new Error('The two new passwords do not match.');
          await api.changePassword(cur, a);
          toast('ok', 'Password changed');
          body.querySelector('#pwCur').value = '';
          body.querySelector('#pwNew').value = '';
          body.querySelector('#pwNew2').value = '';
          info.password_is_default = false;
          render();
        }).catch(() => {}),
      );

      body.querySelector('#btnTestReset').addEventListener('click', () => forgotPasswordFlow());
    }

    // ---------------------------------------------------------------------
    function email() {
      body.innerHTML = `
        <div class="g2">
          <div class="card">
            <div class="card-h"><div class="ht"><h3>Gmail</h3>
              <p>Used for absence notices and password recovery</p></div></div>
            <div class="card-b">
              <div class="fld"><label>Sender address</label>
                <input class="inp" data-set="smtp_user" value="${val('smtp_user')}"></div>
              <div class="fld"><label>App password</label>
                <input class="inp" type="password" data-set="smtp_pass"
                       placeholder="${settings.smtp_pass ? 'Saved — type to replace' : 'Sixteen characters from Google'}">
                <div class="hint">This is a Google <b>app password</b>, not the account password.
                  Create one under Google Account → Security → App passwords.</div></div>
              <div class="grid2">
                <div class="fld"><label>SMTP server</label>
                  <input class="inp" data-set="smtp_host" value="${val('smtp_host', 'smtp.gmail.com')}"></div>
                <div class="fld"><label>Port</label>
                  <input class="inp" data-set="smtp_port" value="${val('smtp_port', '587')}"></div>
              </div>
              <div class="btn-grp">
                <button class="btn pri" data-save>${icon('save')} Save</button>
                <button class="btn" id="btnTestMail">${icon('mail')} Send a test message</button>
              </div>
            </div>
          </div>

          <div class="card">
            <div class="card-h"><div class="ht"><h3>Automatic Notifications</h3>
              <p>Who gets emailed, and when</p></div></div>
            <div class="card-b">
              <div class="note b" style="margin-bottom:14px">${icon('info')}<div>
                Which notices are sent is controlled on the <b>Attendance Rules</b> screen.
                The times they go out are set here.
              </div></div>
              <div class="grid2">
                <div class="fld"><label>Absence notices sent at</label>
                  <input type="time" class="inp" data-set="email_absentees_at"
                         value="${val('email_absentees_at', '10:00')}"></div>
                <div class="fld"><label>Daily summary sent at</label>
                  <input type="time" class="inp" data-set="daily_summary_at"
                         value="${val('daily_summary_at', '17:00')}"></div>
              </div>
              <button class="btn pri" data-save>${icon('save')} Save times</button>
              <div class="divider"></div>
              <h4 style="font-size:12.5px;margin-bottom:10px">Send now</h4>
              <button class="btn" style="width:100%" id="btnAbsenceNow">
                ${icon('mail')} Email today's absentees
              </button>
            </div>
          </div>
        </div>`;

      wireSave();

      body.querySelector('#btnTestMail').addEventListener('click', (e) =>
        withBusy(e.currentTarget, async () => {
          const to = await api.sendTestMail(null);
          toast('ok', `Test message sent to ${to}`);
        }).catch(() => {}),
      );

      body.querySelector('#btnAbsenceNow').addEventListener('click', (e) =>
        withBusy(e.currentTarget, async () => {
          const r = await api.sendAbsenceEmails(null);
          toast(r.failed ? 'err' : 'ok',
            `${r.sent} sent · ${r.skipped_no_email} without an address · ${r.failed} failed`);
        }).catch(() => {}),
      );
    }

    // ---------------------------------------------------------------------
    function updates() {
      body.innerHTML = `
        <div class="g-2-1">
          <div class="card">
            <div class="card-h"><div class="ht"><h3>Software Updates</h3>
              <p>Delivered through GitHub Releases</p></div></div>
            <div class="card-b">
              <div style="display:flex;align-items:center;gap:14px;padding:15px;
                background:var(--brand-50);border:1px solid var(--brand-100);
                border-radius:var(--r);margin-bottom:16px">
                <div class="ico">${icon('down')}</div>
                <div style="flex:1">
                  <div style="font-size:14px;font-weight:680;color:var(--brand-700)">
                    You are running version ${esc(info.version || '—')}</div>
                  <div style="font-size:12px;color:var(--ink-2);margin-top:2px" id="updState">
                    Press check to look for a newer release.</div>
                </div>
                <button class="btn pri" id="btnCheck">${icon('sync')} Check now</button>
              </div>
              <div class="note b">${icon('info')}<div>
                Updates are signed. A package that is not signed with the school's key is refused,
                so a tampered download cannot install itself. The database is backed up before
                any update is applied.
              </div></div>
            </div>
          </div>

          <div class="card" style="align-self:start">
            <div class="card-h"><div class="ht"><h3>Update Source</h3>
              <p>Where releases come from</p></div></div>
            <div class="card-b">
              <div class="fld"><label>GitHub repository</label>
                <input class="inp mono" data-set="update_repo" value="${val('update_repo')}"></div>
              <div class="fld"><label>Release channel</label>
                <select class="inp" data-set="update_channel">
                  ${sel('stable', settings.update_channel, 'Stable')}
                  ${sel('beta', settings.update_channel, 'Beta')}
                </select></div>
              <div class="fld"><label>Check for updates</label>
                <select class="inp" data-set="update_check">
                  ${sel('daily', settings.update_check, 'Daily')}
                  ${sel('weekly', settings.update_check, 'Weekly')}
                  ${sel('startup', settings.update_check, 'When the app starts')}
                  ${sel('manual', settings.update_check, 'Only when I ask')}
                </select></div>
              <button class="btn pri" style="width:100%" data-save>${icon('save')} Save</button>
            </div>
          </div>
        </div>`;

      wireSave();

      body.querySelector('#btnCheck').addEventListener('click', (e) =>
        withBusy(e.currentTarget, async () => {
          if (!isDesktop()) throw new Error('Updates are only available in the installed application.');
          const updater = window.__TAURI__?.updater;
          if (!updater?.check) {
            throw new Error('The updater is not available in this build.');
          }
          const update = await updater.check();
          const state = body.querySelector('#updState');
          if (!update) {
            state.textContent = 'This is the latest version.';
            toast('ok', 'You are up to date');
            return;
          }
          state.textContent = `Version ${update.version} is available.`;
          const go = await modal({
            title: `Update to version ${update.version}?`,
            subtitle: 'The application will restart',
            body: `<p style="font-size:13px;line-height:1.6;margin:0 0 12px">
                ${esc(update.body || 'No release notes were published for this version.')}</p>
              <div class="note b">${icon('info')}<div>
                The database is backed up first. Attendance records are not affected.
              </div></div>`,
            buttons: [
              { label: 'Not now', value: false },
              { label: 'Install and restart', kind: 'pri', value: true },
            ],
          });
          if (!go) return;
          await api.backupNow().catch(() => {});
          await update.downloadAndInstall();
          await window.__TAURI__?.process?.relaunch?.();
        }).catch(() => {}),
      );
    }

    // ---------------------------------------------------------------------
    async function backup() {
      body.innerHTML = `
        <div class="g2">
          <div class="card">
            <div class="card-h"><div class="ht"><h3>Database Backup</h3>
              <p>${esc(info.db_size_kb ?? 0)} KB · ${esc(info.db_path || '')}</p></div></div>
            <div class="card-b">
              <div class="fld"><label>Backup folder</label>
                <input class="inp" data-set="backup_dir" value="${val('backup_dir')}"
                       placeholder="Left blank, backups go beside the database">
              </div>
              <div class="fld"><label>Automatic backup</label>
                <select class="inp" data-set="backup_schedule">
                  ${sel('daily_18', settings.backup_schedule, 'Every day at 18:00')}
                  ${sel('every_6h', settings.backup_schedule, 'Every six hours')}
                  ${sel('weekly', settings.backup_schedule, 'Weekly')}
                  ${sel('off', settings.backup_schedule, 'Off')}
                </select></div>
              <div class="note b" style="margin-bottom:14px">${icon('info')}<div>
                A backup is a complete copy of the attendance database, taken safely even while
                punches are arriving. Keep one copy off this computer.
              </div></div>
              <div class="btn-grp">
                <button class="btn pri" id="btnBackup">${icon('save')} Back up now</button>
                <button class="btn" data-save>Save settings</button>
              </div>
            </div>
          </div>

          <div class="card">
            <div class="card-h"><div class="ht"><h3>Existing Backups</h3>
              <p>Newest first</p></div></div>
            <div class="tbl-wrap"><table class="tbl" id="bk"></table></div>
          </div>
        </div>`;

      wireSave();

      async function loadBackups() {
        const rows = await api.listBackups().catch(() => []);
        table(body.querySelector('#bk'), [
          { label: 'File', cls: 'mono', get: (b) => esc(b.file) },
          { label: 'Created', get: (b) => esc(b.modified) },
          { label: 'Size', cls: 'num', get: (b) => `${b.size_kb} KB` },
          {
            label: '', cls: 'ctr',
            get: (b) => `<button class="btn sm" data-open="${esc(b.path)}">Show</button>`,
          },
        ], rows, {
          empty: 'No backups yet',
          emptyHint: 'Press "Back up now" to create the first one.',
        });
      }

      body.querySelector('#btnBackup').addEventListener('click', (e) =>
        withBusy(e.currentTarget, async () => {
          const path = await api.backupNow();
          toast('ok', `Saved to ${path}`);
          await loadBackups();
        }).catch(() => {}),
      );

      body.querySelector('#bk').addEventListener('click', (e) => {
        const b = e.target.closest('[data-open]');
        if (b) api.openPath(b.dataset.open).catch((err) => toast('err', err.message));
      });

      await loadBackups();
    }

    // ---------------------------------------------------------------------
    function wireSave() {
      body.querySelectorAll('[data-save]').forEach((btn) => {
        btn.addEventListener('click', (e) =>
          withBusy(e.currentTarget, async () => {
            const payload = {};
            body.querySelectorAll('[data-set]').forEach((el) => {
              // An untouched password field must not clear the stored value.
              if (el.type === 'password' && !el.value) return;
              payload[el.dataset.set] = el.value;
            });
            body.querySelectorAll('[data-set-bool]').forEach((el) => {
              payload[el.dataset.setBool] = el.checked ? '1' : '0';
            });
            await api.setSettings(payload);
            Object.assign(settings, payload);
            toast('ok', 'Settings saved');
          }).catch(() => {}),
        );
      });
    }

    render();
  },
};

const sel = (value, current, label) =>
  `<option value="${esc(value)}" ${String(current) === value ? 'selected' : ''}>${esc(label)}</option>`;

const on = (v) => ['1', 'true', 'yes'].includes(String(v ?? '').toLowerCase());
