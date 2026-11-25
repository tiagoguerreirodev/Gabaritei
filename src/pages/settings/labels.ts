import type { Agent } from "../../bindings/Agent";
import type { Effort } from "../../bindings/Effort";
import type { Role } from "../../bindings/Role";

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
export const roleOrder: { role: Role; key: "generate" | "blindCheck" | "recheck" | "explain" }[] = [
  { role: "generate", key: "generate" },
  { role: "blind_check", key: "blindCheck" },
  { role: "recheck", key: "recheck" },
  { role: "explain", key: "explain" },
];
