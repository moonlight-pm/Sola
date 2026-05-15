// Settings root. Owns canonical state (single `on('state', …)`
// listener), section state, and the Sidebar / Container shell.
// Each panel reads its slice via props.

import { type Handle } from "@remix-run/ui";
import { Container } from "@sola/container";
import { on } from "@sola/ipc";
import { Root } from "@sola/root";
import { Sidebar, SidebarItem, SidebarSection } from "@sola/sidebar";
import { Split } from "@sola/split";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

import {
  ApplicationsPanel,
  type ApplicationsState,
} from "./panels/applications.tsx";
import { MailPanel, type MailConfig } from "./panels/mail.tsx";

type Section = "applications" | "mail";

interface SettingsState {
  applications: ApplicationsState;
  mail: MailConfig;
}

function emptyState(): SettingsState {
  return {
    applications: { apps: [], missing: [], candidates: [] },
    mail: {
      email: "",
      imap_host: "",
      imap_port: 993,
      smtp_host: "",
      smtp_port: 587,
      username: "",
      password: "",
      rules: [],
    },
  };
}

export function Main(handle: Handle) {
  let section: Section = "applications";
  let state: SettingsState = emptyState();

  on("state", (payload: unknown) => {
    const p = payload as Partial<SettingsState>;
    state = {
      applications: p.applications ?? state.applications,
      mail: p.mail ?? state.mail,
    };
    handle.update();
  });

  const setSection = (s: Section) => {
    if (s === section) return;
    section = s;
    handle.update();
  };

  return () => (
    <Root>
      <Split direction="row" position="240px">
        <Sidebar>
          <SidebarSection label="Settings">
            <SidebarItem
              active={section === "applications"}
              onSelect={() => setSection("applications")}
            >
              Applications
            </SidebarItem>
            <SidebarItem
              active={section === "mail"}
              onSelect={() => setSection("mail")}
            >
              Mail
            </SidebarItem>
          </SidebarSection>
        </Sidebar>
        <Container maxWidth="article">
          <Stack gap="xl">
            <Text kind="display">
              {section === "applications" ? "Applications" : "Mail"}
            </Text>
            {section === "applications"
              ? <ApplicationsPanel state={state.applications} />
              : <MailPanel state={state.mail} />}
          </Stack>
        </Container>
      </Split>
    </Root>
  );
}
