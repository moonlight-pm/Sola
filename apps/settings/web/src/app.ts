import { html, reactive } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';

interface Application {
  app_id: string;
  label: string;
  command: string;
  icon: string;
}

interface Candidate {
  app_id: string;
  title: string;
  suggested_command: string | null;
}

interface ApplicationsState {
  apps: Application[];
  missing: string[];
  candidates: Candidate[];
}

interface MailCondition {
  field: string;
  match: string;
  value: string;
}

interface MailRule {
  name: string;
  action: string;
  dest: string | null;
  conditions: MailCondition[];
}

interface MailConfig {
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  username: string;
  password: string;
  rules: MailRule[];
}

interface StatePayload {
  applications: ApplicationsState;
  mail: MailConfig;
}

type Section = 'applications' | 'mail';

function emptyMail(): MailConfig {
  return {
    email: '',
    imap_host: '',
    imap_port: 993,
    smtp_host: '',
    smtp_port: 587,
    username: '',
    password: '',
    rules: [],
  };
}

function emptyRule(): MailRule {
  return { name: '', action: 'smart_mailbox', dest: '', conditions: [] };
}

const state = reactive({
  section: 'applications' as Section,
  // Applications
  apps: [] as Application[],
  missing: [] as string[],
  candidates: [] as Candidate[],
  editing: null as string | null,
  adding: false,
  form: { app_id: '', label: '', command: '', icon: '' },
  error: '',
  // Mail
  mail: emptyMail(),
  mailDraft: emptyMail(),
  mailError: '',
  ruleForm: emptyRule(),
  addingRule: false,
  ruleError: '',
});

function applyState(p: StatePayload) {
  state.apps = p.applications?.apps ?? [];
  state.missing = p.applications?.missing ?? [];
  state.candidates = p.applications?.candidates ?? [];
  state.mail = p.mail ?? emptyMail();
  state.mailDraft = { ...state.mail, rules: state.mail.rules };
}

// --- Applications section ---------------------------------------------------

function startAdd(prefill?: Partial<Application>) {
  state.adding = true;
  state.editing = null;
  state.form = {
    app_id: prefill?.app_id ?? '',
    label: prefill?.label ?? prefill?.app_id ?? '',
    command: prefill?.command ?? '',
    icon: prefill?.icon ?? '',
  };
  state.error = '';
}

function startEdit(app: Application) {
  state.editing = app.app_id;
  state.adding = false;
  state.form = { ...app };
  state.error = '';
}

function cancelAppForm() {
  state.editing = null;
  state.adding = false;
  state.error = '';
}

function validateApp(): string {
  const f = state.form;
  if (!f.app_id.trim()) return 'app_id is required';
  if (!f.label.trim()) return 'label is required';
  if (!f.command.trim()) return 'command is required';
  return '';
}

async function submitAdd() {
  const err = validateApp();
  if (err) { state.error = err; return; }
  try {
    const next = await invoke('applications_add', {
      app_id: state.form.app_id.trim(),
      label: state.form.label.trim(),
      command: state.form.command.trim(),
      icon: state.form.icon.trim(),
    }) as StatePayload;
    applyState(next);
    state.adding = false;
    state.error = '';
  } catch (e) {
    state.error = String(e);
  }
}

async function submitUpdate(oldAppId: string) {
  const err = validateApp();
  if (err) { state.error = err; return; }
  try {
    const next = await invoke('applications_update', {
      old_app_id: oldAppId,
      app_id: state.form.app_id.trim(),
      label: state.form.label.trim(),
      command: state.form.command.trim(),
      icon: state.form.icon.trim(),
    }) as StatePayload;
    applyState(next);
    state.editing = null;
    state.error = '';
  } catch (e) {
    state.error = String(e);
  }
}

async function removeApp(app_id: string) {
  try {
    const next = await invoke('applications_remove', { app_id }) as StatePayload;
    applyState(next);
  } catch (e) {
    state.error = String(e);
  }
}

