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
const [waiting, setWaiting] = createSignal<DraftListItem[]>([]);
let watching = false;
let waitingRequest = 0;

/** The ordered stages the backend reports for the current run. */
export const generationStages = () => status()?.stages ?? [];

/**
 * Index into `generationStages()` of the stage the running job is in, or `null` when none has
 * started yet (still preparing) or nothing is running.
 */
export const generationStage = (): number | null => {
  const current = status();
  if (!current?.running) return null;
  const index = generationStages().findIndex((stage) => stage.id === current.stage);
  return index < 0 ? null : index;
};

/** "Etapa 2 de 3: Conferindo às cegas", or "" when the stage is not known. */
export function generationStageSentence(): string {
  const index = generationStage();
  if (index === null) return "";
  return `Etapa ${index + 1} de ${generationStages().length}: ${generationStages()[index]?.label}`;
}

/** Whether cancelling now would discard the draft. */
export const cancelDiscardsDraft = () => status()?.cancelDiscardsDraft ?? false;

/** Whether the run on screen (or, once it ended, its outcome) is a re-check of a saved draft. */
export const isRecheckRun = () => {
  const current = status();
  return (current?.running ? current.kind : current?.outcomeKind) === "recheck";
};

/** The cancelled outcome's wording: a re-check keeps its draft, a generation saves nothing. */
export const cancelledMessage = (short = false) =>
  isRecheckRun()
    ? short
      ? "Conferência cancelada."
      : "Conferência cancelada. O pacote foi mantido."
    : short
      ? "Geração cancelada."
      : "Geração cancelada. Nada foi salvo.";

/** The failed outcome's wording. */
export const failedPrefix = () => (isRecheckRun() ? "A conferência falhou:" : "A geração falhou:");

/** Set by the progress panel on the Gerar page while it shows, so the banner can stay quiet. */
const [progressPanel, setProgressPanel] = createSignal(false);
export const generationPanelShown = progressPanel;
export const setGenerationPanelShown = setProgressPanel;

/** Updates the mirrored status when the snapshot is newer. Exported for tests. */
export function applyGenerationStatus(next: GenerationStatus) {
  const before = untrack(status);
  if (before && next.revision <= before.revision) return false;
  setStatusRaw(next);
  return true;
}

/** The latest known status (`null` until the first read). */
export const generationStatus = status;

/** Changes when a run starts or finishes, including across an unseen completion. */
export const generationRunRevision = () => status()?.runRevision ?? 0;

/** The drafts waiting for review. They stay until imported or discarded, unlike `outcome`. */
export const waitingDrafts = waiting;

/** Reads the drafts waiting for review again. Call after anything that adds or removes one. */
export async function refreshWaitingDrafts() {
  const request = ++waitingRequest;
  try {
    const drafts = await api.draftsWaiting();
    if (request === waitingRequest) setWaiting(drafts);
  } catch {
    // Keep what we had: the banner is a convenience, and the Revisar pacote page reports errors.
  }
}

/** Starts mirroring the backend. Safe to call more than once. */
export function watchGeneration() {
  if (watching) return;
  watching = true;
  // Listen first to cover transitions during the read. Snapshots may arrive in either order.
  const update = (next: GenerationStatus) => {
    const before = untrack(status);
    if (
      applyGenerationStatus(next) &&
      (before === null || before.runRevision !== next.runRevision)
    ) {
      void refreshWaitingDrafts();
    }
  };
  void api
    .onGenerationStatus(update)
    .catch(() => undefined)
    .then(() => api.generationStatus())
    .then(update)
    .catch(() => {
      if (untrack(status) === null) void refreshWaitingDrafts();
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
  waitingRequest++;
  setStatusRaw(null);
  setWaiting([]);
}
