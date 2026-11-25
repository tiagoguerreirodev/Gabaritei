import type { ResponseView } from "./bindings/ResponseView";

/** pt-BR number, e.g. 9.368. */
export const formatNumber = (n: number) => n.toLocaleString("pt-BR");

/** pt-BR date, e.g. 19/11/2026. */
export const formatDate = (iso: string) => new Date(iso).toLocaleDateString("pt-BR");

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
  ratio === null
    ? "—"
    : ratio.toLocaleString("pt-BR", { style: "percent", maximumFractionDigits: 1 });
