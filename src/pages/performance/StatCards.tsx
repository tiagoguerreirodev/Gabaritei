import { For } from "solid-js";
import type { GroupStats } from "../../bindings/GroupStats";
import { formatNumber, formatPercent } from "../../format";

/**
 * Headline numbers of a group of questions. Achievement, the main measure, leads in a larger
 * card with a bar; accuracy, coverage and answers follow as supporting numbers.
 */
export default function StatCards(props: { stats: GroupStats; label: string }) {
  const achievement = () => props.stats.score.achievement;
  const cards = () => [
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
      <li class="card card-compact stat stat-card-main">
        <span class="stat-name">Aproveitamento</span>
        <strong class="stat-value">{formatPercent(achievement())}</strong>
        <span class="meter-track mastery-bar" aria-hidden="true">
          <span
            class="meter-fill"
            style={{ width: `${Math.round((achievement() ?? 0) * 100)}%` }}
          />
        </span>
        <span class="stat-hint muted">Acertos ÷ questões avaliadas</span>
      </li>
      <For each={cards()}>
        {(card) => (
          <li class="card card-compact stat">
            <span class="stat-name">{card.name}</span>
            <strong class="stat-value">{card.value}</strong>
            <span class="stat-hint muted">{card.hint}</span>
          </li>
        )}
      </For>
    </ul>
  );
}
