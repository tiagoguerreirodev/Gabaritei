import FilterSelect from "../../components/FilterSelect";
import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { CollectionSummary } from "../../bindings/CollectionSummary";
import { EmptyState, Loading, ResourceView } from "../../components/states";
import CollectionDetailModal from "./CollectionDetailModal";
import { formatDate, formatNumber, questionCount } from "../../format";

export const PAGE_SIZE = 20;

function status(collection: CollectionSummary) {
  if (collection.inProgress) return "Em andamento";
  if (collection.lastFinishedAt) return `Concluído em ${formatDate(collection.lastFinishedAt)}`;
  return "Não iniciado";
}

/** Collections with search, contest and board filters, and pagination. */
export default function CollectionsView() {
  const [search, setSearch] = createSignal("");
  const [contest, setContest] = createSignal("");
  const [board, setBoard] = createSignal("");
  const [page, setPage] = createSignal(0);
  const [selected, setSelected] = createSignal<CollectionSummary>();

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

  return (
    <>
      <div class="filters" role="search">
        <label>
          <span>Buscar cadernos</span>
          <input
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
        placeholder={<Loading variant="skeleton" label="Carregando cadernos…" />}
        empty={
          <EmptyState title={filtering() ? "Nenhum caderno encontrado." : "Nenhum caderno ainda."}>
            <Show when={filtering()}>
              <p class="muted">Tente outra busca ou limpe os filtros.</p>
            </Show>
          </EmptyState>
        }
      >
        {(loaded) => (
          <>
            <p class="muted" aria-live="polite">
              {formatNumber(loaded().total)} {loaded().total === 1 ? "caderno" : "cadernos"}
            </p>
            <ul class="collection-list" aria-label="Cadernos">
              <For each={loaded().items}>
                {(collection) => (
                  <li>
                    <button
                      type="button"
                      class="collection-row"
                      onClick={() => setSelected(collection)}
                    >
                      <span class="collection-title">{collection.title}</span>
                      <span class="collection-meta">
                        <Show when={collection.contest}>
                          <span class="tag">{collection.contest}</span>
                        </Show>
                        <Show when={collection.board}>
                          <span class="tag">{collection.board}</span>
                        </Show>
                        <span>{questionCount(collection.questions)}</span>
                        <span class="muted">{status(collection)}</span>
                      </span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
            <nav class="pager" aria-label="Páginas">
              <button type="button" disabled={page() === 0} onClick={() => setPage(page() - 1)}>
                Anterior
              </button>
              <span>
                Página {page() + 1} de {pages()}
              </span>
              <button
                type="button"
                disabled={page() + 1 >= pages()}
                onClick={() => setPage(page() + 1)}
              >
                Próxima
              </button>
            </nav>
          </>
        )}
      </ResourceView>

      <Show when={selected()}>
        {(collection) => (
          <CollectionDetailModal
            id={collection().id}
            title={collection().title}
            onClose={() => setSelected(undefined)}
          />
        )}
      </Show>
    </>
  );
}
