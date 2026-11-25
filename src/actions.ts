import { createSignal } from "solid-js";
import { showError } from "./components/toasts";

export interface ActionRunner {
  /** Whether any action is running now. */
  active: () => boolean;
  /**
   * Runs one action at a time, shows any error, and returns whether the action succeeded.
   */
  run: (work: () => Promise<void>, afterError?: () => void) => Promise<boolean>;
}

export function createAction(): ActionRunner {
  const [active, setActive] = createSignal(false);
  return {
    active,
    run: async (work, afterError) => {
      if (active()) return false;
      setActive(true);
      try {
        await work();
        return true;
      } catch (error) {
        showError(error);
        afterError?.();
        return false;
      } finally {
        setActive(false);
      }
    },
  };
}
