/**
 * The models and efforts each agent offers, read from the CLIs by the backend (`ai_catalog`).
 * Loaded once per app session and shared by Configurações and Gerar; "Atualizar" reads it again.
 */
import { createResource, createSignal, type Accessor } from "solid-js";
import { api } from "./api";
import type { Agent } from "./bindings/Agent";
import type { AgentCatalog } from "./bindings/AgentCatalog";
import type { Catalog } from "./bindings/Catalog";
import type { RoleChange } from "./bindings/RoleChange";
import type { RoleConfig } from "./bindings/RoleConfig";
import { showError } from "./components/toasts";

/** `undefined` while not loaded yet, `null` if the request failed. */
export type CatalogValue = Catalog | null | undefined;

const [catalog, setCatalog] = createSignal<CatalogValue>(undefined);
let inflight: Promise<void> | undefined;

export const catalogState = catalog;

/** Loads the lists once, retrying a failed load. */
export function loadCatalog(): Promise<void> {
  if (inflight) return inflight;
  return catalog() ? Promise.resolve() : fetchCatalog(api.aiCatalog);
}

/** Reloads the lists from the CLIs. */
export function refreshCatalog(): Promise<void> {
  return fetchCatalog(api.refreshAiCatalog);
}

/** Simultaneous calls share one request. */
function fetchCatalog(load: () => Promise<Catalog>): Promise<void> {
  if (inflight) return inflight;
  const request = load()
    .then((loaded) => {
      if (inflight === request) setCatalog(loaded);
    })
    .catch(() => {
      if (inflight === request) setCatalog(null);
    })
    .finally(() => {
      if (inflight === request) inflight = undefined;
    });
  inflight = request;
  return request;
}

/** Forgets the lists and any earlier request after the configured CLI paths changed. */
export function resetCatalog(): void {
  inflight = undefined;
  setCatalog(undefined);
}

/** The agent's list, if it was read. `undefined` for unloaded, failed or unavailable lists. */
export function readyList(value: CatalogValue, agent: Agent): AgentCatalog | undefined {
  const found = value?.agents.find((entry) => entry.agent === agent);
  return found?.status.kind === "ready" ? found : undefined;
}

/** Why the agent's list couldn't be read, in pt-BR. */
export function unavailableReason(value: CatalogValue, agent: Agent): string | undefined {
  if (value === null) return "Não foi possível carregar as listas de modelos. Digite o modelo.";
  const found = value?.agents.find((entry) => entry.agent === agent);
  return found?.status.kind === "unavailable" ? found.status.reason : undefined;
}

/** Tracks the backend's choices for the current role and applies agent or model switches. */
export function createRoleEditor(
  value: Accessor<RoleConfig>,
  onChange: (config: RoleConfig) => void,
) {
  const [options] = createResource(
    () => {
      // A catalog refresh can change the available choices for the same config.
      if (catalogState() === undefined) return undefined;
      return { ...value() };
    },
    (config) => api.aiRoleOptions(config).catch(() => undefined),
  );
  const [changing, setChanging] = createSignal(false);
  const change = async (edit: RoleChange) => {
    if (changing()) return;
    setChanging(true);
    try {
      onChange(await api.aiChangeRole({ ...value() }, edit));
    } catch (error) {
      showError(error);
    } finally {
      setChanging(false);
    }
  };
  return { value, options, onChange, change, changing };
}

export type RoleEditor = ReturnType<typeof createRoleEditor>;
