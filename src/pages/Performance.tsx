import { A } from "@solidjs/router";
import { createAction } from "../actions";
import { feedbackModes, modeLabel } from "../labels";
import FilterSelect from "../components/FilterSelect";
import { For, Show, createEffect, createResource, createSignal } from "solid-js";
import { api } from "../api";
import type { FeedbackMode } from "../bindings/FeedbackMode";
import type { FilterOptions } from "../bindings/FilterOptions";
import type { PerformanceQuery } from "../bindings/PerformanceQuery";
import { EmptyState, Loading, ResourceView } from "../components/states";
import { showToast } from "../components/toasts";
import StatCards from "./performance/StatCards";
import StrengthTable, { SmallSample } from "./performance/StrengthTable";
import TrendChart from "./performance/TrendChart";
import { formatPercent } from "../format";

/** Minutes to add to UTC to get the local time (−180 in Brasília). */
const localOffsetMinutes = () => -new Date().getTimezoneOffset();

/** Desempenho: strength by topic (latest answer per question) and the weekly trend. */
export default function Performance() {
  const [contest, setContest] = createSignal("");
  const [board, setBoard] = createSignal("");
  const [mode, setMode] = createSignal<FeedbackMode | "">("");
  const exporting = createAction();

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
  const clearFilters = () => {
    setContest("");
    setBoard("");
    setMode("");
  };

  const exportCsv = () => {
    const filters = query();
    return exporting.run(async () => {
      if (await api.exportPerformanceCsv(filters)) showToast("Planilha salva.");
    });
  };

  const display = () => {
    const loaded = current();
    return loaded && (loaded.overall.available > 0 || loaded.overall.score.evaluated > 0)
      ? loaded
      : undefined;
  };

  return (
    <section class="page">
      <h1>Desempenho</h1>
      <p class="muted">
        A força de cada assunto vem da sua última resposta a cada questão; a evolução semanal conta
        todas as respostas.
      </p>

      <div class="filters" role="search">
        <FilterSelect
          field="contest"
          value={contest()}
          options={options().contests ?? []}
          onChange={setContest}
        />
        <FilterSelect
          field="board"
          value={board()}
          options={options().boards ?? []}
          onChange={setBoard}
        />
        <label>
          <span>Modo</span>
          <select
            value={mode()}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setMode(feedbackModes.find((mode) => mode === value) ?? "");
            }}
          >
            <option value="">Todos os modos</option>
            <For each={feedbackModes}>
              {(mode) => <option value={mode}>{modeLabel[mode]}</option>}
            </For>
          </select>
        </label>
        <button
          type="button"
          class="export-button"
          disabled={exporting.active()}
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
      <ResourceView
        value={display()}
        error={() => result.error}
        pending={result.state === "pending"}
        retrying={result.loading}
        onRetry={() => refetch()}
        placeholder={<Loading variant="skeleton" label="Calculando desempenho…" />}
        empty={
          <EmptyState
            title={
              filtering()
                ? "Nenhuma questão corresponde a estes filtros."
                : "Nenhuma questão no banco ainda."
            }
            action={
              <Show
                when={filtering()}
                fallback={
                  <A class="button-link primary" href="/gerar">
                    Gerar questões
                  </A>
                }
              >
                <button type="button" class="secondary" onClick={clearFilters}>
                  Limpar filtros
                </button>
              </Show>
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
        {(data) => (
          <>
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
                  action={
                    <Show
                      when={filtering()}
                      fallback={
                        <A class="button-link primary" href="/banco">
                          Ir para o Banco
                        </A>
                      }
                    >
                      <button type="button" class="secondary" onClick={clearFilters}>
                        Limpar filtros
                      </button>
                    </Show>
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
          </>
        )}
      </ResourceView>
    </section>
  );
}
