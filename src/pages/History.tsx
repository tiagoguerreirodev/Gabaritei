import { For, Show, createResource, createSignal, type JSX } from "solid-js";
import { api } from "../api";
import type { ActiveAttempt } from "../bindings/ActiveAttempt";
import type { FeedbackMode } from "../bindings/FeedbackMode";
import type { HistoryItem } from "../bindings/HistoryItem";
import type { HistoryQuery } from "../bindings/HistoryQuery";
import type { HistorySource } from "../bindings/HistorySource";
import { EmptyState, ResourceView } from "../components/states";
import { formatDateTime, formatNumber, formatPercent } from "../format";
import { feedbackModes, modeLabel } from "../labels";
import { openAttempt } from "../navigation";
import { bancoHref, emptyFilter } from "./bank/filterUrl";

const PAGE_SIZE = 20;

const sourceOptions: { value: HistorySource | ""; label: string }[] = [
  { value: "", label: "Todas" },
  { value: "collection", label: "Cadernos" },
  { value: "filter", label: "Filtros do Banco" },
  { value: "older", label: "Anteriores" },
];

/** Histórico: attempts still open, then finished ones scored by the current keys. */
export default function History() {
  const [source, setSource] = createSignal<HistorySource | "">("");
  const [mode, setMode] = createSignal<FeedbackMode | "">("");
  const [page, setPage] = createSignal(0);
  const query = (): HistoryQuery => ({
    source: source() || null,
    mode: mode() || null,
    page: page(),
    pageSize: PAGE_SIZE,
  });
  const [open, { refetch: refetchOpen }] = createResource(() => api.activeAttempts());
  const [finished, { refetch: refetchFinished }] = createResource(query, (q) =>
    api.attemptHistory(q),
  );
  const filtering = () => source() !== "" || mode() !== "";
  const nothingAtAll = () =>
    open.state === "ready" &&
    finished.state === "ready" &&
    open().length === 0 &&
    finished().total === 0 &&
    !filtering();

  const chooseSource = (value: string) => {
    setSource(sourceOptions.find((option) => option.value === value)?.value || "");
    setPage(0);
  };
  const chooseMode = (value: string) => {
    setMode(feedbackModes.find((candidate) => candidate === value) ?? "");
    setPage(0);
  };

  return (
    <section class="page">
      <h1>Histórico</h1>
      <Show
        when={!nothingAtAll()}
        fallback={
          <EmptyState
            title="Nenhuma tentativa ainda."
            action={
              <a class="btn btn-primary" href="#/banco">
                Ir para o Banco
              </a>
            }
          >
            <p>As tentativas que você começar e finalizar aparecem aqui.</p>
          </EmptyState>
        }
      >
        <div class="form-row">
          <label class="field">
            <span class="field-label">Origem</span>
            <select
              class="input"
              value={source()}
              onChange={(event) => chooseSource(event.currentTarget.value)}
            >
              <For each={sourceOptions}>
                {(option) => <option value={option.value}>{option.label}</option>}
              </For>
            </select>
          </label>
          <label class="field">
            <span class="field-label">Modo</span>
            <select
              class="input"
              value={mode()}
              onChange={(event) => chooseMode(event.currentTarget.value)}
            >
              <option value="">Estudo e Simulado</option>
              <For each={feedbackModes}>
                {(candidate) => <option value={candidate}>{modeLabel[candidate]}</option>}
              </For>
            </select>
          </label>
        </div>
        <div class="history-sections">
          <OpenSection
            attempts={open.state === "ready" ? open() : undefined}
            pending={open.loading}
            error={() => open.error as unknown}
            onRetry={refetchOpen}
          />
          <ResourceView
            value={finished.error ? undefined : finished.latest}
            error={() => finished.error as unknown}
            pending={finished.loading && !finished.latest}
            retrying={finished.loading}
            onRetry={refetchFinished}
          >
            {(loaded) => (
              <FinishedSection
                items={loaded().items}
                total={loaded().total}
                page={page()}
                onPage={setPage}
              />
            )}
          </ResourceView>
        </div>
      </Show>
    </section>
  );
}

