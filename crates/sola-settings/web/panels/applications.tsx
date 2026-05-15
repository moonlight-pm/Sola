// Applications panel. Two Cards: "Configured" (editable rows + add)
// and "Running, not configured" (candidate rows with one-click
// Configure that spawns a draft in the Configured list).
//
// Drafts are local to this panel; commit happens via the
// applications_add / applications_update JS commands. Errors come
// back as { error } from invoke and render inline.

import { type Handle } from "@remix-run/ui";
import { Badge } from "@sola/badge";
import { Button } from "@sola/button";
import { Card } from "@sola/card";
import { Field } from "@sola/field";
import { invoke } from "@sola/ipc";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";

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

export interface ApplicationsState {
  apps: Application[];
  missing: string[];
  candidates: Candidate[];
}

export interface ApplicationsProps {
  state: ApplicationsState;
}

const DEBOUNCE_MS = 500;

interface DraftRow {
  app_id: string;
  label: string;
  command: string;
  icon: string;
  /** Stable client-side key — null for committed rows (we key by app_id),
      string for unsaved drafts (keeps DOM identity stable across updates). */
  draftKey: string | null;
  error: string;
}

function emptyDraft(seed?: Partial<Application>): DraftRow {
  return {
    app_id: seed?.app_id ?? "",
    label: seed?.label ?? seed?.app_id ?? "",
    command: seed?.command ?? "",
    icon: seed?.icon ?? "",
    draftKey: `draft-${Date.now()}-${Math.random()}`,
    error: "",
  };
}

