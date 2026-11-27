import { emptyFilter } from "./filterUrl";
import type { BancoState } from "./filterUrl";

/** Shortcuts to a filter and order, shared by Banco and Início. */
export const presets = {
  reviews: { filter: { ...emptyFilter, statuses: ["due", "wrong"] }, order: "dueFirst" },
  errors: { filter: { ...emptyFilter, statuses: ["wrong"] }, order: "recentErrorsFirst" },
} satisfies Record<string, Partial<BancoState>>;
