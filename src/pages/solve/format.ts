import type { Outcome } from "../../bindings/Outcome";
import type { ResponseView } from "../../bindings/ResponseView";

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

/** What the reveal says about the user's answer; also what the live region announces. */
export const outcomeText: Record<Outcome, string> = {
  correct: "Você acertou.",
  wrong: "Você errou.",
  blank: "Questão em branco.",
  annulled: "Questão anulada: não entra na pontuação.",
};

/** Opens an attempt in the Resolver (works outside the router, e.g. from modals). */
export function openAttempt(id: string) {
  window.location.hash = `#/resolver?tentativa=${encodeURIComponent(id)}`;
}
