import { Show } from "solid-js";
import type { LearningItem } from "../../bindings/LearningItem";
import type { ScheduledReview } from "../../bindings/ScheduledReview";
import { formatDate, formatPercent } from "../../format";
import { openAttempt } from "../../navigation";

const GOOD_ACCURACY = 0.75;
const FAIR_ACCURACY = 0.5;

type AccuracyBand = "good" | "fair" | "poor";

const accuracyBand = (accuracy: number): AccuracyBand =>
  accuracy >= GOOD_ACCURACY ? "good" : accuracy >= FAIR_ACCURACY ? "fair" : "poor";

const topicName = (item: LearningItem) => item.topics[0]?.topic ?? "Sem assunto";

const matterName = (item: LearningItem) => {
  const [first] = item.topics;
  return first ? `${first.area} › ${first.matter}` : "";
};

const accessibleTopic = (item: LearningItem) =>
  item.topics.length === 0
    ? "Sem assunto"
    : item.topics.map((t) => `${t.area} › ${t.matter} › ${t.topic}`).join("; ");

const intervalText = (days: number) => `revisão de ${days} ${days === 1 ? "dia" : "dias"}`;

/** "Vence hoje", "Venceu em 12/11/2026" or "Próxima revisão em 25/11/2026". */
function dueText(review: ScheduledReview | null) {
  if (!review) return "Sem revisão agendada";
  switch (review.state) {
    case "today":
      return "Vence hoje";
    case "upcoming":
      return `Próxima revisão em ${formatDate(review.at)}`;
    case "overdue":
      return `Venceu em ${formatDate(review.at)}`;
  }
}

/**
 * One review or error: topic, schedule chip, last answer and accuracy, and one action. It
 * carries no stem, key or explanation; the action opens the past attempt.
 */
export default function LearningRow(props: { item: LearningItem }) {
  return (
    <li>
      <span class="learning-topic">{topicName(props.item)}</span>
      <span class="muted">
        {matterName(props.item)}
        <Show when={props.item.review}>
          {(review) => (
            <span class="badge learning-chip">{intervalText(review().intervalDays)}</span>
          )}
        </Show>
        <Show when={props.item.isError}>
          <span class="badge">Erro</span>
        </Show>
      </span>
      <span class="muted learning-meta">
        {dueText(props.item.review)} · Última resposta em {formatDate(props.item.lastAnsweredAt)}
        <Show when={props.item.accuracy !== null}>
          <span class={`badge badge-${accuracyBand(props.item.accuracy ?? 0)}`}>
            {formatPercent(props.item.accuracy)} de acerto
          </span>
        </Show>
      </span>
      <button
        type="button"
        class="btn"
        aria-label={`Ver resultado: ${accessibleTopic(props.item)}`}
        onClick={() => openAttempt(props.item.lastAttemptId)}
      >
        Ver resultado
      </button>
    </li>
  );
}
