import { A } from "@solidjs/router";
import { For, Show, createEffect, createResource, createSignal, onCleanup } from "solid-js";
import { api } from "../api";
import type { LearningItem } from "../bindings/LearningItem";
import type { ScheduledReview } from "../bindings/ScheduledReview";
import type { SessionSource } from "../bindings/SessionSource";
import { EmptyState, Loading, ResourceView } from "../components/states";
import { formatDate, formatNumber } from "../format";
import StartSessionModal from "./solve/StartSessionModal";
import { openAttempt } from "../navigation";

const topicLabel = (item: LearningItem) =>
  item.topics.length === 0
    ? "Sem assunto"
    : item.topics.map((t) => `${t.area} › ${t.matter} › ${t.topic}`).join("; ");

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
 * Erros e revisões: what is due, what is coming and the error notebook, with a way to
 * practice. Rows show no answer keys; they link to the past attempt, where the question has
 * already been revealed.
 */
export default function Learning() {
  const [includeBlanks, setIncludeBlanks] = createSignal(false);
  // An object source: Solid doesn't fetch while a source is `false`.
  const [overview, { refetch }] = createResource(
    () => ({ blanks: includeBlanks() }),
    ({ blanks }) => api.learningOverview(blanks),
  );
  const [practice, setPractice] = createSignal<{ title: string; source: SessionSource } | null>(
    null,
  );
  createEffect(() => {
    if (overview.error || overview.loading) return;
    const at = overview()?.refreshAt;
    if (!at) return;
    const timer = setTimeout(() => void refetch(), Math.max(0, Date.parse(at) - Date.now()));
    onCleanup(() => clearTimeout(timer));
  });

  return (
    <section class="page">
      <h1>Erros e revisões</h1>

      <ResourceView
        value={overview()}
        error={() => overview.error}
        pending={overview.state === "pending"}
        retrying={overview.loading}
        onRetry={() => refetch()}
        placeholder={<Loading variant="skeleton" />}
      >
        {(data) => (
          <>
            <ul class="stat-cards" aria-label="Resumo">
              <li class="stat-card">
                <span class="stat-name">Para revisar agora</span>
                <strong class="stat-value">{formatNumber(data().due)}</strong>
                <span class="stat-hint muted">
                  Revisões vencidas
                  <Show when={data().dueInOpenAttempt > 0}>
                    {" "}
                    · {formatNumber(data().dueInOpenAttempt)} em tentativa aberta
                  </Show>
                </span>
              </li>
              <li class="stat-card">
                <span class="stat-name">Próximas</span>
                <strong class="stat-value">{formatNumber(data().upcoming)}</strong>
                <span class="stat-hint muted">Revisões agendadas</span>
              </li>
              <li class="stat-card">
                <span class="stat-name">Caderno de erros</span>
                <strong class="stat-value">{formatNumber(data().errors)}</strong>
                <span class="stat-hint muted">Última resposta errada</span>
              </li>
            </ul>

            <div class="learning-actions">
              <button
                type="button"
                class="primary"
                disabled={data().practice === 0}
                onClick={() =>
                  setPractice({ title: "Praticar revisões", source: { kind: "reviews" } })
                }
              >
                Praticar agora ({formatNumber(data().practice)})
              </button>
              <button
                type="button"
                disabled={data().practiceErrors === 0}
                onClick={() =>
                  setPractice({
                    title: "Praticar erros",
                    source: { kind: "errors", includeBlanks: includeBlanks() },
                  })
                }
              >
                Praticar erros ({formatNumber(data().practiceErrors)})
              </button>
              <label class="learning-blanks">
                <input
                  type="checkbox"
                  checked={includeBlanks()}
                  onChange={(event) => setIncludeBlanks(event.currentTarget.checked)}
                />
                Incluir questões em branco
              </label>
            </div>

            <Show
              when={data().items.length > 0}
              fallback={
                <EmptyState
                  title="Nada para revisar por enquanto."
                  action={
                    <A class="button-link primary" href="/banco">
                      Ir para o Banco
                    </A>
                  }
                >
                  <p class="muted">
                    Finalize uma tentativa: as questões entram aqui conforme você responde.
                  </p>
                </EmptyState>
              }
            >
              <ul class="learning-list" aria-label="Questões">
                <For each={data().items}>
                  {(item) => (
                    <li>
                      <span class="learning-topic">{topicLabel(item)}</span>
                      <span class="muted">
                        {dueText(item.review)}
                        <Show when={item.isError}>
                          {" "}
                          · <span class="badge">Erro</span>
                        </Show>
                      </span>
                      <button
                        type="button"
                        aria-label={`Ver resultado: ${topicLabel(item)}`}
                        onClick={() => openAttempt(item.lastAttemptId)}
                      >
                        Ver resultado
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </>
        )}
      </ResourceView>

      <Show when={practice()}>
        {(session) => (
          <StartSessionModal
            title={session().title}
            source={session().source}
            onClose={() => setPractice(null)}
          />
        )}
      </Show>
    </section>
  );
}
