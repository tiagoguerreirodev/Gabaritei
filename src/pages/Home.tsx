import { A } from "@solidjs/router";
import { For, Show, createResource } from "solid-js";
import { api } from "../api";
import type { GroupStats } from "../bindings/GroupStats";
import LegacyImport from "../components/LegacyImport";
import MasteryBar from "../components/MasteryBar";
import { Loading, ResourceView } from "../components/states";
import { formatDate, formatNumber, formatPercent, questionCount } from "../format";
import { modeLabel } from "../labels";
import { openAttempt } from "../navigation";
import FirstSteps from "./home/FirstSteps";
import TodayCard from "./home/TodayCard";
import { todayAction } from "./home/todayAction";

/**
 * Início, the day's dashboard: the first-use checklist until it's complete, one main action, a compact strip of numbers, then the
 * other open attempts, what waits for the user and achievement per area.
 */
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

      <ResourceView
        value={home()}
        error={() => home.error}
        pending={home.state === "pending"}
        retrying={home.loading}
        onRetry={() => refetchHome()}
        placeholder={<Loading />}
      >
        {(summary) => {
          const evaluated = () => summary().overall.score.evaluated > 0;
          const otherAttempts = () => summary().activeAttempts.slice(1);
          return (
            <>
              <FirstSteps steps={summary().firstSteps} />
              <TodayCard action={todayAction(summary())} />

              <p class="muted">{questionCount(summary().questions)} no banco.</p>
              <Show when={evaluated()}>
                <SummaryStrip stats={summary().overall} />
              </Show>

              <Show when={otherAttempts().length > 0}>
                <section class="card wide" aria-labelledby="active-title">
                  <h2 id="active-title">Outras tentativas em andamento</h2>
                  <ul class="attempt-list">
                    <For each={otherAttempts()}>
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
                            class="secondary"
                            aria-label={`Retomar ${attempt.title}`}
                            onClick={() => openAttempt(attempt.id)}
                          >
                            Retomar
                          </button>
                        </li>
                      )}
                    </For>
                  </ul>
                </section>
              </Show>

              <Show when={summary().dueReviews > 0 || summary().draftsWaiting > 0}>
                <section class="card wide" aria-labelledby="waiting-title">
                  <h2 id="waiting-title">Aguardando você</h2>
                  <ul class="waiting-list">
                    <Show when={summary().dueReviews > 0}>
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
                    </Show>
                    <Show when={summary().draftsWaiting > 0}>
                      <li>
                        <A href="/revisar-pacote">Pacotes para revisar</A>
                        <strong>{formatNumber(summary().draftsWaiting)}</strong>
                      </li>
                    </Show>
                  </ul>
                </section>
              </Show>

              <Show when={evaluated()}>
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
          );
        }}
      </ResourceView>
    </section>
  );
}

/** The headline numbers in one compact row, below the main action. */
function SummaryStrip(props: { stats: GroupStats }) {
  const items = () => [
    { name: "Aproveitamento", value: formatPercent(props.stats.score.achievement) },
    { name: "Acerto entre respondidas", value: formatPercent(props.stats.score.accuracy) },
    {
      name: "Cobertura",
      value: formatPercent(props.stats.coverage),
      hint: `${formatNumber(props.stats.answered)} de ${formatNumber(props.stats.available)}`,
    },
  ];
  return (
    <ul class="summary-strip" aria-label="Resumo">
      <For each={items()}>
        {(item) => (
          <li>
            <span class="stat-name">{item.name}</span>
            <strong>{item.value}</strong>
            <Show when={item.hint}>
              <span class="muted">{item.hint}</span>
            </Show>
          </li>
        )}
      </For>
    </ul>
  );
}