export function ApplicationsPanel(handle: Handle<ApplicationsProps>) {
  // Pending drafts that haven't been committed yet. After commit,
  // the canonical row replaces the draft on the next `state` event.
  let drafts: DraftRow[] = [];

  // Per-row debounce timers, keyed by app_id (committed) or draftKey
  // (uncommitted). Cleared on commit or unmount.
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  // Per-row inline error display. Keyed by app_id or draftKey.
  const rowErrors = new Map<string, string>();

  const clearTimer = (key: string) => {
    const t = timers.get(key);
    if (t) {
      clearTimeout(t);
      timers.delete(key);
    }
  };

  const update = () => handle.update();

  const setRowError = (key: string, msg: string) => {
    if (msg) rowErrors.set(key, msg);
    else rowErrors.delete(key);
    update();
  };

  const commitDraft = async (draft: DraftRow) => {
    if (
      !draft.app_id.trim() ||
      !draft.label.trim() ||
      !draft.command.trim()
    ) {
      // Don't commit incomplete drafts silently — surface a hint.
      setRowError(
        draft.draftKey ?? "",
        "app_id, label, and command are required",
      );
      return;
    }
    setRowError(draft.draftKey ?? "", "");
    try {
      await invoke("applications_add", {
        app_id: draft.app_id.trim(),
        label: draft.label.trim(),
        command: draft.command.trim(),
        icon: draft.icon.trim(),
      });
      // Drop the draft — the canonical row will arrive via state event.
      drafts = drafts.filter((d) => d.draftKey !== draft.draftKey);
      update();
    } catch (e) {
      setRowError(draft.draftKey ?? "", String(e));
    }
  };

  const commitUpdate = async (originalAppId: string, edits: Application) => {
    setRowError(originalAppId, "");
    try {
      await invoke("applications_update", {
        old_app_id: originalAppId,
        app_id: edits.app_id.trim(),
        label: edits.label.trim(),
        command: edits.command.trim(),
        icon: edits.icon.trim(),
      });
    } catch (e) {
      setRowError(originalAppId, String(e));
    }
  };

  const scheduleUpdate = (originalAppId: string, edits: Application) => {
    clearTimer(originalAppId);
    timers.set(
      originalAppId,
      setTimeout(() => {
        timers.delete(originalAppId);
        commitUpdate(originalAppId, edits);
      }, DEBOUNCE_MS),
    );
  };

  const removeApp = async (appId: string) => {
    try {
      await invoke("applications_remove", { app_id: appId });
    } catch (e) {
      setRowError(appId, String(e));
    }
  };

  const startConfigure = (c: Candidate) => {
    drafts = [
      emptyDraft({
        app_id: c.app_id,
        label: c.app_id,
        command: c.suggested_command ?? "",
        icon: "",
      }),
      ...drafts,
    ];
    update();
  };

  const startAddBlank = () => {
    drafts = [...drafts, emptyDraft()];
    update();
  };

  const discardDraft = (key: string) => {
    drafts = drafts.filter((d) => d.draftKey !== key);
    rowErrors.delete(key);
    update();
  };

  const renderConfiguredRow = (app: Application) => {
    // Working copy — each render reads from canonical state, edits flow
    // through invoke→state push.
    const working: Application = { ...app };
    const onField = (field: keyof Application) => (v: string) => {
      working[field] = v;
      scheduleUpdate(app.app_id, working);
    };

    return (
      <Stack gap="xs">
        <Stack direction="row" gap="md" align="center">
          <Text>{app.label || app.app_id}</Text>
          {handle.props.state.missing.includes(app.app_id)
            ? <Badge kind="warning">not found</Badge>
            : null}
          <Button
            variant="danger"
            confirm
            confirmLabel="Click again to remove"
            onPress={() => removeApp(app.app_id)}
          >
            Remove
          </Button>
        </Stack>
        <Stack direction="row" gap="sm">
          <Field label="app_id">
            <TextInput
              value={app.app_id}
              onChange={onField("app_id")}
            />
          </Field>
          <Field label="label">
            <TextInput
              value={app.label}
              onChange={onField("label")}
            />
          </Field>
          <Field label="command">
            <TextInput
              value={app.command}
              onChange={onField("command")}
            />
          </Field>
          <Field label="icon">
            <TextInput
              value={app.icon}
              onChange={onField("icon")}
            />
          </Field>
        </Stack>
        {rowErrors.get(app.app_id)
          ? <Text tone="muted">{rowErrors.get(app.app_id)}</Text>
          : null}
      </Stack>
    );
  };

  const renderDraftRow = (draft: DraftRow) => {
    const onField = (field: keyof DraftRow) => (v: string) => {
      (draft as Record<string, unknown>)[field] = v;
      update();
    };

    return (
      <Stack gap="xs">
        <Stack direction="row" gap="md" align="center">
          <Text tone="muted">New application</Text>
          <Button
            variant="primary"
            onPress={() => commitDraft(draft)}
          >
            Add
          </Button>
          <Button
            variant="ghost"
            onPress={() => discardDraft(draft.draftKey!)}
          >
            Discard
          </Button>
        </Stack>
        <Stack direction="row" gap="sm">
          <Field label="app_id">
            <TextInput
              value={draft.app_id}
              onInput={onField("app_id")}
              placeholder="firefox"
            />
          </Field>
          <Field label="label">
            <TextInput
              value={draft.label}
              onInput={onField("label")}
              placeholder="Firefox"
            />
          </Field>
          <Field label="command">
            <TextInput
              value={draft.command}
              onInput={onField("command")}
              placeholder="firefox"
            />
          </Field>
          <Field label="icon">
            <TextInput
              value={draft.icon}
              onInput={onField("icon")}
              placeholder="simpleicons/firefox"
            />
          </Field>
        </Stack>
        {rowErrors.get(draft.draftKey ?? "")
          ? <Text tone="muted">{rowErrors.get(draft.draftKey ?? "")}</Text>
          : null}
      </Stack>
    );
  };

  const renderCandidate = (c: Candidate) => (
    <Stack direction="row" gap="md" align="center">
      <Stack gap="xs">
        <Text>{c.app_id}</Text>
        <Text tone="muted">
          {c.title || "(no title)"}
          {c.suggested_command
            ? ` · ${c.suggested_command}`
            : " · command unknown — fill in manually"}
        </Text>
      </Stack>
      <Button variant="ghost" onPress={() => startConfigure(c)}>
        Configure
      </Button>
    </Stack>
  );

  return () => {
    const { apps, candidates } = handle.props.state;
    return (
      <Stack gap="xl">
        <Card
          label="Configured"
          description="Edits commit half a second after the last keystroke."
        >
          <Stack gap="lg">
            {apps.length === 0 && drafts.length === 0
              ? <Text tone="muted">No applications configured.</Text>
              : null}
            {drafts.map(renderDraftRow)}
            {apps.map(renderConfiguredRow)}
            <Button variant="ghost" onPress={startAddBlank}>
              + Add application
            </Button>
          </Stack>
        </Card>
        {candidates.length > 0
          ? (
            <Card
              label="Running, not configured"
              description="Pre-filled by what's currently running. One click drops a draft into Configured."
            >
              <Stack gap="md">
                {candidates.map(renderCandidate)}
              </Stack>
            </Card>
          )
          : null}
      </Stack>
    );
  };
}
