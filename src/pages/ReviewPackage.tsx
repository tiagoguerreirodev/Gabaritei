import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { For, Show, createMemo, createResource, createSignal } from "solid-js";
import { createAction } from "../actions";
import { api } from "../api";
import type { DraftSummary } from "../bindings/DraftSummary";
import type { ImportResult } from "../bindings/ImportResult";
import type { LineState } from "../bindings/LineState";
import ConfirmModal from "../components/ConfirmModal";
import RichText from "../components/RichText";
import { InlineError, ResourceView } from "../components/states";
import { showToast } from "../components/toasts";
import { generationRunRevision, refreshWaitingDrafts } from "../generation";
import { agentLabels, effortLabels } from "../labels";
import { formatDuration, formatNumber, optionLetters } from "../format";

const lineText: Record<LineState, string> = {
  agreed: "conferida",
  fixed: "corrigida e conferida",
  disputed: "contestada",
  unchecked: "sem conferência",
};

/**
 * Revisar pacote. Shows counts, problems and one line per question ("Q3 · contestada"). Only a
 * flagged question expands to its stem and options. The key, explanation and references are
 * never shown: the backend doesn't send them.
 */
export default function ReviewPackage() {
  const [params] = useSearchParams<{ id?: string }>();
  return (
    <section class="page">
      <h1>Revisar pacote</h1>
      <Show when={params.id} fallback={<WaitingList />}>
        {(id) => <Package id={id()} />}
      </Show>
    </section>
  );
}

function WaitingList() {
  const runRevision = createMemo(generationRunRevision);
  const [drafts, { refetch }] = createResource(runRevision, () => api.draftsWaiting());
  return (
    <ResourceView
      value={drafts()?.length ? drafts() : undefined}
      error={() => drafts.error}
      pending={drafts.state === "pending"}
      retrying={drafts.loading}
      onRetry={() => refetch()}
      empty={
        <p class="muted">
          Nenhum pacote esperando revisão. <A href="/gerar">Gerar um pacote</A>.
        </p>
      }
    >
      {(list) => (
        <>
          <p class="muted">Pacotes esperando revisão:</p>
          <ul>
            <For each={list()}>
              {(draft) => (
                <li>
                  <A href={`/revisar-pacote?id=${encodeURIComponent(draft.id)}`}>
                    {draft.topic.area} › {draft.topic.matter} › {draft.topic.topic}
                  </A>{" "}
                  <span class="muted">
                    ({formatNumber(draft.count)} questões
                    {draft.disputed > 0 ? `, ${formatNumber(draft.disputed)} contestadas` : ""}
                    {draft.checking ? ", conferindo…" : ""})
                  </span>
                </li>
              )}
            </For>
          </ul>
        </>
      )}
    </ResourceView>
  );
}

function Package(props: { id: string }) {
  const runRevision = createMemo(generationRunRevision);
  const [summary, { refetch }] = createResource(
    () => ({ id: props.id, runRevision: runRevision() }),
    ({ id }) => api.draftSummary(id),
  );
  return (
    <ResourceView
      value={summary()}
      error={() => summary.error}
      pending={summary.state === "pending"}
      retrying={summary.loading}
      onRetry={() => refetch()}
    >
      {(current) => <Summary summary={current()} onChanged={() => void refetch()} />}
    </ResourceView>
  );
}

