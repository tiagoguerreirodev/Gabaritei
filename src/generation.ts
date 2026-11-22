/**
 * The state of the background generation, shared by every page: the Gerar form, the Revisar
 * pacote page and the banner in the layout. The backend is the source of truth; this mirrors
 * it from an initial read and the `generation-status` events.
 */
import { createSignal, untrack } from "solid-js";
import { api } from "./api";
import type { DraftListItem } from "./bindings/DraftListItem";
import type { GenerationStatus } from "./bindings/GenerationStatus";

const [status, setStatusRaw] = createSignal<GenerationStatus | null>(null);
const [stage, setStage] = createSignal(0);
const [waiting, setWaiting] = createSignal<DraftListItem[]>([]);
let watching = false;

/**
 * The stages the backend reports, in order. The backend only exposes a stage through its fixed
 * pt-BR progress labels, so these are all that can be shown: saving is not reported.
 */
export const GENERATION_STAGES = [
  { id: "generating", label: "Gerando as questões" },
  { id: "checking", label: "Conferindo às cegas" },
  { id: "fixing", label: "Resolvendo as contestações" },
] as const;

/** Which stage a progress label starts, or `null` for a label inside the current stage. */
export function stageOfLabel(label: string): number | null {
  if (label.startsWith("Gerando as questões")) return 0;
  if (label.startsWith("Conferindo às cegas")) return 1;
  if (label.startsWith("Resolvendo as contestações") || label.startsWith("Reconferindo")) {
    return 2;
  }
  return null;
}

/** Index into `GENERATION_STAGES` of the stage a running generation is in. Never goes back. */
export const generationStage = stage;

function setStatus(next: GenerationStatus | null) {
  if (!next?.running) {
    setStage(0);
  } else {
    const started = next.progress ? stageOfLabel(next.progress.label) : null;
    if (started !== null) setStage((current) => Math.max(current, started));
  }
  setStatusRaw(next);
}

/** The latest known status (`null` until the first read). */
export const generationStatus = status;

/** The drafts waiting for review. They stay until imported or discarded, unlike `outcome`. */
export const waitingDrafts = waiting;

/** Reads the drafts waiting for review again. Call after anything that adds or removes one. */
export async function refreshWaitingDrafts() {
  try {
    setWaiting(await api.draftsWaiting());
  } catch {
    // Keep what we had: the banner is a convenience, and the Revisar pacote page reports errors.
  }
}

/** Starts mirroring the backend. Safe to call more than once. */
export function watchGeneration() {
  if (watching) return;
  watching = true;
  void api
    .generationStatus()
    .then(setStatus)
    .catch(() => undefined);
  void refreshWaitingDrafts();
  // A finished generation adds a draft, and a finished blind check changes its verdicts. The
  // list only changes when a run starts or ends, not on every progress update.
  void api.onGenerationStatus((next) => {
    const before = untrack(status);
    const changed =
      before?.running !== next.running || before?.outcome?.kind !== next.outcome?.kind;
    setStatus(next);
    if (changed) void refreshWaitingDrafts();
  });
}

/** Forgets how the last generation ended, here and in the backend. */
export async function dismissGeneration() {
  const current = untrack(status);
  if (current) setStatusRaw({ ...current, outcome: null });
  await api.generationDismiss().catch(() => undefined);
}

/** For tests: forget everything and allow `watchGeneration` to run again. */
export function resetGenerationWatch() {
  watching = false;
  setStatus(null);
  setWaiting([]);
}
