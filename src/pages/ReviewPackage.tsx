import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { For, Show, createEffect, createResource, createSignal, on } from "solid-js";
import { api } from "../api";
import type { DraftSummary } from "../bindings/DraftSummary";
import type { ImportMode } from "../bindings/ImportMode";
import type { LineState } from "../bindings/LineState";
import { ErrorState, Loading } from "../components/states";
import { showError, showToast } from "../components/toasts";
import { dismissGeneration, generationStatus } from "../generation";
import { agentLabels, effortLabels } from "./settings/labels";

const lineText: Record<LineState, string> = {
  agreed: "conferida",
  fixed: "corrigida e conferida",
  disputed: "contestada",
  unchecked: "sem conferência",
};

const number = (n: number) => n.toLocaleString("pt-BR");
const duration = (ms: number) => `${Math.max(1, Math.round(ms / 1000)).toLocaleString("pt-BR")} s`;

/**
 * Revisar pacote. Shows counts, problems and one line per question ("Q3 · contestada"). It
 * never shows a stem, an option, a key or an explanation: the backend doesn't send them.
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
  const [drafts] = createResource(() => api.draftsWaiting());
  return (
    <Show when={!drafts.error} fallback={<ErrorState error={drafts.error as unknown} />}>
      <Show when={drafts()} fallback={<Loading />}>
        {(list) => (
          <Show
            when={list().length > 0}
            fallback={
              <p class="muted">
                Nenhum pacote esperando revisão. <A href="/gerar">Gerar um pacote</A>.
              </p>
            }
          >
            <p class="muted">Pacotes esperando revisão:</p>
            <ul>
              <For each={list()}>
                {(draft) => (
                  <li>
                    <A href={`/revisar-pacote?id=${encodeURIComponent(draft.id)}`}>
                      {draft.topic.area} › {draft.topic.matter} › {draft.topic.topic}
                    </A>{" "}
                    <span class="muted">
                      ({number(draft.count)} questões
                      {draft.disputed > 0 ? `, ${number(draft.disputed)} contestadas` : ""}
                      {draft.checking ? ", conferindo…" : ""})
                    </span>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </Show>
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
    <Show when={!summary.error} fallback={<ErrorState error={summary.error as unknown} />}>
      <Show when={summary()} fallback={<Loading />}>
        {(current) => <Summary summary={current()} onChanged={() => void refetch()} />}
      </Show>
    </Show>
  );
}

function Summary(props: { summary: DraftSummary; onChanged: () => void }) {
  const navigate = useNavigate();
  const [busy, setBusy] = createSignal(false);
  const s = () => props.summary;
  const waiting = () => s().status === "review";
  const checking = () => s().checking;
  const confirmed = () => s().agreed + s().fixed;

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  /** Waits until the background job (the blind check) is done. */
  const waitForJob = async () => {
    for (;;) {
      const status = await api.generationStatus();
      if (!status.running) return;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  };

  const importAs = (mode: ImportMode) => {
    const id = s().id;
    const onChanged = props.onChanged;
    const needsCheck = mode === "all" && (s().unchecked > 0 || s().checkFailed !== null);
    return run(async () => {
      if (needsCheck) {
        // "Importar tudo" imports checked questions only: check the unchecked ones first.
        await api.blindCheckStart(id);
        showToast("Conferindo as questões que faltavam. A importação segue sozinha.");
        await waitForJob();
        await dismissGeneration();
        const after = await api.draftSummary(id);
        if (after.unchecked > 0 || after.checkFailed !== null || after.problems.length > 0) {
          onChanged();
          showToast(
            after.checkFailed !== null
              ? `A conferência não terminou: ${after.checkFailed} Nada foi importado.`
              : "O pacote ainda tem pendências. Nada foi importado.",
          );
          return;
        }
      }
      const result = await api.draftImport(id, mode);
      showToast(
        `${number(result.imported)} questões importadas para “${result.title}”` +
          (result.skipped > 0 ? `; ${number(result.skipped)} ficaram de fora.` : "."),
      );
      navigate("/banco");
    });
  };

  const discard = () => {
    const id = s().id;
    return run(async () => {
      await api.draftDiscard(id);
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

  return (
    <>
      <section class="card wide" aria-labelledby="package-title">
        <h2 id="package-title">
          {s().topic.area} › {s().topic.matter} › {s().topic.topic}
        </h2>
        <p>
          {number(s().count)} questões
          {s().multiple > 0 ? `, ${number(s().multiple)} de múltipla escolha` : ""}
          {s().boolean > 0 ? `, ${number(s().boolean)} Certo/Errado` : ""}.
          <Show when={s().board}>{(board) => <> Estilo: {board()}.</>}</Show>
        </p>
        <p class="muted">
          Gerado por {agentLabels[s().generator.agent]} ({s().generator.model},{" "}
          {effortLabels[s().generator.effort].toLowerCase()}) em {duration(s().durationMs)}.
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
            <p class="error" role="alert">
              A conferência às cegas não foi concluída: {message()}
            </p>
          )}
        </Show>
        <Show when={s().duplicates > 0}>
          <p class="muted">
            {number(s().duplicates)} já existem no banco e serão aproveitadas, sem duplicar.
          </p>
        </Show>
      </section>

      <section class="card wide" aria-labelledby="checks-title">
        <h2 id="checks-title">Conferência às cegas</h2>
        <p>
          {number(s().agreed)} conferidas
          {s().fixed > 0 ? `, ${number(s().fixed)} corrigidas e conferidas` : ""}
          {s().disputed > 0 ? `, ${number(s().disputed)} contestadas` : ""}
          {s().unchecked > 0 ? `, ${number(s().unchecked)} sem conferência` : ""}.
        </p>
        <ul class="question-lines" aria-label="Questões do pacote">
          <For each={s().lines}>
            {(line) => (
              <li class={`line-${line.state}`}>
                Q{line.position} · {lineText[line.state]}
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
        <div class="review-actions">
          <button
            type="button"
            class="primary"
            disabled={busy() || checking() || s().problems.length > 0}
            onClick={() => void importAs("all")}
          >
            {s().unchecked > 0 || s().checkFailed !== null
              ? "Conferir e importar tudo"
              : "Importar tudo"}
          </button>
          <button
            type="button"
            disabled={busy() || checking() || confirmed() === 0}
            onClick={() => void importAs("confirmed")}
          >
            Importar só as confirmadas
          </button>
          <Show when={!checking() && (s().checkFailed !== null || s().unchecked > 0)}>
            <button type="button" disabled={busy()} onClick={() => void rerunCheck()}>
              Conferir de novo
            </button>
          </Show>
          <button type="button" disabled={busy() || checking()} onClick={() => void discard()}>
            Descartar
          </button>
        </div>
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
