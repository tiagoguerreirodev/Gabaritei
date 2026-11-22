/**
 * The models and efforts each agent offers, read from the CLIs by the backend (`ai_catalog`).
 * Loaded once per app session and shared by Configurações and Gerar; "Atualizar" reads it again.
 */
import { createSignal } from "solid-js";
import { api } from "./api";
import type { Agent } from "./bindings/Agent";
import type { AgentCatalog } from "./bindings/AgentCatalog";
import type { Catalog } from "./bindings/Catalog";
import type { Effort } from "./bindings/Effort";
import type { RoleConfig } from "./bindings/RoleConfig";
import { efforts as knownEfforts } from "./pages/settings/labels";

/** `undefined` while not loaded yet, `null` if the request failed. */
export type CatalogValue = Catalog | null | undefined;

const [catalog, setCatalog] = createSignal<CatalogValue>(undefined);
let inflight: Promise<void> | undefined;

export const catalogState = catalog;

/**
 * Loads the lists the first time (a failed load is tried again), or reloads them with `refresh`.
 * Simultaneous calls share one request.
 */
export function loadCatalog(refresh = false): Promise<void> {
  if (inflight) return inflight;
  if (!refresh && catalog()) return Promise.resolve();
  const request: Promise<void> = api
    .aiCatalog(refresh)
    .then((loaded) => {
      setCatalog(loaded);
    })
    .catch(() => {
      setCatalog(null);
    })
    .finally(() => {
      inflight = undefined;
    });
  inflight = request;
  return request;
}

/** Forgets what was loaded. For tests. */
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

/** Efforts a free-text agent accepts. Claude's `--effort` stops at `max`. */
export function fallbackEfforts(agent: Agent): Effort[] {
  return knownEfforts.filter((effort) => agent === "codex" || effort !== "ultra");
}

/** The efforts the selected model supports, or the agent's fallback list if none are listed. */
export function effortOptions(value: CatalogValue, config: RoleConfig): Effort[] {
  const list = readyList(value, config.agent);
  if (!list) return fallbackEfforts(config.agent);
  const model = list.models.find((entry) => entry.id === config.model);
  if (model && model.efforts.length > 0) return model.efforts;
  // A model listed without efforts takes the efforts other models report.
  const reported = new Set(list.models.flatMap((entry) => entry.efforts));
  return reported.size > 0
    ? knownEfforts.filter((e) => reported.has(e))
    : fallbackEfforts(config.agent);
}

/**
 * Whether the role's model has Fast Mode. Only Codex's list says so. Claude's support is visible
 * only per run, so it (and any list that couldn't be read) is `"unknown"`, which shows the option.
 */
export function fastSupport(value: CatalogValue, config: RoleConfig): "yes" | "no" | "unknown" {
  if (config.agent !== "codex") return "unknown";
  const model = readyList(value, config.agent)?.models.find((entry) => entry.id === config.model);
  if (!model) return "unknown";
  return model.supportsFast ? "yes" : "no";
}

/** Why a role can't run with the lists as loaded, in pt-BR, or `undefined` if it can. */
export function roleProblem(value: CatalogValue, config: RoleConfig): string | undefined {
  const list = readyList(value, config.agent);
  if (!list) return undefined;
  const model = list.models.find((entry) => entry.id === config.model);
  if (!model) return `O modelo “${config.model}” não está mais disponível. Escolha outro.`;
  if (model.efforts.length > 0 && !model.efforts.includes(config.effort)) {
    return `O esforço “${config.effort}” não existe para o modelo “${config.model}”. Escolha outro.`;
  }
  if (config.fast && !model.supportsFast) {
    return `O modelo “${config.model}” não tem modo rápido. Desligue o modo rápido.`;
  }
  return undefined;
}

/** The supported effort closest to `wanted` (the higher one on a tie). */
function closestEffort(supported: Effort[], wanted: Effort): Effort {
  if (supported.includes(wanted)) return wanted;
  const rank = (effort: Effort) => knownEfforts.indexOf(effort);
  return (
    [...supported].sort(
      (a, b) =>
        Math.abs(rank(a) - rank(wanted)) - Math.abs(rank(b) - rank(wanted)) || rank(b) - rank(a),
    )[0] ?? wanted
  );
}

/** The role after switching to `model`, keeping the effort when the model supports it. */
export function roleForModel(value: CatalogValue, current: RoleConfig, model: string): RoleConfig {
  const next = { ...current, model };
  return {
    ...next,
    effort: closestEffort(effortOptions(value, next), current.effort),
    fast: current.fast && fastSupport(value, next) !== "no",
  };
}

/**
 * The role after switching to `agent`: its first listed model, or an empty model to type when it
 * has no list. The old agent's model name is never kept.
 */
export function roleForAgent(value: CatalogValue, agent: Agent, current: RoleConfig): RoleConfig {
  const first = readyList(value, agent)?.models[0];
  const next = { agent, model: first?.id ?? "", effort: current.effort, fast: false };
  return { ...next, effort: closestEffort(effortOptions(value, next), current.effort) };
}
