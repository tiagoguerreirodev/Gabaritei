import { createEffect, createSignal, on, onCleanup, type Accessor } from "solid-js";

const [attemptOpen, setAttemptOpen] = createSignal(false);
const [menuShown, setMenuShown] = createSignal(false);

/** Focus mode: while an unfinished attempt is on screen the sidebar is hidden, so the page reads
 * like an exam sheet, until the user brings the menu back for this attempt. */
export const focusMode = () => attemptOpen() && !menuShown();

export const toggleFocusMode = () => setMenuShown(!menuShown());

/** Turns focus mode on while `open()` holds and the calling component is mounted. Leaving the
 * attempt (finishing, abandoning or navigating away) also forgets that the menu was shown. */
export function useFocusMode(open: Accessor<boolean>) {
  const close = () => {
    setAttemptOpen(false);
    setMenuShown(false);
  };
  createEffect(on(open, (value) => (value ? setAttemptOpen(true) : close())));
  onCleanup(close);
}
