import { createSignal, onCleanup, type Accessor } from "solid-js";

/** Whether `query` currently matches. `false` where the browser has no `matchMedia`. */
export function createMediaQuery(query: string): Accessor<boolean> {
  if (typeof window.matchMedia !== "function") return () => false;
  const list = window.matchMedia(query);
  const [matches, setMatches] = createSignal(list.matches);
  const update = (event: MediaQueryListEvent) => setMatches(event.matches);
  list.addEventListener("change", update);
  onCleanup(() => list.removeEventListener("change", update));
  return matches;
}