function renderAppForm(onSave: () => void) {
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
        <button class="btn" @click="${cancelAppForm}">Cancel</button>
      </div>
    </div>
  `;
}

function renderAppRow(app: Application) {
  return html`
    <div class="row">
      ${() => state.editing === app.app_id
        ? renderAppForm(() => submitUpdate(app.app_id))
        : html`
          <div class="row-info">
            <span class="row-label">
              ${() => app.label}
              ${() => state.missing.includes(app.app_id)
                ? html`<span class="badge missing" title="Command not found on PATH">not found</span>`
                : html``}
            </span>
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

function renderCandidate(c: Candidate) {
  return html`
    <div class="row">
      <div class="row-info">
        <span class="row-label">${() => c.app_id}</span>
        <span class="row-detail">
          ${() => c.title || '(no title)'}
          ${() => c.suggested_command
            ? html` · ${() => c.suggested_command}`
            : html` · <span class="text-muted">command unknown — add manually</span>`}
        </span>
      </div>
      <div class="row-actions">
        <button class="btn-text" @click="${() => startAdd({
          app_id: c.app_id,
          label: c.app_id,
          command: c.suggested_command ?? '',
          icon: '',
        })}">+ Add</button>
      </div>
    </div>
  `;
}

function renderCandidates() {
  return html`
    ${() => state.candidates.length > 0
      ? html`
        <div class="section-subhead">Running, not configured</div>
        <div class="list">
          ${() => state.candidates.map(renderCandidate)}
        </div>
      `
      : html``}
  `;
}

function renderApplications() {
  return html`
    <div class="section">
      <h2>Applications</h2>
      <p class="section-desc">Entries in <code>~/.config/sola/shell/applications.json</code>. The launcher reloads them each time it opens.</p>
      ${renderCandidates()}
      <div class="section-subhead">Configured</div>
      <div class="list">
        ${() => state.apps.map((app) => renderAppRow(app))}
      </div>
      ${() => state.adding
        ? renderAppForm(submitAdd)
        : html`<button class="btn add" @click="${() => startAdd()}">+ Add application</button>`}
    </div>
  `;
}

// --- Mail section -----------------------------------------------------------

function numericInput(e: Event, fallback: number): number {
  const n = Number((e.target as HTMLInputElement).value);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

async function saveMailAccount() {
  try {
    const next = await invoke('mail_save_account', {
      email: state.mailDraft.email.trim(),
      imap_host: state.mailDraft.imap_host.trim(),
      imap_port: state.mailDraft.imap_port || 993,
      smtp_host: state.mailDraft.smtp_host.trim(),
      smtp_port: state.mailDraft.smtp_port || 587,
      username: state.mailDraft.username.trim(),
      password: state.mailDraft.password,
    }) as StatePayload;
    applyState(next);
    state.mailError = '';
  } catch (e) {
    state.mailError = String(e);
  }
}

function resetMailDraft() {
  state.mailDraft = { ...state.mail, rules: state.mail.rules };
  state.mailError = '';
}

function startAddRule() {
  state.addingRule = true;
  state.ruleForm = emptyRule();
  state.ruleError = '';
}

function cancelRule() {
  state.addingRule = false;
  state.ruleError = '';
}

function addCondition() {
  state.ruleForm.conditions = [
    ...state.ruleForm.conditions,
    { field: 'from', match: 'contains', value: '' },
  ];
}

function updateCondition(i: number, patch: Partial<MailCondition>) {
  state.ruleForm.conditions = state.ruleForm.conditions.map(
    (c, idx) => idx === i ? { ...c, ...patch } : c,
  );
}

function removeCondition(i: number) {
  state.ruleForm.conditions = state.ruleForm.conditions.filter((_, idx) => idx !== i);
}

async function submitRule() {
  if (!state.ruleForm.name.trim()) { state.ruleError = 'name is required'; return; }
  if (state.ruleForm.conditions.length === 0) { state.ruleError = 'at least one condition'; return; }
  try {
    const next = await invoke('mail_add_rule', {
      name: state.ruleForm.name.trim(),
      action: state.ruleForm.action,
      dest: state.ruleForm.action === 'move' ? (state.ruleForm.dest ?? '').trim() : null,
      conditions: state.ruleForm.conditions.map(c => ({
        field: c.field,
        match: c.match,
        value: c.value.trim(),
      })),
    }) as StatePayload;
    applyState(next);
    state.addingRule = false;
    state.ruleError = '';
  } catch (e) {
    state.ruleError = String(e);
  }
}

async function removeRule(index: number) {
  try {
    const next = await invoke('mail_remove_rule', { index }) as StatePayload;
    applyState(next);
  } catch (e) {
    state.mailError = String(e);
  }
}

function conditionSummary(rule: MailRule): string {
  return rule.conditions
    .map(c => `${c.field} ${c.match} "${c.value}"`)
    .join(' AND ');
}

function ruleTarget(rule: MailRule): string {
  if (rule.action === 'smart_mailbox') return 'smart mailbox';
  if (rule.action === 'move' && rule.dest) return `move → ${rule.dest}`;
  return rule.action;
}

function renderMailAccount() {
  return html`
    <div class="form">
      <div class="field-row">
        <label class="field-label">Email</label>
        <input class="field" type="email"
          @input="${(e: Event) => state.mailDraft.email = (e.target as HTMLInputElement).value}"
          value="${() => state.mailDraft.email}" />
      </div>
      <div class="field-row">
        <label class="field-label">IMAP host</label>
        <input class="field"
          @input="${(e: Event) => state.mailDraft.imap_host = (e.target as HTMLInputElement).value}"
          value="${() => state.mailDraft.imap_host}" />
      </div>
      <div class="field-row">
        <label class="field-label">IMAP port</label>
        <input class="field field-narrow" type="number"
          @input="${(e: Event) => state.mailDraft.imap_port = numericInput(e, 993)}"
          value="${() => String(state.mailDraft.imap_port)}" />
      </div>
      <div class="field-row">
        <label class="field-label">SMTP host</label>
        <input class="field"
          @input="${(e: Event) => state.mailDraft.smtp_host = (e.target as HTMLInputElement).value}"
          value="${() => state.mailDraft.smtp_host}" />
      </div>
      <div class="field-row">
        <label class="field-label">SMTP port</label>
        <input class="field field-narrow" type="number"
          @input="${(e: Event) => state.mailDraft.smtp_port = numericInput(e, 587)}"
          value="${() => String(state.mailDraft.smtp_port)}" />
      </div>
      <div class="field-row">
        <label class="field-label">Username</label>
        <input class="field"
          @input="${(e: Event) => state.mailDraft.username = (e.target as HTMLInputElement).value}"
          value="${() => state.mailDraft.username}" />
      </div>
      <div class="field-row">
        <label class="field-label">Password</label>
        <input class="field" type="password"
          @input="${(e: Event) => state.mailDraft.password = (e.target as HTMLInputElement).value}"
          value="${() => state.mailDraft.password}" />
      </div>
      ${() => state.mailError ? html`<span class="error">${() => state.mailError}</span>` : html``}
      <div class="form-actions">
        <button class="btn primary" @click="${saveMailAccount}">Save account</button>
        <button class="btn" @click="${resetMailDraft}">Revert</button>
      </div>
    </div>
  `;
}

function renderConditionRow(i: number, c: MailCondition) {
  return html`
    <div class="condition-row">
      <select class="field field-narrow"
        @change="${(e: Event) => updateCondition(i, { field: (e.target as HTMLSelectElement).value })}">
        ${() => ['from', 'to', 'subject'].map(opt => html`
          <option value="${opt}" selected="${c.field === opt ? 'selected' : false}">${opt}</option>
        `)}
      </select>
      <select class="field field-narrow"
        @change="${(e: Event) => updateCondition(i, { match: (e.target as HTMLSelectElement).value })}">
        ${() => ['contains', 'equals', 'address', 'domain'].map(opt => html`
          <option value="${opt}" selected="${c.match === opt ? 'selected' : false}">${opt}</option>
        `)}
      </select>
      <input class="field" placeholder="value"
        @input="${(e: Event) => updateCondition(i, { value: (e.target as HTMLInputElement).value })}"
        value="${c.value}" />
      <button class="btn-text danger" @click="${() => removeCondition(i)}">×</button>
    </div>
  `;
}

function renderRuleForm() {
  return html`
    <div class="form">
      <input class="field" placeholder="rule name"
        @input="${(e: Event) => state.ruleForm.name = (e.target as HTMLInputElement).value}"
        value="${() => state.ruleForm.name}" />
      <div class="field-row">
        <label class="field-label">Action</label>
        <select class="field field-narrow"
          @change="${(e: Event) => state.ruleForm.action = (e.target as HTMLSelectElement).value}">
          ${() => ['smart_mailbox', 'move'].map(opt => html`
            <option value="${opt}" selected="${state.ruleForm.action === opt ? 'selected' : false}">${opt}</option>
          `)}
        </select>
      </div>
      ${() => state.ruleForm.action === 'move'
        ? html`
          <div class="field-row">
            <label class="field-label">Destination</label>
            <input class="field" placeholder="mailbox (e.g. Trash)"
              @input="${(e: Event) => state.ruleForm.dest = (e.target as HTMLInputElement).value}"
              value="${() => state.ruleForm.dest ?? ''}" />
          </div>
        `
        : html``}
      <div class="section-subhead">Conditions (all must match)</div>
      ${() => state.ruleForm.conditions.map((c, i) => renderConditionRow(i, c))}
      <button class="btn add" @click="${addCondition}">+ Add condition</button>
      ${() => state.ruleError ? html`<span class="error">${() => state.ruleError}</span>` : html``}
      <div class="form-actions">
        <button class="btn primary" @click="${submitRule}">Save rule</button>
        <button class="btn" @click="${cancelRule}">Cancel</button>
      </div>
    </div>
  `;
}

function renderRuleRow(rule: MailRule, index: number) {
  return html`
    <div class="row">
      <div class="row-info">
        <span class="row-label">${rule.name}</span>
        <span class="row-detail">${ruleTarget(rule)} · ${conditionSummary(rule)}</span>
      </div>
      <div class="row-actions">
        <button class="btn-text danger" @click="${() => removeRule(index)}">Remove</button>
      </div>
    </div>
  `.key(`rule-${index}-${rule.name}`);
}

function renderMail() {
  return html`
    <div class="section">
      <h2>Mail</h2>
      <p class="section-desc">Stored in <code>~/.config/sola/mail.json</code>. Used by <code>sola-mail</code>.</p>
      <div class="section-subhead">Account</div>
      ${renderMailAccount()}
      <div class="section-subhead">Rules</div>
      <div class="list">
        ${() => state.mail.rules.length > 0
          ? state.mail.rules.map((r, i) => renderRuleRow(r, i))
          : html`<div class="empty">No rules configured.</div>`}
      </div>
      ${() => state.addingRule
        ? renderRuleForm()
        : html`<button class="btn add" @click="${startAddRule}">+ Add rule</button>`}
    </div>
  `;
}

// --- Chrome -----------------------------------------------------------------

function navButton(id: Section, label: string) {
  return html`
    <button
      class="${() => 'nav-item' + (state.section === id ? ' active' : '')}"
      @click="${() => state.section = id}">${label}</button>
  `;
}

function renderSidebar() {
  return html`
    <nav class="sidebar">
      <div class="sidebar-title">Settings</div>
      ${navButton('applications', 'Applications')}
      ${navButton('mail', 'Mail')}
    </nav>
  `;
}

export async function createApp(root: HTMLElement): Promise<void> {
  const restored = (window as unknown as { RESTORED_STATE?: StatePayload }).RESTORED_STATE;
  if (restored) applyState(restored);

  on('state', (payload: unknown) => {
    const p = payload as StatePayload;
    if (p && p.applications) applyState(p);
  });

  html`
    <div class="layout">
      ${renderSidebar()}
      <main class="content">
        ${() => state.section === 'applications'
          ? renderApplications()
          : renderMail()}
      </main>
    </div>
  `(root);
}
