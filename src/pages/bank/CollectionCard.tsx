import { Show } from "solid-js";
import type { CollectionSummary } from "../../bindings/CollectionSummary";
import { achievementBand, formatDate, formatPercent, questionCount } from "../../format";
import { openAttempt } from "../../navigation";

/**
 * One collection: its tags, size, last finished attempt and achievement, and one action
 * button. The title opens the details; the button continues the attempt in progress or asks
 * to start one.
 */
export default function CollectionCard(props: {
  collection: CollectionSummary;
  onDetails: () => void;
  onStart: () => void;
}) {
  const collection = () => props.collection;
  /** Wrapped so a 0% achievement still shows (`Show` hides falsy values). */
  const lastScore = () => {
    const achievement = collection().lastAchievement;
    return achievement === null ? undefined : { achievement };
  };
  return (
    <li class="collection-card">
      <div class="collection-main">
        <button type="button" class="collection-title" onClick={() => props.onDetails()}>
          {collection().title}
        </button>
        <span class="collection-tags">
          <span class="chip">{questionCount(collection().questions)}</span>
          <Show when={collection().contest}>
            <span class="badge badge-accent">{collection().contest}</span>
          </Show>
          <Show when={collection().board}>
            <span class="badge badge-accent">{collection().board}</span>
          </Show>
        </span>
        <span class="collection-meta">
          <Show when={collection().lastFinishedAt} fallback={<span>Nunca resolvido</span>}>
            {(finishedAt) => <span>Última tentativa em {formatDate(finishedAt())}</span>}
          </Show>
          <Show when={lastScore()}>
            {(score) => (
              <span class="score-chip" data-band={achievementBand(score().achievement)}>
                {formatPercent(score().achievement)} de aproveitamento
              </span>
            )}
          </Show>
          <Show when={collection().activeAttemptId}>
            <span class="chip">Em andamento</span>
          </Show>
        </span>
      </div>
      <Show
        when={collection().activeAttemptId}
        fallback={
          <button
            type="button"
            class="btn btn-primary"
            aria-label={`Resolver ${collection().title}`}
            disabled={collection().questions === 0}
            onClick={() => props.onStart()}
          >
            Resolver
          </button>
        }
      >
        {(attemptId) => (
          <button
            type="button"
            class="btn btn-primary"
            aria-label={`Continuar ${collection().title}`}
            onClick={() => openAttempt(attemptId())}
          >
            Continuar
          </button>
        )}
      </Show>
    </li>
  );
}
