import { api } from '../api.js';
import { icon, esc, table, modal, toast, withBusy, readForm } from '../ui.js';
import { refreshDeviceChip } from '../main.js';

const MODELS = [
  'ZKTeco K40 Pro', 'ZKTeco K40', 'ZKTeco K50', 'ZKTeco F18',
  'ZKTeco MB360', 'ZKTeco iClock', 'Other ZKTeco (ADMS)',
];

export default {
  async mount(host) {
    host.innerHTML = `
      <div class="tbar">
        <div class="sp"></div>
        <button class="btn" id="btnTest">${icon('wifi')} Test all</button>
        <button class="btn" id="btnListener">${icon('plug')} Listener</button>
        <button class="btn pri" id="btnAdd">${icon('plus')} Add device</button>
      </div>

      <div class="g3" id="cards"></div>

      <div class="card mt14">
        <div class="card-h"><div class="ht"><h3>Machine List</h3>
          <p>Every terminal registered on this computer</p></div></div>
        <div class="tbl-wrap"><table class="tbl" id="tbl"></table></div>
      </div>

      <div class="g2 mt14">
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Connection Mode</h3>
            <p>How this computer and the terminal talk</p></div></div>
          <div class="card-b" id="mode"></div>
        </div>
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Recent Transfers</h3>
            <p>Last sync jobs</p></div></div>
          <div class="tbl-wrap"><table class="tbl" id="sync"></table></div>
        </div>
      </div>`;

    async function load() {
      const [devices, info, addresses, history] = await Promise.all([
        api.listDevices(), api.appInfo(), api.localAddresses().catch(() => []), api.syncHistory(8),
      ]);

      // --- cards ---
      host.querySelector('#cards').innerHTML = devices.length
        ? devices.map((d) => {
          const online = !!d.last_seen;
          return `<div class="card">
            <div class="card-h">
              <div class="ico ${online ? '' : 'b'}" style="${online ? '' : 'background:var(--line-2)'}">${icon('cpu')}</div>
              <div class="ht"><h3>${esc(d.name)}</h3><p>${esc(d.model)} · No. ${d.machine_no}</p></div>
              <span class="tag ${online ? 'g' : 'n'}"><span class="d"></span>${online ? 'Seen' : 'Not seen'}</span>
            </div>
            <div class="card-b" style="padding-top:12px;padding-bottom:12px">
              <div class="stat-row"><span class="sl">Address</span>
                <span class="sv mono">${esc(d.ip)}:${d.port}</span></div>
              <div class="stat-row"><span class="sl">Serial</span>
                <span class="sv mono">${esc(d.serial || '—')}</span></div>
              <div class="stat-row"><span class="sl">Mode</span>
                <span class="sv">${d.mode === 'push' ? 'Push (ADMS)' : 'Pull (TCP)'}</span></div>
              <div class="stat-row"><span class="sl">Last contact</span>
                <span class="sv">${esc(d.last_seen || 'never')}</span></div>
              <div class="stat-row"><span class="sl">Location</span>
                <span class="sv" style="font-size:12px">${esc(d.location || '—')}</span></div>
            </div>
            <div class="card-f" style="display:flex;gap:7px">
              <button class="btn sm" style="flex:1" data-ping="${d.id}">Test</button>
              <button class="btn sm" style="flex:1" data-read="${d.id}">Read info</button>
              <button class="btn sm ic" data-edit="${d.id}">${icon('edit')}</button>
            </div>
          </div>`;
        }).join('')
        : `<div class="card" style="grid-column:1/-1"><div class="empty">
            <div class="ei">${icon('cpu')}</div>
            <b>No terminal registered yet</b>
            <p>Add the K40 Pro at the main gate to start collecting attendance.</p>
            <button class="btn pri" style="margin-top:14px" id="emptyAdd">${icon('plus')} Add device</button>
          </div></div>`;

      // --- machine list ---
      table(host.querySelector('#tbl'), [
        { label: 'Device', get: (d) => `<b>${esc(d.name)}</b>` },
        {
          label: 'Status',
          get: (d) => `<span class="tag ${d.last_seen ? 'g' : 'n'}"><span class="d"></span>${d.last_seen ? 'Reporting' : 'Silent'}</span>`,
        },
        { label: 'Machine No.', cls: 'mono', get: (d) => d.machine_no },
        { label: 'Model', get: (d) => esc(d.model) },
        { label: 'IP address', cls: 'mono', get: (d) => `${esc(d.ip)}:${d.port}` },
        { label: 'Serial', cls: 'mono', get: (d) => esc(d.serial || '—') },
        { label: 'MAC', cls: 'mono', get: (d) => esc(d.mac || '—') },
        { label: 'Location', get: (d) => esc(d.location || '—') },
      ], devices, { empty: 'No terminals', emptyHint: 'Add one to get started.' });

      // --- mode panel ---
      host.querySelector('#mode').innerHTML = `
        <div class="rowbox">
          <div class="rt">
            <b>Push / ADMS</b>
            <span>The terminal sends each scan here the moment it happens — no polling delay.
              Point its <b>Server Address</b> at this computer, port ${info.push_port}.</span>
          </div>
          <span class="tag ${info.push_running ? 'g' : 'n'}" style="flex:none">
            <span class="d"></span>${info.push_running ? `port ${info.push_port}` : 'stopped'}
          </span>
          <button class="btn sm ${info.push_running ? '' : 'pri'}" id="togglePush" style="flex:none">
            ${info.push_running ? 'Stop' : 'Start'}
          </button>
        </div>
        ${addresses.length ? `<div class="note g" style="margin-top:10px">${icon('info')}<div>
            On the terminal, set <b>Comm → Cloud Server → Server Address</b> to
            <b class="mono">${esc(addresses[0])}</b> and <b>Server Port</b> to
            <b class="mono">${info.push_port}</b>, then turn <b>HTTPS off</b>.
          </div></div>`
        : `<div class="note y" style="margin-top:10px">${icon('warn')}<div>
            This computer's LAN address could not be detected. Find it with
            <b class="mono">ipconfig</b> and enter that as the terminal's Server Address.
          </div></div>`}
        <div class="rowbox" style="margin-top:10px">
          <div class="rt"><b>Pull / Standalone</b>
            <span>This computer asks the terminal for its stored records over TCP
              ${devices[0]?.port || 4370}. Used as a fallback in Data Transfer.</span></div>
          <span class="tag n">Available</span>
        </div>`;

      // --- sync history ---
      table(host.querySelector('#sync'), [
        { label: 'When', get: (s) => `<span style="color:var(--ink-3)">${esc(s.ts)}</span>` },
        { label: 'Job', get: (s) => `<b>${esc(s.job)}</b>` },
        { label: 'Device', get: (s) => esc(s.device || '—') },
        { label: 'Result', get: (s) => esc(s.result || '') },
        { label: 'Status', get: (s) => `<span class="tag ${s.ok ? 'g' : 'r'}">${s.ok ? 'Success' : 'Failed'}</span>` },
      ], history, { empty: 'No transfers yet', emptyHint: 'Sync jobs appear here once they run.' });

      wire(devices, info);
    }

    function wire(devices, info) {
      host.querySelector('#emptyAdd')?.addEventListener('click', () => addDevice(load));
      host.querySelector('#btnAdd').onclick = () => addDevice(load);

      host.querySelector('#togglePush')?.addEventListener('click', (e) =>
        withBusy(e.currentTarget, async () => {
          if (info.push_running) {
            await api.pushStop();
            toast('inf', 'Listener stopped');
          } else {
            const p = await api.pushStart();
            toast('ok', `Listening on port ${p}`);
          }
          await refreshDeviceChip();
          await load();
        }).catch(() => {}),
      );

      host.querySelector('#btnTest').onclick = (e) =>
        withBusy(e.currentTarget, async () => {
          if (!devices.length) return toast('inf', 'No terminals to test');
          for (const d of devices) {
            try {
              const ms = await api.devicePing(d.ip, d.port);
              toast('ok', `${d.name} answered in ${ms} ms`);
            } catch (err) {
              toast('err', `${d.name}: ${err.message}`);
            }
          }
        }).catch(() => {});

      host.querySelector('#btnListener').onclick = () => showListenerHelp(info);

      host.querySelector('#cards').addEventListener('click', async (e) => {
        const ping = e.target.closest('[data-ping]');
        if (ping) {
          const d = devices.find((x) => x.id === Number(ping.dataset.ping));
          return withBusy(ping, async () => {
            const ms = await api.devicePing(d.ip, d.port);
            toast('ok', `${d.name} answered in ${ms} ms`);
          }).catch(() => {});
        }
        const read = e.target.closest('[data-read]');
        if (read) {
          const d = devices.find((x) => x.id === Number(read.dataset.read));
          return withBusy(read, async () => {
            const i = await api.deviceInfo(d.ip, d.port, d.comm_key);
            await modal({
              title: `${d.name} — device information`,
              body: [
                ['Serial number', i.serial], ['Device name', i.name],
                ['Platform', i.platform], ['MAC address', i.mac],
                ['Users enrolled', i.user_count], ['Fingerprints', i.fp_count],
                ['Records stored', i.log_count],
              ].map(([k, v]) =>
                `<div class="stat-row"><span class="sl">${esc(k)}</span>
                 <span class="sv mono">${esc(v || '—')}</span></div>`).join(''),
              buttons: [{ label: 'Close', value: null }],
            });
          }).catch(() => {});
        }
        const edit = e.target.closest('[data-edit]');
        if (edit) {
          const d = devices.find((x) => x.id === Number(edit.dataset.edit));
          if (await deviceDialog(d)) await load();
        }
      });
    }

    async function addDevice(reload) {
      if (await deviceDialog({ machine_no: 101, port: 4370, comm_key: 0, model: MODELS[0] })) {
        await reload();
      }
    }

    await load();
  },
};

