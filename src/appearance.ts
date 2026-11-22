/**
 * Colour theme and text size: applied as attributes on `<html>`, which `src/styles/tokens.css`
 * reads. The backend keeps the saved choice; a copy in localStorage only avoids a flash of the
 * wrong theme at startup, so every use of it is optional.
 */
import { createSignal } from "solid-js";
import { api } from "./api";
import type { Appearance } from "./bindings/Appearance";

const CACHE_KEY = "gabaritei.appearance";
const THEMES: Appearance["theme"][] = ["system", "light", "dark"];
const TEXT_SIZES: Appearance["textSize"][] = ["s", "m", "l", "xl"];

export const DEFAULT_APPEARANCE: Appearance = { theme: "system", textSize: "m" };

const [current, setCurrent] = createSignal<Appearance>(DEFAULT_APPEARANCE);

/** The appearance in effect. */
export const appearance = current;

/** Sets `data-theme` (absent for "system") and `data-text-size` on the root element. */
export function applyAppearance(value: Appearance, root: HTMLElement = document.documentElement) {
  if (value.theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", value.theme);
  root.setAttribute("data-text-size", value.textSize);
  setCurrent(value);
}

function remember(value: Appearance) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(value));
  } catch {
    // Storage can be blocked; the backend copy is the one that counts.
  }
}

function recall(): Appearance | null {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null");
    if (typeof raw !== "object" || raw === null) return null;
    const { theme, textSize } = raw as Partial<Appearance>;
    if (theme && textSize && THEMES.includes(theme) && TEXT_SIZES.includes(textSize)) {
      return { theme, textSize };
    }
  } catch {
    // Missing, blocked or corrupt: start from the defaults.
  }
  return null;
}

/** Applies the cached choice at once, then the saved one from the backend. */
export function startAppearance(): Promise<void> {
  applyAppearance(recall() ?? DEFAULT_APPEARANCE);
  return api
    .appearance()
    .then((saved) => {
      applyAppearance(saved);
      remember(saved);
    })
    .catch(() => {
      // Keep what is applied; the app is usable with the defaults.
    });
}

/** Saves a new choice and applies it. Rejects (and changes nothing) if saving fails. */
export async function saveAppearance(next: Appearance): Promise<void> {
  const saved = await api.saveAppearance(next);
  applyAppearance(saved);
  remember(saved);
}
