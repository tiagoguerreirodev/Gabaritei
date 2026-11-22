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

/**
 * The stages of a run, in order. The backend reports the current one as `status.stage`; saving
 * is not reported, so it is not listed.
 */
export const GENERATION_STAGES = [
  { id: "generating", label: "Gerando as questões" },
  { id: "checking", label: "Conferindo às cegas" },
  { id: "resolving", label: "Resolvendo as contestações" },
] as const;

/** The stages that apply to the running job: two for a re-check, three for a generation. */
export const generationStages = () =>
  status()?.kind === "recheck" ? GENERATION_STAGES.slice(1) : GENERATION_STAGES;

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

/** Whether cancelling now throws the draft away: only a fresh run past the generating stage. */
export const cancelDiscardsDraft = () => {
  const current = status();
  return (
    !!current?.running &&
    current.kind === "generate" &&
    (current.stage === "checking" || current.stage === "resolving")
  );
};

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

/** Updates the mirrored status. Exported for tests; the app calls it from `watchGeneration`. */
export function applyGenerationStatus(next: GenerationStatus | null) {
  setStatusRaw(next);
}
const setStatus = applyGenerationStatus;

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
  // Listen first, then read: an event that arrives before the read answers is newer than it.
  let sawEvent = false;
  void refreshWaitingDrafts();
  // A finished generation adds a draft, and a finished blind check changes its verdicts. The
  // list only changes when a run starts or ends, not on every progress update.
  const listening = api.onGenerationStatus((next) => {
    sawEvent = true;
    const before = untrack(status);
    const changed =
      before?.running !== next.running || before?.outcome?.kind !== next.outcome?.kind;
    setStatus(next);
    if (changed) void refreshWaitingDrafts();
  });
  void listening
    .catch(() => undefined)
    .then(() => api.generationStatus())
    .then((initial) => {
      if (!sawEvent) setStatus(initial);
    })
    .catch(() => undefined);
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
