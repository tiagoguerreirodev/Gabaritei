/**
 * The state of the background generation, shared by every page: the Gerar form, the Revisar
 * pacote page and the banner in the layout. The backend is the source of truth; this mirrors
 * it from an initial read and the `generation-status` events.
 */
import { createSignal } from "solid-js";
import { api } from "./api";
import type { GenerationStatus } from "./bindings/GenerationStatus";

const [status, setStatus] = createSignal<GenerationStatus | null>(null);
let watching = false;

/** The latest known status (`null` until the first read). */
export const generationStatus = status;

/** Starts mirroring the backend. Safe to call more than once. */
export function watchGeneration() {
  if (watching) return;
  watching = true;
  void api
    .generationStatus()
    .then(setStatus)
    .catch(() => undefined);
  void api.onGenerationStatus(setStatus);
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
}
