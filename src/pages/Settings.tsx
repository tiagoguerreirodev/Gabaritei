import { createResource } from "solid-js";
import { createStore, reconcile, unwrap } from "solid-js/store";
import { api } from "../api";
import { createRoleEditor, refreshCatalog, resetCatalog } from "../catalog";
import type { Agent } from "../bindings/Agent";
import type { AiSettings } from "../bindings/AiSettings";
import { cloneJson } from "../json";
import LegacyImport from "../components/LegacyImport";
import { ResourceView } from "../components/states";
import { showSuccess } from "../components/toasts";
import AppearanceSection from "./settings/AppearanceSection";
import BackupSection from "./settings/BackupSection";
import DataFolderSection from "./settings/DataFolderSection";
import DataFileSection from "./settings/DataFileSection";
import { createAction } from "../actions";
import AgentsSection from "./settings/AgentsSection";
import RemNoteSection from "./settings/RemNoteSection";
import { roleOrder } from "../labels";
import RolesSection from "./settings/RolesSection";
import ShortcutsSection from "./settings/ShortcutsSection";

/** Configurações: CLIs, roles, RemNote and data. */
export default function Settings() {
  const [loaded, { refetch }] = createResource(() => api.aiSettings());
  const [legacy, { refetch: refetchLegacy }] = createResource(() => api.legacyImportStatus());
  // One flag for every section of Dados: a restore and an import must never overlap.
  const busy = createAction();

  return (
    <section class="page">
      <h1 id="settings-title">Configurações</h1>
      <AppearanceSection />
      <ShortcutsSection />
      <ResourceView
        value={loaded()}
        error={() => loaded.error}
        pending={loaded.state === "pending"}
        retrying={loaded.loading}
        onRetry={() => refetch()}
        describedBy="settings-title"
      >
        {(settings) => <Form initial={settings()} onReload={() => void refetch()} />}
      </ResourceView>
      <h2 id="settings-data-title">Dados</h2>
      <BackupSection busy={busy} />
      <DataFileSection busy={busy} />
      <DataFolderSection />
      <ResourceView
        value={legacy()}
        error={() => legacy.error}
        pending={legacy.state === "pending"}
        retrying={legacy.loading}
        onRetry={() => refetchLegacy()}
        describedBy="settings-data-title"
      >
        {(current) => <LegacyImport initialReport={current().report} busy={busy} />}
      </ResourceView>
    </section>
  );
}

function Form(props: { initial: AiSettings; onReload: () => void }) {
  const [draft, setDraft] = createStore<AiSettings>(cloneJson(props.initial));
  const saving = createAction();
  let savedPaths = JSON.stringify(props.initial.cliPaths);
  const editors = roleOrder.map(({ role, key }) => ({
    role,
    editor: createRoleEditor(
      () => draft.roles[key],
      (config) => setDraft("roles", key, config),
    ),
  }));
  const changingRole = () => editors.some(({ editor }) => editor.changing());

  const persistSettings = () => {
    if (changingRole()) return Promise.resolve(false);
    return saving.run(async () => {
      const { cliPaths, roles, remnote } = cloneJson(unwrap(draft));
      const saved = await api.saveAiSettings({ cliPaths, roles, remnote });
      setDraft(reconcile(saved));
      // The model lists come from the CLIs, so a different CLI path means different lists.
      if (JSON.stringify(saved.cliPaths) !== savedPaths) {
        savedPaths = JSON.stringify(saved.cliPaths);
        resetCatalog();
        void refreshCatalog();
      }
    });
  };

  const saveSettings = async () => {
    if (await persistSettings()) showSuccess("Configurações salvas.");
  };

  return (
    <>
      <AgentsSection
        paths={draft.cliPaths}
        onChange={(agent: Agent, path: string) => setDraft("cliPaths", agent, path || null)}
        beforeTest={persistSettings}
      />
      <RolesSection editors={editors} />
      <RemNoteSection
        remnote={draft.remnote}
        hasToken={draft.hasRemnoteToken}
        onChange={(remnote) => setDraft("remnote", remnote)}
        onTokenSaved={(has) => setDraft("hasRemnoteToken", has)}
        beforeTest={persistSettings}
      />
      <p>
        <button
          type="button"
          class="btn btn-primary"
          disabled={saving.active() || changingRole()}
          onClick={() => void saveSettings()}
        >
          {saving.active() ? "Salvando…" : "Salvar configurações"}
        </button>
      </p>
    </>
  );
}
