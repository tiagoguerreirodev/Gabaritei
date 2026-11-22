/**
 * Whether the Resolver's single-letter shortcuts (A–E, X, F) are on. The backend keeps the saved
 * choice for this computer; until it answers, they are on, like the default.
 */
import { createSignal } from "solid-js";
import { api } from "./api";

const [enabled, setEnabled] = createSignal(true);

/** The setting in effect. */
export const shortcutsEnabled = enabled;

/** Reads the saved choice. A failure keeps what is in effect: shortcuts are not essential. */
export function loadShortcuts(): Promise<void> {
  return api.keyboardShortcuts().then(
    (saved) => void setEnabled(saved),
    () => {},
  );
}

/** Saves a new choice and applies it. Rejects (and changes nothing) if saving fails. */
export async function saveShortcuts(next: boolean): Promise<void> {
  setEnabled(await api.saveKeyboardShortcuts(next));
}
