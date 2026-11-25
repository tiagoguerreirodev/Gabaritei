import type { Agent } from "./bindings/Agent";
import type { Effort } from "./bindings/Effort";
import type { FeedbackMode } from "./bindings/FeedbackMode";
import type { Outcome } from "./bindings/Outcome";
import type { Role } from "./bindings/Role";

export const agentLabels: Record<Agent, string> = {
  claude: "Claude Code",
  codex: "Codex",
};

export const effortLabels: Record<Effort, string> = {
  low: "Baixo",
  medium: "Médio",
  high: "Alto",
  xhigh: "Extra alto",
  max: "Máximo",
  ultra: "Ultra",
};

export const roleLabels: Record<Role, { name: string; hint: string }> = {
  generate: { name: "Gerar questões", hint: "Escreve o pacote de questões." },
  blind_check: {
    name: "Conferência às cegas",
    hint: "Responde as questões sem ver o gabarito.",
  },
  recheck: { name: "Revisão sob demanda", hint: "Reconfere um gabarito que você contestou." },
  explain: { name: "Explicação sob demanda", hint: "Escreve a explicação que faltava." },
};

export const agents: Agent[] = ["codex", "claude"];
export const efforts: Effort[] = ["low", "medium", "high", "xhigh", "max", "ultra"];
export const roleOrder: { role: Role; key: "generate" | "blindCheck" | "recheck" | "explain" }[] = [
  { role: "generate", key: "generate" },
  { role: "blind_check", key: "blindCheck" },
  { role: "recheck", key: "recheck" },
  { role: "explain", key: "explain" },
];

export const feedbackModes: FeedbackMode[] = ["study", "exam"];

export const modeLabel: Record<FeedbackMode, string> = {
  study: "Estudo",
  exam: "Simulado",
};

export const modeHint: Record<FeedbackMode, string> = {
  study: "Mostra o gabarito e a explicação logo após cada resposta.",
  exam: "Mostra o gabarito e as explicações só ao finalizar.",
};

const outcomeLabels: Record<Outcome, { singular: string; plural: string }> = {
  correct: { singular: "certa", plural: "certas" },
  wrong: { singular: "errada", plural: "erradas" },
  blank: { singular: "em branco", plural: "em branco" },
  annulled: { singular: "anulada", plural: "anuladas" },
};

export const outcomeLabel = (outcome: Outcome, count: number) =>
  count === 1 ? outcomeLabels[outcome].singular : outcomeLabels[outcome].plural;

/** What the reveal says about the user's answer; also what the live region announces. */
export const outcomeText: Record<Outcome, string> = {
  correct: "Você acertou.",
  wrong: "Você errou.",
  blank: "Questão em branco.",
  annulled: "Questão anulada: não entra na pontuação.",
};