function showListenerHelp(info) {
  return modal({
    title: 'Setting up the terminal',
    subtitle: 'One-time configuration on the K40 Pro keypad',
    body: `<ol style="font-size:13.5px;line-height:1.85;padding-left:20px;margin:0">
        <li>On the terminal press <b>M/OK</b> → <b>Comm.</b> → <b>Ethernet</b>.</li>
        <li>Confirm the terminal has a fixed IP and DHCP is off.</li>
        <li>Go back and open <b>Comm.</b> → <b>Cloud Server Setting</b>.</li>
        <li>Set <b>Server Mode</b> to <b>ADMS</b>.</li>
        <li>Set <b>Server Address</b> to this computer's LAN address.</li>
        <li>Set <b>Server Port</b> to <b>${info.push_port}</b>.</li>
        <li>Turn <b>HTTPS</b> <b>off</b> — this listener speaks plain HTTP on the school network.</li>
        <li>Save and let the terminal restart. It will connect within a minute.</li>
      </ol>
      <div class="note y" style="margin-top:14px">${icon('warn')}<div>
        The first time Windows asks, allow JWS Attendance through the firewall on
        <b>private networks</b>, or the terminal will not be able to reach it.
      </div></div>`,
    buttons: [{ label: 'Got it', kind: 'pri', value: true }],
  });
}

