import { html, reactive } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';

interface Application {
  app_id: string;
  label: string;
  command: string;
  icon: string;
}

interface RestoredState { apps: Application[] }

const state = reactive({
  section: 'applications' as 'applications',
  apps: [] as Application[],
  editing: null as string | null,
  adding: false,
  form: { app_id: '', label: '', command: '', icon: '' },
  error: '',
});

function startAdd() {
  state.adding = true;
  state.editing = null;
  state.form = { app_id: '', label: '', command: '', icon: '' };
  state.error = '';
}

function startEdit(app: Application) {
  state.editing = app.app_id;
  state.adding = false;
  state.form = { ...app };
  state.error = '';
}

function cancel() {
  state.editing = null;
  state.adding = false;
  state.error = '';
}

function validate(): string {
  const f = state.form;
  if (!f.app_id.trim()) return 'app_id is required';
  if (!f.label.trim()) return 'label is required';
  if (!f.command.trim()) return 'command is required';
  return '';
}

async function submitAdd() {
  const err = validate();
  if (err) { state.error = err; return; }
  try {
    const next = await invoke('applications_add', {
      app_id: state.form.app_id.trim(),
      label: state.form.label.trim(),
      command: state.form.command.trim(),
      icon: state.form.icon.trim(),
    }) as Application[];
    state.apps = next;
    state.adding = false;
    state.error = '';
  } catch (e) {
    state.error = String(e);
  }
}

async function submitUpdate(oldAppId: string) {
  const err = validate();
  if (err) { state.error = err; return; }
  try {
    const next = await invoke('applications_update', {
      old_app_id: oldAppId,
      app_id: state.form.app_id.trim(),
      label: state.form.label.trim(),
      command: state.form.command.trim(),
      icon: state.form.icon.trim(),
    }) as Application[];
    state.apps = next;
    state.editing = null;
    state.error = '';
  } catch (e) {
    state.error = String(e);
  }
}

async function removeApp(app_id: string) {
  try {
    const next = await invoke('applications_remove', { app_id }) as Application[];
    state.apps = next;
  } catch (e) {
    state.error = String(e);
  }
}

function renderForm(onSave: () => void) {
  return html`
    <div class="form">
      <input class="field" placeholder="app_id (e.g. firefox)"
        @input="${(e: Event) => state.form.app_id = (e.target as HTMLInputElement).value}"
        value="${() => state.form.app_id}" />
      <input class="field" placeholder="label (e.g. Firefox)"
        @input="${(e: Event) => state.form.label = (e.target as HTMLInputElement).value}"
        value="${() => state.form.label}" />
      <input class="field" placeholder="command (e.g. firefox)"
        @input="${(e: Event) => state.form.command = (e.target as HTMLInputElement).value}"
        value="${() => state.form.command}" />
      <input class="field" placeholder="icon (e.g. simpleicons/firefox)"
        @input="${(e: Event) => state.form.icon = (e.target as HTMLInputElement).value}"
        value="${() => state.form.icon}" />
      ${() => state.error ? html`<span class="error">${() => state.error}</span>` : html``}
      <div class="form-actions">
        <button class="btn primary" @click="${onSave}">Save</button>
        <button class="btn" @click="${cancel}">Cancel</button>
      </div>
    </div>
  `;
}

function renderRow(app: Application) {
  return html`
    <div class="row">
      ${() => state.editing === app.app_id
        ? renderForm(() => submitUpdate(app.app_id))
        : html`
          <div class="row-info">
            <span class="row-label">${() => app.label}</span>
            <span class="row-detail">${() => app.app_id} · ${() => app.command}</span>
          </div>
          <div class="row-actions">
            <button class="btn-text" @click="${() => startEdit(app)}">Edit</button>
            <button class="btn-text danger" @click="${() => removeApp(app.app_id)}">Remove</button>
          </div>
        `}
    </div>
  `;
}

function renderApplications() {
  return html`
    <div class="section">
      <h2>Applications</h2>
      <p class="section-desc">Entries in <code>~/.config/sola/shell/applications.json</code>. The launcher reloads them each time it opens.</p>
      <div class="list">
        ${() => state.apps.map((app) => renderRow(app))}
      </div>
      ${() => state.adding
        ? renderForm(submitAdd)
        : html`<button class="btn add" @click="${startAdd}">+ Add application</button>`}
    </div>
  `;
}

function renderSidebar() {
  return html`
    <nav class="sidebar">
      <div class="sidebar-title">Settings</div>
      <button class="nav-item active">Applications</button>
    </nav>
  `;
}

export async function createApp(root: HTMLElement): Promise<void> {
  const restored = (window as unknown as { RESTORED_STATE?: RestoredState }).RESTORED_STATE;
  state.apps = restored?.apps ?? [];

  on('state', (payload: unknown) => {
    const p = payload as Partial<RestoredState>;
    if (Array.isArray(p.apps)) state.apps = p.apps;
  });

  html`
    <div class="layout">
      ${renderSidebar()}
      <main class="content">
        ${() => state.section === 'applications' ? renderApplications() : html``}
      </main>
    </div>
  `(root);
}
