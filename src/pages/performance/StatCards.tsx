import { For } from "solid-js";
import type { GroupStats } from "../../bindings/GroupStats";
import { formatNumber } from "../bank/format";
import { formatPercent } from "../solve/format";

/** Headline numbers of a group of questions: achievement, accuracy, coverage and answers. */
export default function StatCards(props: { stats: GroupStats; label: string }) {
  const cards = () => [
    {
      name: "Aproveitamento",
      value: formatPercent(props.stats.score.achievement),
      hint: "Acertos ÷ questões avaliadas",
    },
    {
      name: "Acerto entre respondidas",
      value: formatPercent(props.stats.score.accuracy),
      hint: "Acertos ÷ questões respondidas",
    },
    {
      name: "Cobertura",
      value: formatPercent(props.stats.coverage),
      hint: `${formatNumber(props.stats.answered)} de ${formatNumber(props.stats.available)} questões`,
    },
    {
      name: "Avaliadas",
      value: formatNumber(props.stats.score.evaluated),
      hint: `${formatNumber(props.stats.score.annulled)} anuladas fora da conta`,
    },
  ];

  return (
    <ul class="stat-cards" aria-label={props.label}>
      <For each={cards()}>
        {(card) => (
          <li class="stat-card">
            <span class="stat-name">{card.name}</span>
            <strong class="stat-value">{card.value}</strong>
            <span class="stat-hint muted">{card.hint}</span>
          </li>
        )}
      </For>
    </ul>
  );
}
