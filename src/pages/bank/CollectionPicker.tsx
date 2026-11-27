import { A } from "@solidjs/router";
import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { CollectionSummary } from "../../bindings/CollectionSummary";
import FilterSelect from "../../components/FilterSelect";
import { EmptyState, Loading, ResourceView } from "../../components/states";
import { formatNumber, questionCount } from "../../format";

export const PAGE_SIZE = 20;

/** Collections with search, contest and board filters and pagination; a row picks one. */
export default function CollectionPicker(props: {
  onPick: (collection: CollectionSummary) => void;
}) {
  const [search, setSearch] = createSignal("");
  const [contest, setContest] = createSignal("");
  const [board, setBoard] = createSignal("");
  const [page, setPage] = createSignal(0);

  const query = () => ({
    search: search(),
    contest: contest(),
    board: board(),
    page: page(),
    pageSize: PAGE_SIZE,
  });
  const [result, { refetch }] = createResource(query, (q) => api.collections(q));
  /** The latest page, or nothing if loading failed (reading a failed resource throws). */
  const current = () => (result.error ? undefined : result.latest);

  /** Changing a filter starts again from the first page. */
  const filter = (setter: (value: string) => void) => (value: string) => {
    setter(value);
    setPage(0);
  };
  const pages = () => Math.max(1, Math.ceil((current()?.total ?? 0) / PAGE_SIZE));
  const filtering = () => search().trim() !== "" || contest() !== "" || board() !== "";
  const clearFilters = () => {
    setSearch("");
    setContest("");
    setBoard("");
    setPage(0);
  };

  return (
    <>
      <div class="filter-bar" role="search">
        <label class="field">
          <span class="field-label">Buscar cadernos</span>
          <input
            class="input"
            type="search"
            placeholder="Área, matéria ou assunto"
            value={search()}
            onInput={(event) => filter(setSearch)(event.currentTarget.value)}
          />
        </label>
        <FilterSelect
          field="contest"
          value={contest()}
          options={current()?.contests ?? []}
          onChange={filter(setContest)}
        />
        <Show when={(current()?.boards.length ?? 0) > 0}>
          <FilterSelect
            field="board"
            value={board()}
            options={current()?.boards ?? []}
            onChange={filter(setBoard)}
          />
        </Show>
      </div>
      <ResourceView
        value={current()?.total ? current() : undefined}
        error={() => result.error}
        pending={!current()}
        retrying={result.loading}
        onRetry={() => refetch()}
        placeholder={<Loading label="Carregando cadernos…" />}
        empty={
          <EmptyState
            title={filtering() ? "Nenhum caderno encontrado." : "Nenhum caderno ainda."}
            action={
              <Show
                when={filtering()}
                fallback={
                  <A class="btn btn-primary" href="/gerar">
                    Gerar questões
                  </A>
                }
              >
                <button type="button" class="btn" onClick={clearFilters}>
                  Limpar filtros
                </button>
              </Show>
            }
          />
        }
      >
        {(loaded) => (
          <>
            <p class="muted" aria-live="polite">
              {formatNumber(loaded().total)} {loaded().total === 1 ? "caderno" : "cadernos"}
            </p>
            <ul class="item-list is-ruled" aria-label="Cadernos">
              <For each={loaded().items}>
                {(collection) => (
                  <li>
                    <button
                      type="button"
                      class="collection-pick"
                      onClick={() => props.onPick(collection)}
                    >
                      <span class="item-title">{collection.title}</span>
                      <span class="collection-meta">
                        <Show when={collection.contest}>
                          <span class="badge">{collection.contest}</span>
                        </Show>
                        <Show when={collection.board}>
                          <span class="badge">{collection.board}</span>
                        </Show>
                        <span class="muted">{questionCount(collection.questions)}</span>
                      </span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
            <nav class="pager" aria-label="Páginas">
              <button
                type="button"
                class="btn"
                disabled={page() === 0}
                onClick={() => setPage(page() - 1)}
              >
                Anterior
              </button>
              <span>
                Página {page() + 1} de {pages()}
              </span>
              <button
                type="button"
                class="btn"
                disabled={page() + 1 >= pages()}
                onClick={() => setPage(page() + 1)}
              >
                Próxima
              </button>
            </nav>
          </>
        )}
      </ResourceView>
    </>
  );
}
