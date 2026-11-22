import { Show, createResource, createSignal } from "solid-js";
import { createStore, reconcile, unwrap } from "solid-js/store";
import { api } from "../api";
import { loadCatalog } from "../catalog";
import type { Agent } from "../bindings/Agent";
import type { AiSettings } from "../bindings/AiSettings";
import type { RoleConfig } from "../bindings/RoleConfig";
import type { RoleSettings } from "../bindings/RoleSettings";
import LegacyImport from "../components/LegacyImport";
import { ErrorState, Loading } from "../components/states";
import { showError, showToast } from "../components/toasts";
import AppearanceSection from "./settings/AppearanceSection";
import BackupSection from "./settings/BackupSection";
import DataFolderSection from "./settings/DataFolderSection";
import DataFileSection from "./settings/DataFileSection";
import { createBusy } from "./settings/busy";
import AgentsSection from "./settings/AgentsSection";
import RemNoteSection from "./settings/RemNoteSection";
import RolesSection from "./settings/RolesSection";
import ShortcutsSection from "./settings/ShortcutsSection";

/** A deep copy where no two places share an object (unlike `structuredClone`). */
function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** Configurações: CLIs, roles, RemNote and data. */
export default function Settings() {
  const [loaded, { refetch }] = createResource(() => api.aiSettings());
  const [legacy] = createResource(() => api.legacyImportStatus());
  // One flag for every section of Dados: a restore and an import must never overlap.
  const busy = createBusy();

  return (
    <section class="page">
      <h1 id="settings-title">Configurações</h1>
      <AppearanceSection />
      <ShortcutsSection />
      <Show
        when={!loaded.error}
        fallback={
          <ErrorState
            error={loaded.error as unknown}
            onRetry={() => refetch()}
            busy={loaded.loading}
            describedBy="settings-title"
          />
        }
      >
        <Show when={loaded()} fallback={<Loading />}>
          {(settings) => <Form initial={settings()} onReload={() => void refetch()} />}
        </Show>
      </Show>
      <h2>Dados</h2>
      <BackupSection busy={busy} />
      <DataFileSection busy={busy} />
      <DataFolderSection />
      <Show when={legacy()} fallback={<Loading />}>
        {(current) => <LegacyImport initialReport={current().report} busy={busy} />}
      </Show>
    </section>
  );
}

function Form(props: { initial: AiSettings; onReload: () => void }) {
  const [draft, setDraft] = createStore<AiSettings>(clone(props.initial));
  const [saving, setSaving] = createSignal(false);
  let savedPaths = JSON.stringify(props.initial.cliPaths);

  const save = async (quiet: boolean): Promise<boolean> => {
    setSaving(true);
    try {
      const { cliPaths, roles, remnote } = clone(unwrap(draft));
      const saved = await api.saveAiSettings({ cliPaths, roles, remnote });
      setDraft(reconcile(saved));
      // The model lists come from the CLIs, so a different CLI path means different lists.
      if (JSON.stringify(saved.cliPaths) !== savedPaths) {
        savedPaths = JSON.stringify(saved.cliPaths);
        void loadCatalog(true);
      }
      if (!quiet) showToast("Configurações salvas.");
      return true;
    } catch (error) {
      showError(error);
      return false;
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <AgentsSection
        paths={draft.cliPaths}
        onChange={(agent: Agent, path: string) => setDraft("cliPaths", agent, path || null)}
        beforeTest={() => save(true)}
      />
      <RolesSection
        roles={draft.roles}
        onChange={(key: keyof RoleSettings, config: RoleConfig) => setDraft("roles", key, config)}
      />
      <RemNoteSection
        remnote={draft.remnote}
        hasToken={draft.hasRemnoteToken}
        onChange={(remnote) => setDraft("remnote", remnote)}
        onTokenSaved={(has) => setDraft("hasRemnoteToken", has)}
        beforeTest={() => save(true)}
      />
      <p>
        <button type="button" class="primary" disabled={saving()} onClick={() => void save(false)}>
          {saving() ? "Salvando…" : "Salvar configurações"}
        </button>
      </p>
    </>
  );
}
