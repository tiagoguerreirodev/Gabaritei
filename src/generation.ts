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
 * The stages the backend reports, in order. The backend only exposes a stage through its fixed
 * pt-BR progress labels, so these are all that can be shown: saving is not reported.
 */
export const GENERATION_STAGES = [
  { id: "generating", label: "Gerando as questões" },
  { id: "checking", label: "Conferindo às cegas" },
  { id: "fixing", label: "Resolvendo as contestações" },
] as const;

/**
 * Which stage a progress label starts, or `null` for any other label (a label inside the current
 * stage, or one this list does not know: it never changes the stage). The labels come from
 * `src-tauri/src/ai/blind_check.rs`: "Gerando as questões…", "Conferindo às cegas…",
 * "Resolvendo as contestações…" and "Reconferindo as questões revisadas…". A test reads that
 * file so a renamed label fails here.
 */
export function stageOfLabel(label: string): number | null {
  if (label.startsWith("Gerando as questões")) return 0;
  if (label.startsWith("Conferindo às cegas")) return 1;
  if (label.startsWith("Resolvendo as contestações") || label.startsWith("Reconferindo")) {
    return 2;
  }
  return null;
}

/** The highest stage (index into `GENERATION_STAGES`) seen in this run, `null` if none yet. */
const [seen, setSeen] = createSignal<number | null>(null);
/**
 * A re-check of a saved draft never says "Gerando as questões…": its first stage label is the
 * check. Then generating is left out of the list.
 */
const [recheck, setRecheck] = createSignal(false);

/** The stages that apply to the running job: two for a re-check, three for a generation. */
export const generationStages = () => (recheck() ? GENERATION_STAGES.slice(1) : GENERATION_STAGES);

/**
 * Index into `generationStages()` of the stage the running job is in, or `null` when no stage
 * label was seen (for example the page was reloaded in the middle of a run). Never goes back.
 */
export const generationStage = (): number | null => {
  const current = seen();
  return current === null ? null : current - (recheck() ? 1 : 0);
};

/** "Etapa 2 de 3: Conferindo às cegas", or "" when the stage is not known. */
export function generationStageSentence(): string {
  const index = generationStage();
  if (index === null) return "";
  return `Etapa ${index + 1} de ${generationStages().length}: ${generationStages()[index]?.label}`;
}

/** Whether cancelling now throws the draft away: only a fresh run past the generating stage. */
export const cancelDiscardsDraft = () => !recheck() && (seen() ?? 0) >= 1;

/** Set by the progress panel on the Gerar page while it shows, so the banner can stay quiet. */
const [progressPanel, setProgressPanel] = createSignal(false);
export const generationPanelShown = progressPanel;
export const setGenerationPanelShown = setProgressPanel;

/** Updates the mirrored status. Exported for tests; the app calls it from `watchGeneration`. */
export function applyGenerationStatus(next: GenerationStatus | null) {
  if (!next?.running) {
    setSeen(null);
    setRecheck(false);
  } else {
    const started = next.progress ? stageOfLabel(next.progress.label) : null;
    if (started !== null) {
      if (seen() === null) setRecheck(started === 1);
      setSeen((current) => Math.max(current ?? 0, started));
    }
  }
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
