import { For, Match, Show, Switch, createResource, createSignal } from "solid-js";
import { api } from "../api";
import type { LearningItem } from "../bindings/LearningItem";
import type { SessionSource } from "../bindings/SessionSource";
import { EmptyState, ErrorState, Loading } from "../components/states";
import { formatDate, formatNumber } from "./bank/format";
import StartSessionModal from "./solve/StartSessionModal";
import { openAttempt } from "./solve/format";

/** Largest practice session the backend builds from reviews or errors. */
const MAX_SESSION = 100;

const topicLabel = (item: LearningItem) =>
  item.topics.length === 0
    ? "Sem assunto"
    : item.topics.map((t) => `${t.area} › ${t.matter} › ${t.topic}`).join("; ");

/** "Vence hoje", "Venceu em 12/11/2026" or "Próxima revisão em 25/11/2026". */
function dueText(dueAt: string, now: number) {
  const due = new Date(dueAt);
  if (due.toDateString() === new Date(now).toDateString()) return "Vence hoje";
  return due.getTime() > now
    ? `Próxima revisão em ${formatDate(dueAt)}`
    : `Venceu em ${formatDate(dueAt)}`;
}

/**
 * Erros e revisões: what is due, what is coming and the error notebook, with a way to
 * practice. Rows show no answer keys; they link to the past attempt, where the question has
 * already been revealed.
 */
export default function Learning() {
  const [includeBlanks, setIncludeBlanks] = createSignal(false);
  // An object source: Solid doesn't fetch while a source is `false`.
  const [overview] = createResource(
    () => ({ blanks: includeBlanks() }),
    ({ blanks }) => api.learningOverview(blanks),
  );
  const [practice, setPractice] = createSignal<{ title: string; source: SessionSource } | null>(
    null,
  );
  const now = () => Date.now();

  return (
    <section class="page">
      <h1>Erros e revisões</h1>

      <Switch>
        <Match when={overview.error as unknown}>{(error) => <ErrorState error={error()} />}</Match>
        <Match when={overview()}>
          {(data) => (
            <>
              <ul class="stat-cards" aria-label="Resumo">
                <li class="stat-card">
                  <span class="stat-name">Para revisar agora</span>
                  <strong class="stat-value">{formatNumber(data().due)}</strong>
                  <span class="stat-hint muted">Revisões vencidas</span>
                </li>
                <li class="stat-card">
                  <span class="stat-name">Próximas</span>
                  <strong class="stat-value">{formatNumber(data().upcoming)}</strong>
                  <span class="stat-hint muted">Revisões agendadas</span>
                </li>
                <li class="stat-card">
                  <span class="stat-name">Caderno de erros</span>
                  <strong class="stat-value">{formatNumber(data().errors)}</strong>
                  <span class="stat-hint muted">Último resultado errado</span>
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
                  disabled={data().errors === 0}
                  onClick={() =>
                    setPractice({
                      title: "Praticar erros",
                      source: { kind: "errors", includeBlanks: includeBlanks() },
                    })
                  }
                >
                  Praticar erros ({formatNumber(Math.min(data().errors, MAX_SESSION))})
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
                  <EmptyState title="Nada para revisar por enquanto.">
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
                          {item.dueAt ? dueText(item.dueAt, now()) : "Sem revisão agendada"}
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
        </Match>
        <Match when={overview.loading}>
          <Loading />
        </Match>
      </Switch>

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
