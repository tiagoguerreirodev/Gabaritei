import { For, Match, Show, Switch, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { CollectionSummary } from "../../bindings/CollectionSummary";
import { EmptyState, ErrorState, Loading } from "../../components/states";
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
        <label>
          <span>Concurso</span>
          <select
            value={contest()}
            onChange={(event) => filter(setContest)(event.currentTarget.value)}
          >
            <option value="">Todos os concursos</option>
            <For each={current()?.contests ?? []}>
              {(name) => <option value={name}>{name}</option>}
            </For>
          </select>
        </label>
        <Show when={(current()?.boards.length ?? 0) > 0}>
          <label>
            <span>Banca</span>
            <select
              value={board()}
              onChange={(event) => filter(setBoard)(event.currentTarget.value)}
            >
              <option value="">Todas as bancas</option>
              <For each={current()?.boards ?? []}>
                {(name) => <option value={name}>{name}</option>}
              </For>
            </select>
          </label>
        </Show>
      </div>

      <Switch>
        <Match when={result.error as unknown}>
          {(error) => (
            <ErrorState error={error()} onRetry={() => refetch()} busy={result.loading} />
          )}
        </Match>
        <Match when={!current()}>
          <Loading variant="skeleton" label="Carregando cadernos…" />
        </Match>
        <Match when={current()?.total === 0}>
          <EmptyState title={filtering() ? "Nenhum caderno encontrado." : "Nenhum caderno ainda."}>
            <Show when={filtering()}>
              <p class="muted">Tente outra busca ou limpe os filtros.</p>
            </Show>
          </EmptyState>
        </Match>
        <Match when={current()}>
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
        </Match>
      </Switch>

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
