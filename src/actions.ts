import { createSignal } from "solid-js";

/**
 * One "something is running" flag for the whole Dados section, shared by the backup and the
 * data-file sections: a restore, an import and an export must never overlap, or the second
 * would replace what the first just did.
 */
export interface Busy {
  /** Whether any action is running now. */
  active: () => boolean;
  /**
   * Runs `work` unless another action is running (then it does nothing and returns `false`).
   * The flag is cleared when `work` ends, however it ends.
   */
  run: (work: () => Promise<void>) => Promise<boolean>;
}

export function createBusy(): Busy {
  const [active, setActive] = createSignal(false);
  return {
    active,
    run: async (work) => {
      if (active()) return false;
      setActive(true);
      try {
        await work();
      } finally {
        setActive(false);
      }
      return true;
    },
  };
}
