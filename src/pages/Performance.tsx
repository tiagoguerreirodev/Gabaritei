import { For, Match, Show, Switch, createEffect, createResource, createSignal } from "solid-js";
import { api } from "../api";
import type { FeedbackMode } from "../bindings/FeedbackMode";
import type { FilterOptions } from "../bindings/FilterOptions";
import type { PerformanceQuery } from "../bindings/PerformanceQuery";
import { EmptyState, ErrorState, Loading } from "../components/states";
import { showError, showToast } from "../components/toasts";
import StatCards from "./performance/StatCards";
import StrengthTable, { SmallSample } from "./performance/StrengthTable";
import TrendChart from "./performance/TrendChart";
import { formatPercent } from "./solve/format";

/** Minutes to add to UTC to get the local time (−180 in Brasília). */
const localOffsetMinutes = () => -new Date().getTimezoneOffset();

/** Desempenho: strength by topic (latest answer per question) and the weekly trend. */
export default function Performance() {
  const [contest, setContest] = createSignal("");
  const [board, setBoard] = createSignal("");
  const [mode, setMode] = createSignal<FeedbackMode | "">("");
  const [exporting, setExporting] = createSignal(false);

  const query = (): PerformanceQuery => ({
    contest: contest() || null,
    board: board() || null,
    feedbackMode: mode() || null,
    utcOffsetMinutes: localOffsetMinutes(),
  });
  const [result, { refetch }] = createResource(query, (q) => api.performance(q));
  /** The latest result, or nothing if loading failed (reading a failed resource throws). */
  const current = () => (result.error ? undefined : result.latest);
  /** The tags to offer: kept from the last successful load, so an error doesn't empty them. */
  const [options, setOptions] = createSignal<FilterOptions>({ contests: [], boards: [] });
  createEffect(() => {
    const loaded = current();
    if (loaded) setOptions(loaded.filters);
  });
  const filtering = () => contest() !== "" || board() !== "" || mode() !== "";

  const exportCsv = async () => {
    setExporting(true);
    try {
      if (await api.exportPerformanceCsv(query())) showToast("Planilha salva.");
    } catch (error) {
      showError(error);
    } finally {
      setExporting(false);
    }
  };

  return (
    <section class="page">
      <h1>Desempenho</h1>
      <p class="muted">
        A força de cada assunto vem da sua última resposta a cada questão; a evolução semanal conta
        todas as respostas.
      </p>

      <div class="filters" role="search">
        <label>
          <span>Concurso</span>
          <select value={contest()} onChange={(event) => setContest(event.currentTarget.value)}>
            <option value="">Todos os concursos</option>
            <For each={options().contests}>{(name) => <option value={name}>{name}</option>}</For>
          </select>
        </label>
        <label>
          <span>Banca</span>
          <select value={board()} onChange={(event) => setBoard(event.currentTarget.value)}>
            <option value="">Todas as bancas</option>
            <For each={options().boards}>{(name) => <option value={name}>{name}</option>}</For>
          </select>
        </label>
        <label>
          <span>Modo</span>
          <select
            value={mode()}
            onChange={(event) => setMode(event.currentTarget.value as FeedbackMode | "")}
          >
            <option value="">Todos os modos</option>
            <option value="study">Estudo</option>
            <option value="exam">Simulado</option>
          </select>
        </label>
        <button
          type="button"
          class="export-button"
          disabled={exporting()}
          onClick={() => void exportCsv()}
        >
          Exportar CSV
        </button>
      </div>

      <Show when={result.loading && current()}>
        <p class="muted" role="status">
          Atualizando…
        </p>
      </Show>
      <Switch>
        <Match when={result.error as unknown}>
          {(error) => <ErrorState error={error()} onRetry={() => void refetch()} />}
        </Match>
        <Match when={current()}>
          {(data) => (
            <Show
              when={data().overall.available > 0 || data().overall.score.evaluated > 0}
              fallback={
                <EmptyState
                  title={
                    filtering()
                      ? "Nenhuma questão corresponde a estes filtros."
                      : "Nenhuma questão no banco ainda."
                  }
                >
                  <p class="muted">
                    {filtering()
                      ? "Tente outro concurso, banca ou modo."
                      : "Importe o banco do Caderno Local em Início ou em Configurações."}
                  </p>
                </EmptyState>
              }
            >
              <StatCards stats={data().overall} label="Resumo" />
              <Show
                when={data().overall.score.evaluated > 0}
                fallback={
                  <EmptyState
                    title={
                      filtering()
                        ? "Nenhuma tentativa finalizada com estes filtros."
                        : "Você ainda não finalizou nenhuma tentativa."
                    }
                  >
                    <p class="muted">
                      Resolva um caderno ou assunto do Banco e finalize para ver seu desempenho.
                    </p>
                  </EmptyState>
                }
              >
                <section class="card wide" aria-labelledby="weakest-title">
                  <h2 id="weakest-title">Assuntos mais fracos</h2>
                  <ol class="weakest-list">
                    <For each={data().weakest}>
                      {(topic) => (
                        <li>
                          <span class="weak-name">
                            <strong>{topic.topic}</strong>
                            <span class="muted">
                              {" "}
                              {topic.area} › {topic.matter}
                            </span>
                          </span>
                          <span class="weak-value">
                            {formatPercent(topic.stats.score.achievement)}
                          </span>
                          <SmallSample stats={topic.stats} />
                        </li>
                      )}
                    </For>
                  </ol>
                </section>
              </Show>
              <section class="card wide" aria-labelledby="trend-title">
                <h2 id="trend-title">Evolução semanal</h2>
                <TrendChart weeks={data().trend} />
              </section>
              <Show when={data().areas.length > 0}>
                <section class="card wide" aria-labelledby="table-title">
                  <h2 id="table-title">Por área, matéria e assunto</h2>
                  <StrengthTable areas={data().areas} />
                </section>
              </Show>
            </Show>
          )}
        </Match>
        <Match when={result.loading}>
          <Loading variant="skeleton" label="Calculando desempenho…" />
        </Match>
      </Switch>
    </section>
  );
}
