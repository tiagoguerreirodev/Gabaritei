/**
 * The state of the background generation, shared by every page: the Gerar form, the Revisar
 * pacote page and the banner in the layout. The backend is the source of truth; this mirrors
 * it from an initial read and the `generation-status` events.
 */
import { createSignal } from "solid-js";
import { api } from "./api";
import type { DraftListItem } from "./bindings/DraftListItem";
import type { GenerationStatus } from "./bindings/GenerationStatus";

const [status, setStatus] = createSignal<GenerationStatus | null>(null);
const [waiting, setWaiting] = createSignal<DraftListItem[]>([]);
let watching = false;

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
  // A finished generation adds a draft, and a finished blind check changes its verdicts.
  void api.onGenerationStatus((next) => {
    setStatus(next);
    void refreshWaitingDrafts();
  });
}

/** Forgets how the last generation ended, here and in the backend. */
export async function dismissGeneration() {
  setStatus((current) => (current ? { ...current, outcome: null } : current));
  await api.generationDismiss().catch(() => undefined);
}

/** For tests: forget everything and allow `watchGeneration` to run again. */
export function resetGenerationWatch() {
  watching = false;
  setStatus(null);
  setWaiting([]);
}
