import { For, Show, createSignal } from "solid-js";
import { errorMessage } from "./states";

export type ToastTone = "info" | "success" | "error";

interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
}

const [toasts, setToasts] = createSignal<Toast[]>([]);
let nextId = 0;

/** How long a toast stays on screen, in milliseconds. */
export const TOAST_DURATION = 6000;

export function dismissToast(id: number) {
  setToasts((list) => list.filter((toast) => toast.id !== id));
}

/** Shows a short pt-BR message in the corner of the window. */
export function showToast(message: string, tone: ToastTone = "info") {
  const id = nextId++;
  setToasts((list) => [...list, { id, message, tone }]);
  setTimeout(() => dismissToast(id), TOAST_DURATION);
  return id;
}

/** Confirms that something the person asked for (save, import, backup) is done. */
export function showSuccess(message: string) {
  return showToast(message, "success");
}

/** Shows an error from a failed command (or anything thrown) as a toast. */
export function showError(error: unknown) {
  return showToast(errorMessage(error), "error");
}

/** Renders the active toasts. Mounted once, in the layout. */
export function Toaster() {
  return (
    <div class="toasts">
      <For each={toasts()}>
        {(toast) => (
          <div
            class={`toast toast-${toast.tone}`}
            role={toast.tone === "error" ? "alert" : "status"}
          >
            <Show when={toast.tone === "success"}>
              <span class="toast-glyph" aria-hidden="true">
                ✓
              </span>
            </Show>
            <span>{toast.message}</span>
            <button
              type="button"
              class="icon-button"
              aria-label="Dispensar aviso"
              onClick={() => dismissToast(toast.id)}
            >
              ×
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
