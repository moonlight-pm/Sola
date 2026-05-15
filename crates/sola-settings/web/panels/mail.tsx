// Mail panel. Two Cards: "Account" (explicit save/revert) and
// "Rules" (one Card per rule with inline edit + save/discard, plus
// + Add rule). The bus contract added `mail_update_rule` so a rule
// can be patched without delete+add.

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { Card } from "@sola/card";
import { Field } from "@sola/field";
import { invoke } from "@sola/ipc";
import { NumberInput } from "@sola/number-input";
import { PopoverSelect } from "@sola/popover-select";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";

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

export interface MailConfig {
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  username: string;
  password: string;
  rules: MailRule[];
}

export interface MailProps {
  state: MailConfig;
}

function emptyAccount(): MailConfig {
  return {
    email: "",
    imap_host: "",
    imap_port: 993,
    smtp_host: "",
    smtp_port: 587,
    username: "",
    password: "",
    rules: [],
  };
}

function emptyRule(): MailRule {
  return {
    name: "",
    action: "smart_mailbox",
    dest: "",
    conditions: [],
  };
}

function emptyCondition(): MailCondition {
  return { field: "from", match: "contains", value: "" };
}

function ruleEquals(a: MailRule, b: MailRule): boolean {
  if (a.name !== b.name || a.action !== b.action || a.dest !== b.dest) {
    return false;
  }
  if (a.conditions.length !== b.conditions.length) return false;
  for (let i = 0; i < a.conditions.length; i++) {
    const ca = a.conditions[i];
    const cb = b.conditions[i];
    if (ca.field !== cb.field || ca.match !== cb.match || ca.value !== cb.value) {
      return false;
    }
  }
  return true;
}

function accountEquals(a: MailConfig, b: MailConfig): boolean {
  return (
    a.email === b.email &&
    a.imap_host === b.imap_host &&
    a.imap_port === b.imap_port &&
    a.smtp_host === b.smtp_host &&
    a.smtp_port === b.smtp_port &&
    a.username === b.username &&
    a.password === b.password
  );
}

interface RuleDraft {
  /** -1 for new (unsaved) rules; the canonical index otherwise. */
  index: number;
  /** Stable client-side key — needed only for new rules so DOM order
      doesn't get confused if multiple new rules are being authored. */
  key: string;
  draft: MailRule;
}

const FIELD_OPTIONS = [
  { value: "from", label: "from" },
  { value: "to", label: "to" },
  { value: "subject", label: "subject" },
];

const MATCH_OPTIONS = [
  { value: "contains", label: "contains" },
  { value: "equals", label: "equals" },
  { value: "address", label: "address" },
  { value: "domain", label: "domain" },
];

const ACTION_OPTIONS = [
  { value: "smart_mailbox", label: "smart mailbox" },
  { value: "move", label: "move" },
];