function deviceDialog(d) {
  const isNew = !d.id;
  return modal({
    title: isNew ? 'Add device' : `Edit ${d.name}`,
    subtitle: 'A fingerprint terminal on the school network',
    body: `
      <div class="grid2">
        <div class="fld"><label class="req">Device name</label>
          <input class="inp" name="name" value="${esc(d.name || '')}" placeholder="Main Gate"></div>
        <div class="fld"><label class="req">Machine No.</label>
          <input class="inp" name="machine_no" type="number" value="${esc(d.machine_no ?? 101)}"></div>
      </div>
      <div class="fld"><label>Model</label>
        <select class="inp" name="model">
          ${MODELS.map((m) => `<option ${d.model === m ? 'selected' : ''}>${esc(m)}</option>`).join('')}
        </select></div>
      <div class="grid2">
        <div class="fld"><label class="req">IP address</label>
          <input class="inp mono" name="ip" value="${esc(d.ip || '')}" placeholder="192.168.100.99"></div>
        <div class="fld"><label class="req">Port</label>
          <input class="inp mono" name="port" type="number" value="${esc(d.port ?? 4370)}"></div>
      </div>
      <div class="grid2">
        <div class="fld"><label>Comm key</label>
          <input class="inp" name="comm_key" type="number" value="${esc(d.comm_key ?? 0)}">
          <div class="hint">0 unless a key was set on the terminal.</div></div>
        <div class="fld"><label>Location</label>
          <input class="inp" name="location" value="${esc(d.location || '')}" placeholder="Main Building"></div>
      </div>`,
    buttons: [
      { label: 'Cancel', value: null },
      {
        label: 'Test connection',
        onClick: async (ov) => {
          const f = readForm(ov);
          const ms = await api.devicePing(f.ip, Number(f.port));
          toast('ok', `Answered in ${ms} ms`);
          return false; // keep the dialog open
        },
      },
      {
        label: isNew ? 'Add device' : 'Save', kind: 'pri',
        onClick: async (ov) => {
          const f = readForm(ov);
          await api.saveDevice({
            id: d.id ?? null,
            name: (f.name || '').trim(),
            machine_no: Number(f.machine_no) || 1,
            model: f.model,
            ip: (f.ip || '').trim(),
            port: Number(f.port) || 4370,
            comm_key: Number(f.comm_key) || 0,
            location: f.location || null,
          });
          toast('ok', isNew ? 'Device added' : 'Device saved');
          return true;
        },
      },
    ],
  });
}
