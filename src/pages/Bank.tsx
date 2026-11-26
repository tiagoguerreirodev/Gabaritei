import { A, useSearchParams } from "@solidjs/router";
import { For, Show, createMemo, createResource } from "solid-js";
import { api } from "../api";
import type { FeedbackMode } from "../bindings/FeedbackMode";
import type { QuestionFilter } from "../bindings/QuestionFilter";
import type { SessionOrder } from "../bindings/SessionOrder";
import { EmptyState, Loading, ResourceView } from "../components/states";
import NewTopicForm from "./bank/NewTopicForm";
import ResultPanel from "./bank/ResultPanel";
import SourceFilter from "./bank/SourceFilter";
import StatusFilter from "./bank/StatusFilter";
import TopicFilter from "./bank/TopicFilter";
import { bancoSearch, emptyFilter, parseBanco } from "./bank/filterUrl";
import type { BancoParams, BancoState } from "./bank/filterUrl";
import { presets } from "./bank/presets";

const presetButtons = [
  { label: "Revisões de hoje", preset: presets.reviews },
  { label: "Caderno de erros", preset: presets.errors },
];

/** Choose questions by topic, status and caderno, see the counts, and start a session. */
export default function Bank() {
  const [params, setParams] = useSearchParams<BancoParams>();
  const state = createMemo(() => parseBanco(params));
  const filterKey = () => JSON.stringify(state().filter);

  const [tree, { refetch: refetchTree }] = createResource(() => api.topicsTree());
  const [summary, { refetch: refetchSummary }] = createResource(
    () => state().filter,
    (filter) => api.selectionSummary(filter),
  );
  /** The latest summary, or nothing if loading failed (reading a failed resource throws). */
  const currentSummary = () => (summary.error ? undefined : summary.latest);

  const update = (change: Partial<BancoState>) =>
    setParams(bancoSearch({ ...state(), ...change }), { replace: true });
  const updateFilter = (change: Partial<QuestionFilter>) =>
    update({ filter: { ...state().filter, ...change } });
  const clearFilters = () => update({ filter: emptyFilter });

  const filtering = () => filterKey() !== JSON.stringify(emptyFilter);
  const isPressed = (preset: Partial<BancoState>) =>
    JSON.stringify(preset.filter) === filterKey() && preset.order === state().order;

  return (
    <section class="page">
      <h1>Banco</h1>
      <ResourceView
        value={tree()?.areas.length ? tree() : undefined}
        error={() => tree.error}
        pending={tree.loading}
        retrying={tree.loading}
        onRetry={() => refetchTree()}
        placeholder={<Loading label="Carregando assuntos…" />}
        empty={
          <EmptyState
            title="Nenhuma questão no banco ainda."
            action={
              <A class="btn btn-primary" href="/gerar">
                Gerar questões
              </A>
            }
          >
            <p class="muted">
              Crie um assunto abaixo ou importe o banco do Caderno Local em Início ou em
              Configurações.
            </p>
          </EmptyState>
        }
      >
        {(loaded) => (
          <>
            <div class="bank-presets">
              <For each={presetButtons}>
                {({ label, preset }) => (
                  <button
                    type="button"
                    class="btn"
                    aria-pressed={isPressed(preset)}
                    onClick={() => update({ ...preset })}
                  >
                    {label}
                  </button>
                )}
              </For>
            </div>
            <div class="bank-layout">
              <form
                class="card filter-panel"
                aria-label="Filtros"
                onSubmit={(e) => e.preventDefault()}
              >
                <TopicFilter
                  tree={loaded()}
                  counts={currentSummary()?.byTopic ?? []}
                  selected={state().filter.topicIds}
                  onChange={(topicIds) => updateFilter({ topicIds })}
                />
                <StatusFilter
                  counts={currentSummary()?.byStatus ?? []}
                  selected={state().filter.statuses}
                  onChange={(statuses) => updateFilter({ statuses })}
                />
                <SourceFilter
                  collectionId={state().filter.collectionId}
                  attemptId={state().filter.attemptId}
                  onCollection={(collectionId) => updateFilter({ collectionId })}
                  onClearAttempt={() => updateFilter({ attemptId: null })}
                />
                <div class="filter-panel-footer">
                  <button
                    type="button"
                    class="btn btn-link is-inline"
                    disabled={!filtering()}
                    onClick={clearFilters}
                  >
                    Limpar filtros
                  </button>
                </div>
              </form>
              <ResourceView
                value={currentSummary()}
                error={() => summary.error}
                pending={!currentSummary()}
                retrying={summary.loading}
                onRetry={() => refetchSummary()}
                placeholder={<Loading label="Contando questões…" />}
              >
                {(loadedSummary) => (
                  <Show
                    when={loadedSummary().matching > 0}
                    fallback={
                      <div class="card">
                        <EmptyState
                          title="Nenhuma questão com estes filtros."
                          action={
                            <Show when={filtering()}>
                              <button type="button" class="btn" onClick={clearFilters}>
                                Limpar filtros
                              </button>
                            </Show>
                          }
                        />
                      </div>
                    }
                  >
                    <ResultPanel
                      summary={loadedSummary()}
                      filter={state().filter}
                      order={state().order}
                      mode={state().mode}
                      onOrder={(order: SessionOrder) => update({ order })}
                      onMode={(mode: FeedbackMode) => update({ mode })}
                    />
                  </Show>
                )}
              </ResourceView>
            </div>
          </>
        )}
      </ResourceView>
      <NewTopicForm
        tree={tree.state === "ready" ? tree() : undefined}
        onCreated={() => void refetchTree()}
      />
    </section>
  );
}