function Summary(props: { summary: DraftSummary; onChanged: () => void }) {
  const navigate = useNavigate();
  const { active: busy, run } = createAction();
  const [requestingCheck, setRequestingCheck] = createSignal(false);
  const [onlyFlagged, setOnlyFlagged] = createSignal(false);
  const [expanded, setExpanded] = createSignal<ReadonlySet<number>>(new Set());
  const [confirmingDiscard, setConfirmingDiscard] = createSignal(false);
  const s = () => props.summary;
  const waiting = () => s().status === "review";
  const checking = () => s().checking;
  const confirmed = () => s().confirmed;

  const imported = async (result: ImportResult) => {
    await refreshWaitingDrafts();
    showToast(
      `${formatNumber(result.imported)} questões importadas para “${result.title}”` +
        (result.skipped > 0 ? `; ${formatNumber(result.skipped)} ficaram de fora.` : "."),
    );
    navigate("/banco");
  };

  const importAll = () => {
    const { id, needsCheck } = s();
    const onChanged = props.onChanged;
    return run(async () => {
      setRequestingCheck(needsCheck);
      const outcome = await api.draftImportAll(id).finally(() => setRequestingCheck(false));
      if (outcome.kind === "pending") {
        onChanged();
        showToast(outcome.message);
      } else {
        await imported(outcome.result);
      }
    });
  };

  const importConfirmed = () => {
    const id = s().id;
    return run(async () => imported(await api.draftImport(id, "confirmed")));
  };

  const discard = () => {
    const id = s().id;
    return run(async () => {
      await api.draftDiscard(id);
      await refreshWaitingDrafts();
      showToast("Pacote descartado.");
      navigate("/gerar");
    });
  };

  const rerunCheck = () => {
    const id = s().id;
    return run(async () => {
      await api.blindCheckStart(id);
      showToast("Conferindo de novo. Acompanhe pelo aviso no topo.");
    });
  };

  const flagged = () => s().lines.filter((line) => line.content !== null);
  const allExpanded = () => flagged().every((line) => expanded().has(line.position));
  const setAll = (open: boolean) =>
    setExpanded(open ? new Set(flagged().map((line) => line.position)) : new Set<number>());
  const toggleLine = (position: number, open: boolean) =>
    setExpanded((current) => {
      if (current.has(position) === open) return current;
      const next = new Set(current);
      if (open) next.add(position);
      else next.delete(position);
      return next;
    });
  const visibleLines = () =>
    onlyFlagged() ? s().lines.filter((line) => line.content !== null) : s().lines;

  return (
    <>
      <section class="card wide verdict-summary" aria-labelledby="verdict-title">
        <h2 id="verdict-title">Resumo da conferência</h2>
        <Show when={checking()}>
          <p class="muted">Conferência em andamento: os números aparecem quando ela terminar.</p>
        </Show>
        <dl class="verdict-counts">
          <div class="verdict-ok">
            <dt>
              <span aria-hidden="true">✓ </span>Conferidas
            </dt>
            <dd>{checking() ? "…" : formatNumber(s().agreed)}</dd>
          </div>
          <Show when={checking() || s().fixed > 0}>
            <div class="verdict-ok">
              <dt>
                <span aria-hidden="true">✓ </span>Corrigidas e conferidas
              </dt>
              <dd>{checking() ? "…" : formatNumber(s().fixed)}</dd>
            </div>
          </Show>
          <div class="verdict-disputed">
            <dt>
              <span aria-hidden="true">▲ </span>Contestadas
            </dt>
            <dd>{checking() ? "…" : formatNumber(s().disputed)}</dd>
          </div>
          <div class="verdict-problems">
            <dt>
              <span aria-hidden="true">✕ </span>Problemas
            </dt>
            <dd>{formatNumber(s().problems.length)}</dd>
          </div>
          <Show when={s().unchecked > 0}>
            <div>
              <dt>
                <span aria-hidden="true">○ </span>Sem conferência
              </dt>
              <dd>{formatNumber(s().unchecked)}</dd>
            </div>
          </Show>
        </dl>
      </section>

      <section class="card wide" aria-labelledby="package-title">
        <h2 id="package-title">
          {s().topic.area} › {s().topic.matter} › {s().topic.topic}
        </h2>
        <p>
          {formatNumber(s().count)} questões
          {s().multiple > 0 ? `, ${formatNumber(s().multiple)} de múltipla escolha` : ""}
          {s().boolean > 0 ? `, ${formatNumber(s().boolean)} Certo/Errado` : ""}.
          <Show when={s().board}>{(board) => <> Estilo: {board()}.</>}</Show>
        </p>
        <p class="muted">
          Gerado por {agentLabels[s().generator.agent]} ({s().generator.model},{" "}
          {effortLabels[s().generator.effort].toLowerCase()}) em {formatDuration(s().durationMs)}.
          Conferido por {agentLabels[s().checker.agent]} ({s().checker.model},{" "}
          {effortLabels[s().checker.effort].toLowerCase()}).
        </p>
        <Show when={checking()}>
          <p role="status">
            A conferência às cegas ainda está em andamento. Os botões abaixo ficam disponíveis
            quando ela terminar.
          </p>
        </Show>
        <Show when={s().checkFailed}>
          {(message) => (
            <InlineError>A conferência às cegas não foi concluída: {message()}</InlineError>
          )}
        </Show>
        <Show when={s().duplicates > 0}>
          <p class="muted">
            {formatNumber(s().duplicates)} já existem no banco e serão aproveitadas, sem duplicar.
          </p>
        </Show>
      </section>

      <section class="card wide" aria-labelledby="checks-title">
        <h2 id="checks-title">Conferência às cegas</h2>
        <Show
          when={!checking()}
          fallback={<p>Conferência em andamento. Os resultados aparecem quando ela terminar.</p>}
        >
          <p>
            {formatNumber(s().agreed)} conferidas
            {s().fixed > 0 ? `, ${formatNumber(s().fixed)} corrigidas e conferidas` : ""}
            {s().disputed > 0 ? `, ${formatNumber(s().disputed)} contestadas` : ""}
            {s().unchecked > 0 ? `, ${formatNumber(s().unchecked)} sem conferência` : ""}.
          </p>
        </Show>
        <Show when={flagged().length > 0}>
          <div class="curate-toolbar">
            <label>
              <input
                type="checkbox"
                checked={onlyFlagged()}
                onChange={(event) => setOnlyFlagged(event.currentTarget.checked)}
              />{" "}
              Mostrar só as sinaladas ({formatNumber(flagged().length)})
            </label>
            <button type="button" onClick={() => setAll(!allExpanded())}>
              {allExpanded() ? "Recolher todas" : "Expandir todas"}
            </button>
          </div>
        </Show>
        <ul class="question-lines" aria-label="Questões do pacote">
          <For each={visibleLines()}>
            {(line) => (
              <li
                class={`line-${line.state}`}
                classList={{ expanded: line.content !== null && expanded().has(line.position) }}
              >
                <Show
                  when={line.content}
                  fallback={
                    <>
                      Q{line.position} · {lineText[line.state]}
                    </>
                  }
                >
                  {(content) => (
                    <details
                      open={expanded().has(line.position)}
                      onToggle={(event) => toggleLine(line.position, event.currentTarget.open)}
                    >
                      <summary>
                        Q{line.position} · {lineText[line.state]}
                        {line.hasProblem ? " · com problema" : ""}
                      </summary>
                      <RichText text={content().stem} />
                      <ul class="flagged-options">
                        <For each={content().options}>
                          {(option, index) => (
                            <li>
                              <span class="option-letter">{optionLetters(content())[index()]}</span>{" "}
                              <RichText text={option.text} />
                            </li>
                          )}
                        </For>
                      </ul>
                    </details>
                  )}
                </Show>
              </li>
            )}
          </For>
        </ul>
        <Show when={s().disputed > 0}>
          <p class="muted">
            Questões contestadas ganham um aviso quando você as responder, e você pode anulá-las.
          </p>
        </Show>
      </section>

      <Show when={s().problems.length > 0}>
        <section class="card wide" aria-labelledby="problems-title">
          <h2 id="problems-title">Problemas encontrados</h2>
          <ul>
            <For each={s().problems}>
              {(problem) => (
                <li class="error">
                  {problem.position === null ? "" : `Q${problem.position} · `}
                  {problem.message}
                </li>
              )}
            </For>
          </ul>
        </section>
      </Show>

      <Show
        when={waiting()}
        fallback={
          <p class="muted">
            Este pacote já foi {s().status === "imported" ? "importado" : "descartado"}.{" "}
            <A href="/gerar">Gerar outro</A>.
          </p>
        }
      >
        <Show when={requestingCheck() && !checking()}>
          <p role="status">Solicitando conferência e importação…</p>
        </Show>
        <div class="review-actions">
          <button
            type="button"
            class="primary"
            disabled={busy() || checking() || s().problems.length > 0}
            onClick={() => void importAll()}
          >
            {s().needsCheck ? "Conferir e importar tudo" : "Importar tudo"}
          </button>
          <button
            type="button"
            disabled={busy() || checking() || confirmed() === 0}
            onClick={() => void importConfirmed()}
          >
            Importar só as confirmadas
          </button>
          <Show when={!checking() && s().needsCheck}>
            <button type="button" disabled={busy()} onClick={() => void rerunCheck()}>
              Conferir de novo
            </button>
          </Show>
          <button
            type="button"
            disabled={busy() || checking()}
            onClick={() => setConfirmingDiscard(true)}
          >
            Descartar
          </button>
        </div>
        <Show when={confirmingDiscard()}>
          <ConfirmModal
            title="Descartar o pacote?"
            confirmLabel="Descartar pacote"
            cancelLabel="Voltar"
            danger
            busy={busy()}
            onClose={() => setConfirmingDiscard(false)}
            onConfirm={() => {
              setConfirmingDiscard(false);
              void discard();
            }}
          >
            <p>
              As {formatNumber(s().count)} questões deste pacote serão apagadas e nada entra no
              banco.
              <Show when={confirmed() > 0}>
                {" "}
                Para aproveitar parte delas, use “Importar só as confirmadas”.
              </Show>
            </p>
          </ConfirmModal>
        </Show>
        <Show when={s().problems.length > 0}>
          <p class="muted">
            “Importar tudo” fica bloqueado enquanto houver problemas. As questões sem problemas e
            confirmadas ainda podem ser importadas.
          </p>
        </Show>
      </Show>
    </>
  );
}
