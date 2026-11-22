import { A } from "@solidjs/router";
import { For, Match, Show, Switch, createResource } from "solid-js";
import { api } from "../api";
import LegacyImport from "../components/LegacyImport";
import MasteryBar from "../components/MasteryBar";
import { EmptyState, ErrorState, Loading } from "../components/states";
import { formatDate, formatNumber, questionCount } from "./bank/format";
import StatCards from "./performance/StatCards";
import { openAttempt } from "./solve/format";

const modeLabel = { study: "Estudo", exam: "Simulado" } as const;

/** Início: summary cards, attempts to resume, due reviews and drafts waiting for review. */
export default function Home() {
  const [status] = createResource(() => api.legacyImportStatus());
  const [home, { refetch: refetchHome }] = createResource(() => api.home());

  return (
    <section class="page">
      <h1>Início</h1>

      {/* First run: offer the legacy import while the bank is empty and it hasn't run. */}
      <Show when={status()?.bankEmpty && !status()?.report}>
        <LegacyImport initialReport={null} onImported={() => void refetchHome()} />
      </Show>

      <Switch>
        <Match when={home.error as unknown}>
          {(error) => <ErrorState error={error()} onRetry={() => void refetchHome()} />}
        </Match>
        <Match when={home()}>
          {(summary) => (
            <>
              <p class="muted">{questionCount(summary().questions)} no banco.</p>
              <Show
                when={summary().overall.score.evaluated > 0}
                fallback={
                  <EmptyState
                    title="Você ainda não finalizou nenhuma tentativa."
                    action={
                      <A class="primary button-link" href="/banco">
                        Ir para o Banco
                      </A>
                    }
                  >
                    <p class="muted">Comece um caderno ou um assunto no Banco.</p>
                  </EmptyState>
                }
              >
                <StatCards stats={summary().overall} label="Resumo" />
              </Show>

              <section class="card wide" aria-labelledby="active-title">
                <h2 id="active-title">Tentativas para retomar</h2>
                <Show
                  when={summary().activeAttempts.length > 0}
                  fallback={<p class="muted">Nenhuma tentativa em andamento.</p>}
                >
                  <ul class="attempt-list">
                    <For each={summary().activeAttempts}>
                      {(attempt) => (
                        <li>
                          <span class="attempt-title">{attempt.title}</span>
                          <span class="muted">
                            {modeLabel[attempt.feedbackMode]} · {formatNumber(attempt.answered)} de{" "}
                            {formatNumber(attempt.total)} respondidas · iniciada em{" "}
                            {formatDate(attempt.startedAt)}
                          </span>
                          <button
                            type="button"
                            class="primary"
                            aria-label={`Retomar ${attempt.title}`}
                            onClick={() => openAttempt(attempt.id)}
                          >
                            Retomar
                          </button>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </section>

              <section class="card wide" aria-labelledby="waiting-title">
                <h2 id="waiting-title">Aguardando você</h2>
                <ul class="waiting-list">
                  <li>
                    <A href="/erros-e-revisoes">Revisões pendentes</A>
                    <strong>{formatNumber(summary().dueReviews)}</strong>
                    <Show when={summary().dueInOpenAttempt > 0}>
                      <span class="muted">
                        {" "}
                        · {formatNumber(summary().dueInOpenAttempt)} em tentativa aberta
                      </span>
                    </Show>
                  </li>
                  <li>
                    <A href="/revisar-pacote">Pacotes para revisar</A>
                    <strong>{formatNumber(summary().draftsWaiting)}</strong>
                  </li>
                </ul>
              </section>
              <Show when={summary().overall.score.evaluated > 0}>
                <section class="card wide" aria-labelledby="areas-title">
                  <h2 id="areas-title">Aproveitamento por área</h2>
                  <ul class="area-list">
                    <For each={summary().areas.filter((a) => a.stats.score.evaluated > 0)}>
                      {(area) => (
                        <li>
                          <MasteryBar name={area.name} achievement={area.stats.score.achievement} />
                        </li>
                      )}
                    </For>
                  </ul>
                  <A href="/desempenho">Ver desempenho completo</A>
                </section>
              </Show>
            </>
          )}
        </Match>
        <Match when={home.loading}>
          <Loading variant="skeleton" />
        </Match>
      </Switch>
    </section>
  );
}