function OpenSection(props: {
  attempts: ActiveAttempt[] | undefined;
  pending: boolean;
  error: () => unknown;
  onRetry: () => unknown;
}) {
  return (
    <ResourceView
      value={props.attempts}
      error={props.error}
      pending={props.pending}
      onRetry={props.onRetry}
    >
      {(attempts) => (
        <Show when={attempts().length > 0}>
          <section class="history-section" aria-labelledby="history-open">
            <h2 class="label" id="history-open">
              Em andamento
            </h2>
            <ul class="history-list">
              <For each={attempts()}>
                {(attempt) => (
                  <HistoryRow
                    title={attempt.title}
                    meta={[
                      modeLabel[attempt.feedbackMode],
                      `${formatNumber(attempt.answered)}/${formatNumber(attempt.total)} respondidas`,
                      formatDateTime(attempt.startedAt),
                    ]}
                  >
                    <button
                      type="button"
                      class="btn btn-primary"
                      aria-label={`Continuar ${attempt.title}`}
                      onClick={() => openAttempt(attempt.id)}
                    >
                      Continuar
                    </button>
                  </HistoryRow>
                )}
              </For>
            </ul>
          </section>
        </Show>
      )}
    </ResourceView>
  );
}

function FinishedSection(props: {
  items: HistoryItem[];
  total: number;
  page: number;
  onPage: (page: number) => void;
}) {
  const pages = () => Math.max(1, Math.ceil(props.total / PAGE_SIZE));
  return (
    <section class="history-section" aria-labelledby="history-finished">
      <h2 class="label" id="history-finished">
        Finalizadas
      </h2>
      <Show
        when={props.items.length > 0}
        fallback={<p class="muted">Nenhuma tentativa finalizada com esses filtros.</p>}
      >
        <ul class="history-list">
          <For each={props.items}>
            {(item) => (
              <HistoryRow
                title={item.title}
                meta={[
                  modeLabel[item.feedbackMode],
                  `${formatNumber(item.score.correct)}/${formatNumber(item.score.evaluated)}`,
                  <span class="history-score">{formatPercent(item.score.achievement)}</span>,
                  formatDateTime(item.finishedAt),
                ]}
              >
                <button
                  type="button"
                  class="btn"
                  aria-label={`Abrir ${item.title}`}
                  onClick={() => openAttempt(item.id)}
                >
                  Abrir
                </button>
                <a class="btn" href={redoHref(item)} aria-label={`Refazer ${item.title}`}>
                  Refazer
                </a>
              </HistoryRow>
            )}
          </For>
        </ul>
      </Show>
      <Show when={props.total > PAGE_SIZE}>
        <nav class="pager" aria-label="Páginas">
          <button
            type="button"
            class="btn"
            disabled={props.page === 0}
            onClick={() => props.onPage(props.page - 1)}
          >
            Anterior
          </button>
          <span>
            Página {props.page + 1} de {pages()}
          </span>
          <button
            type="button"
            class="btn"
            disabled={props.page + 1 >= pages()}
            onClick={() => props.onPage(props.page + 1)}
          >
            Próxima
          </button>
        </nav>
      </Show>
    </section>
  );
}

function HistoryRow(props: { title: string; meta: JSX.Element[]; children: JSX.Element }) {
  return (
    <li class="history-row">
      <div>
        <p class="history-title">{props.title}</p>
        <p class="history-meta">
          <For each={props.meta}>{(part) => <span>{part}</span>}</For>
        </p>
      </div>
      <div class="history-actions">{props.children}</div>
    </li>
  );
}

/** Banco, filtered to the attempt's caderno or, for any other kind, to its own questions. */
function redoHref(item: HistoryItem): string {
  const filter = item.collectionId
    ? { ...emptyFilter, collectionId: item.collectionId }
    : { ...emptyFilter, attemptId: item.id };
  return bancoHref({ filter });
}