export function MailPanel(handle: Handle<MailProps>) {
  // Local drafts. The account draft is a clone of the canonical state.
  let accountDraft: MailConfig = { ...handle.props.state };
  let accountError = "";

  // Per-rule drafts, addressed by canonical index (existing) or by
  // a client-side key (new unsaved rules).
  let existingDrafts = new Map<number, MailRule>();
  let newRules: RuleDraft[] = [];
  const ruleErrors = new Map<string, string>();

  // Re-sync drafts on every external state change (the on('state', …)
  // listener in main.tsx re-renders us). Drafts that diverge from
  // canonical are preserved; drafts that match canonical get refreshed.
  let lastState: MailConfig = handle.props.state;
  const syncFromState = (next: MailConfig) => {
    if (accountEquals(accountDraft, lastState)) {
      accountDraft = { ...next };
    }
    // existingDrafts: drop drafts that are clean OR refer to indices
    // that no longer exist.
    const carry = new Map<number, MailRule>();
    for (const [idx, draft] of existingDrafts) {
      if (idx >= next.rules.length) continue;
      if (!ruleEquals(draft, lastState.rules[idx])) {
        carry.set(idx, draft); // dirty draft — preserve
      }
    }
    existingDrafts = carry;
    lastState = next;
  };

  const update = () => handle.update();

  const rerender = () => {
    syncFromState(handle.props.state);
    update();
  };

  const accountDirty = () => !accountEquals(accountDraft, lastState);

  const saveAccount = async () => {
    accountError = "";
    try {
      await invoke("mail_save_account", {
        email: accountDraft.email.trim(),
        imap_host: accountDraft.imap_host.trim(),
        imap_port: accountDraft.imap_port || 993,
        smtp_host: accountDraft.smtp_host.trim(),
        smtp_port: accountDraft.smtp_port || 587,
        username: accountDraft.username.trim(),
        password: accountDraft.password,
      });
    } catch (e) {
      accountError = String(e);
      update();
    }
  };

  const revertAccount = () => {
    accountDraft = { ...lastState };
    accountError = "";
    update();
  };

  const onAccountField = <K extends keyof MailConfig>(
    field: K,
    coerce: (raw: string) => MailConfig[K] = (s) => s as MailConfig[K],
  ) =>
    (v: string) => {
      accountDraft = { ...accountDraft, [field]: coerce(v) };
      update();
    };

  const numericField = (raw: string, fallback: number): number => {
    const n = Number(raw);
    return Number.isFinite(n) && n > 0 ? n : fallback;
  };

  // Existing rule drafts: lazily created on first edit.
  const ensureDraft = (index: number): MailRule => {
    const existing = existingDrafts.get(index);
    if (existing) return existing;
    const fresh = { ...lastState.rules[index] };
    existingDrafts.set(index, fresh);
    return fresh;
  };

  const editExistingRule = (index: number, patch: Partial<MailRule>) => {
    const next = { ...ensureDraft(index), ...patch };
    existingDrafts.set(index, next);
    update();
  };

  const saveExistingRule = async (index: number) => {
    const draft = existingDrafts.get(index);
    if (!draft) return;
    try {
      await invoke("mail_update_rule", {
        index,
        name: draft.name.trim(),
        action: draft.action,
        dest: draft.action === "move" ? (draft.dest ?? "").trim() : null,
        conditions: draft.conditions.map((c) => ({
          field: c.field,
          match: c.match,
          value: c.value.trim(),
        })),
      });
      existingDrafts.delete(index);
      ruleErrors.delete(`existing-${index}`);
    } catch (e) {
      ruleErrors.set(`existing-${index}`, String(e));
    }
    update();
  };

  const discardExistingRule = (index: number) => {
    existingDrafts.delete(index);
    ruleErrors.delete(`existing-${index}`);
    update();
  };

  const removeRule = async (index: number) => {
    try {
      await invoke("mail_remove_rule", { index });
      existingDrafts.delete(index);
    } catch (e) {
      ruleErrors.set(`existing-${index}`, String(e));
    }
    update();
  };

  // New rule drafts.
  const startAddRule = () => {
    newRules = [
      ...newRules,
      {
        index: -1,
        key: `new-${Date.now()}-${Math.random()}`,
        draft: emptyRule(),
      },
    ];
    update();
  };

  const editNewRule = (key: string, patch: Partial<MailRule>) => {
    newRules = newRules.map((r) =>
      r.key === key ? { ...r, draft: { ...r.draft, ...patch } } : r,
    );
    update();
  };

  const saveNewRule = async (key: string) => {
    const entry = newRules.find((r) => r.key === key);
    if (!entry) return;
    const d = entry.draft;
    try {
      await invoke("mail_add_rule", {
        name: d.name.trim(),
        action: d.action,
        dest: d.action === "move" ? (d.dest ?? "").trim() : null,
        conditions: d.conditions.map((c) => ({
          field: c.field,
          match: c.match,
          value: c.value.trim(),
        })),
      });
      newRules = newRules.filter((r) => r.key !== key);
      ruleErrors.delete(`new-${key}`);
    } catch (e) {
      ruleErrors.set(`new-${key}`, String(e));
    }
    update();
  };

  const discardNewRule = (key: string) => {
    newRules = newRules.filter((r) => r.key !== key);
    ruleErrors.delete(`new-${key}`);
    update();
  };

  const addCondition = (
    onChange: (next: MailCondition[]) => void,
    current: MailCondition[],
  ) => {
    onChange([...current, emptyCondition()]);
  };

  const updateCondition = (
    idx: number,
    patch: Partial<MailCondition>,
    onChange: (next: MailCondition[]) => void,
    current: MailCondition[],
  ) => {
    onChange(
      current.map((c, i) => (i === idx ? { ...c, ...patch } : c)),
    );
  };

  const removeCondition = (
    idx: number,
    onChange: (next: MailCondition[]) => void,
    current: MailCondition[],
  ) => {
    onChange(current.filter((_, i) => i !== idx));
  };

  const renderConditions = (
    conditions: MailCondition[],
    onChange: (next: MailCondition[]) => void,
  ) => (
    <Stack gap="sm">
      {conditions.map((c, i) => (
        <Stack direction="row" gap="sm" align="center">
          <PopoverSelect
            options={FIELD_OPTIONS}
            value={c.field}
            onChange={(v) =>
              updateCondition(i, { field: v }, onChange, conditions)}
          />
          <PopoverSelect
            options={MATCH_OPTIONS}
            value={c.match}
            onChange={(v) =>
              updateCondition(i, { match: v }, onChange, conditions)}
          />
          <TextInput
            value={c.value}
            placeholder="value"
            onChange={(v) =>
              updateCondition(i, { value: v }, onChange, conditions)}
          />
          <Button
            variant="ghost"
            confirm
            confirmLabel="Click again"
            onPress={() => removeCondition(i, onChange, conditions)}
          >
            Remove
          </Button>
        </Stack>
      ))}
      <Button
        variant="ghost"
        onPress={() => addCondition(onChange, conditions)}
      >
        + Add condition
      </Button>
    </Stack>
  );

  const renderRuleBody = (
    rule: MailRule,
    onChange: (next: MailRule) => void,
  ) => (
    <Stack gap="md">
      <Field label="Name">
        <TextInput
          value={rule.name}
          onChange={(v) => onChange({ ...rule, name: v })}
          placeholder="rule name"
        />
      </Field>
      <Field label="Action">
        <PopoverSelect
          options={ACTION_OPTIONS}
          value={rule.action}
          onChange={(v) => onChange({ ...rule, action: v })}
        />
      </Field>
      {rule.action === "move"
        ? (
          <Field label="Destination">
            <TextInput
              value={rule.dest ?? ""}
              onChange={(v) => onChange({ ...rule, dest: v })}
              placeholder="mailbox (e.g. Trash)"
            />
          </Field>
        )
        : null}
      <Text kind="label">Conditions (all must match)</Text>
      {renderConditions(rule.conditions, (next) =>
        onChange({ ...rule, conditions: next }))}
    </Stack>
  );

  return () => {
    rerender();

    return (
      <Stack gap="xl">
        <Card
          label="Account"
          description="IMAP receive + SMTP send credentials."
        >
          <Stack gap="md">
            <Field label="Email">
              <TextInput
                type="email"
                value={accountDraft.email}
                onChange={onAccountField("email")}
              />
            </Field>
            <Field label="IMAP host">
              <TextInput
                value={accountDraft.imap_host}
                onChange={onAccountField("imap_host")}
              />
            </Field>
            <Field label="IMAP port">
              <NumberInput
                value={`${accountDraft.imap_port}`}
                unit=""
                step={1}
                min={1}
                max={65535}
                onChange={(s) =>
                  onAccountField("imap_port", (raw) =>
                    numericField(raw, 993) as MailConfig["imap_port"])(s)}
              />
            </Field>
            <Field label="SMTP host">
              <TextInput
                value={accountDraft.smtp_host}
                onChange={onAccountField("smtp_host")}
              />
            </Field>
            <Field label="SMTP port">
              <NumberInput
                value={`${accountDraft.smtp_port}`}
                unit=""
                step={1}
                min={1}
                max={65535}
                onChange={(s) =>
                  onAccountField("smtp_port", (raw) =>
                    numericField(raw, 587) as MailConfig["smtp_port"])(s)}
              />
            </Field>
            <Field label="Username">
              <TextInput
                value={accountDraft.username}
                onChange={onAccountField("username")}
              />
            </Field>
            <Field label="Password">
              <TextInput
                type="password"
                value={accountDraft.password}
                onChange={onAccountField("password")}
              />
            </Field>
            <Stack direction="row" gap="md">
              <Button
                variant="primary"
                disabled={!accountDirty()}
                onPress={saveAccount}
              >
                Save account
              </Button>
              <Button
                variant="ghost"
                disabled={!accountDirty()}
                onPress={revertAccount}
              >
                Revert
              </Button>
            </Stack>
            {accountError
              ? <Text tone="muted">{accountError}</Text>
              : null}
          </Stack>
        </Card>

        <Card
          label="Rules"
          description="Each condition row must match for the rule to fire."
        >
          <Stack gap="lg">
            {lastState.rules.length === 0 && newRules.length === 0
              ? <Text tone="muted">No rules configured.</Text>
              : null}
            {lastState.rules.map((rule, index) => {
              const draft = existingDrafts.get(index);
              const working = draft ?? rule;
              const dirty = draft !== undefined &&
                !ruleEquals(draft, rule);
              return (
                <Card label={working.name || "(unnamed rule)"}>
                  <Stack gap="md">
                    {renderRuleBody(working, (next) =>
                      editExistingRule(index, next))}
                    <Stack direction="row" gap="md">
                      <Button
                        variant="primary"
                        disabled={!dirty}
                        onPress={() => saveExistingRule(index)}
                      >
                        Save
                      </Button>
                      <Button
                        variant="ghost"
                        disabled={!dirty}
                        onPress={() => discardExistingRule(index)}
                      >
                        Discard
                      </Button>
                      <Button
                        variant="danger"
                        confirm
                        confirmLabel="Click again to remove"
                        onPress={() => removeRule(index)}
                      >
                        Remove rule
                      </Button>
                    </Stack>
                    {ruleErrors.get(`existing-${index}`)
                      ? (
                        <Text tone="muted">
                          {ruleErrors.get(`existing-${index}`)}
                        </Text>
                      )
                      : null}
                  </Stack>
                </Card>
              );
            })}
            {newRules.map((entry) => (
              <Card label={entry.draft.name || "(new rule)"}>
                <Stack gap="md">
                  {renderRuleBody(entry.draft, (next) =>
                    editNewRule(entry.key, next))}
                  <Stack direction="row" gap="md">
                    <Button
                      variant="primary"
                      onPress={() => saveNewRule(entry.key)}
                    >
                      Save
                    </Button>
                    <Button
                      variant="ghost"
                      onPress={() => discardNewRule(entry.key)}
                    >
                      Discard
                    </Button>
                  </Stack>
                  {ruleErrors.get(`new-${entry.key}`)
                    ? (
                      <Text tone="muted">
                        {ruleErrors.get(`new-${entry.key}`)}
                      </Text>
                    )
                    : null}
                </Stack>
              </Card>
            ))}
            <Button variant="ghost" onPress={startAddRule}>
              + Add rule
            </Button>
          </Stack>
        </Card>
      </Stack>
    );
  };
}
