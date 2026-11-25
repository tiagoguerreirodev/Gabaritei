import QuestionLines from "./review/QuestionLines";
import ReviewActions from "./review/ReviewActions";
import VerdictCounts from "./review/VerdictCounts";
import { A, useSearchParams } from "@solidjs/router";
import { For, Show, createEffect, createResource, on } from "solid-js";
import { api } from "../api";
import type { DraftSummary } from "../bindings/DraftSummary";
import { InlineError, ResourceView } from "../components/states";
import { generationStatus } from "../generation";
import { agentLabels, effortLabels } from "../labels";
import { formatDuration, formatNumber } from "../format";

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
  const [drafts, { refetch }] = createResource(() => api.draftsWaiting());
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
  const [summary, { refetch }] = createResource(
    () => props.id,
    (id) => api.draftSummary(id),
  );
  // A check that was running ends (or a retry finishes): show the new verdicts.
  createEffect(
    on(
      () => generationStatus()?.running,
      () => void refetch(),
      { defer: true },
    ),
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
  const s = () => props.summary;
  const checking = () => s().checking;

  return (
    <>
      <VerdictCounts summary={s()} />

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

      <QuestionLines summary={s()} />

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

      <ReviewActions summary={s()} onChanged={props.onChanged} />
    </>
  );
}
