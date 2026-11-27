import type { ResponseView } from "./bindings/ResponseView";

/** pt-BR number, e.g. 9.368. */
export const formatNumber = (n: number, options?: Intl.NumberFormatOptions) =>
  n.toLocaleString("pt-BR", options);

/** pt-BR date, e.g. 19/11/2026. */
export const formatDate = (iso: string) => new Date(iso).toLocaleDateString("pt-BR");

export const formatDateTime = (iso: string) =>
  new Date(iso).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });

export const formatMegabytes = (bytes: number) =>
  `${formatNumber(bytes / (1024 * 1024), { maximumFractionDigits: 1 })} MB`;

export const formatDuration = (milliseconds: number) =>
  `${formatNumber(Math.max(1, Math.round(milliseconds / 1000)))} s`;

/** "1 questão" / "9.368 questões". */
export const questionCount = (n: number) =>
  `${formatNumber(n)} ${n === 1 ? "questão" : "questões"}`;

/** Keyboard letters for a question's options: A–E, or C/E for Certo/Errado. */
export const optionLetters = (question: Pick<ResponseView, "type" | "options">) =>
  question.type === "boolean"
    ? ["C", "E"]
    : ["A", "B", "C", "D", "E"].slice(0, question.options.length);

/** pt-BR percentage, e.g. 66,7%. `—` when there is nothing to divide. */
export const formatPercent = (ratio: number | null) =>
  ratio === null ? "—" : formatNumber(ratio, { style: "percent", maximumFractionDigits: 1 });

/** Achievement at or above this reads as solid. */
export const STRONG_ACHIEVEMENT = 0.7;
/** Achievement below this reads as weak. */
export const WEAK_ACHIEVEMENT = 0.5;

/** The color band of an achievement badge. The printed percentage always goes with it. */
export const achievementBand = (ratio: number) => {
  if (ratio >= STRONG_ACHIEVEMENT) return "strong";
  if (ratio >= WEAK_ACHIEVEMENT) return "fair";
  return "weak";
};
