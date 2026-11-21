import { A } from "@solidjs/router";
import { For, Match, Show, Switch, createResource, createSignal, untrack } from "solid-js";
import { createStore, unwrap } from "solid-js/store";
import { api } from "../api";
import { catalogState, roleProblem } from "../catalog";
import { CatalogNotice } from "../components/ModelOptions";
import type { AiSettings } from "../bindings/AiSettings";
import type { GenerateRequest } from "../bindings/GenerateRequest";
import type { QuestionType } from "../bindings/QuestionType";
import type { TopicTree } from "../bindings/TopicTree";
import { ErrorState, Loading } from "../components/states";
import { showError } from "../components/toasts";
import { dismissGeneration, generationStatus } from "../generation";
import RolePicker from "./generate/RolePicker";
import TopicPicker from "./generate/TopicPicker";

const BOARDS = [
  "Cebraspe",
  "FGV",
  "FCC",
  "Vunesp",
  "Quadrix",
  "IBFC",
  "Instituto AOCP",
  "Consulplan",
];

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** A deep copy where no two places share an object. */
function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** Gerar: the generation form, and progress while it runs. */
export default function Generate() {
  const [data] = createResource(async () => {
    const [tree, settings] = await Promise.all([api.topicsTree(), api.aiSettings()]);
    return { tree, settings };
  });

  return (
    <section class="page">
      <h1>Gerar</h1>
      <p class="muted">
        Escolha um assunto e a quantidade de questões. Uma IA elabora o pacote e outra confere o
        gabarito às cegas, sem ver a resposta.
      </p>
      <Show when={!data.error} fallback={<ErrorState error={data.error as unknown} />}>
        <Show when={data()} fallback={<Loading />}>
          {(loaded) => <GenerateView tree={loaded().tree} settings={loaded().settings} />}
        </Show>
      </Show>
    </section>
  );
}

function GenerateView(props: { tree: TopicTree; settings: AiSettings }) {
  const status = generationStatus;
  return (
    <Switch>
      <Match when={status()?.running}>
        <section class="card" aria-labelledby="running-title">
          <h2 id="running-title">Gerando o pacote…</h2>
          <p role="status">{status()?.progress?.label ?? "Preparando…"}</p>
          <p class="muted">
            Isto pode levar alguns minutos. Você pode usar outras páginas enquanto isso.
          </p>
          <button type="button" onClick={() => void api.generationCancel()}>
            Cancelar
          </button>
        </section>
      </Match>
      <Match when={true}>
        <Outcome />
        <Form tree={props.tree} settings={props.settings} />
      </Match>
    </Switch>
  );
}

function Outcome() {
  return (
    <Show when={generationStatus()?.outcome}>
      {(outcome) => (
        <section class="card" aria-label="Resultado da última geração">
          <Switch>
            <Match when={outcome().kind === "draft"}>
              <p>O pacote ficou pronto.</p>
              <A
                href={`/revisar-pacote?id=${encodeURIComponent(
                  (outcome() as { kind: "draft"; draftId: string }).draftId,
                )}`}
                onClick={() => void dismissGeneration()}
              >
                Revisar pacote
              </A>
            </Match>
            <Match when={outcome().kind === "failed"}>
              <p class="error" role="alert">
                A geração falhou: {(outcome() as { kind: "failed"; message: string }).message}
              </p>
            </Match>
            <Match when={outcome().kind === "cancelled"}>
              <p>Geração cancelada. Nada foi salvo.</p>
            </Match>
          </Switch>
          <button type="button" onClick={() => void dismissGeneration()}>
            Dispensar
          </button>
        </section>
      )}
    </Show>
  );
}

function Form(props: { tree: TopicTree; settings: AiSettings }) {
  const [form, setForm] = createStore<GenerateRequest>({
    topic: { area: "", matter: "", topic: "" },
    count: 10,
    board: null,
    format: null,
    instructions: null,
    sources: { remnote: false, web: false, attachments: [] },
    // Prefilled from the role defaults once; later changes in Configurações apply next time.
    generator: untrack(() => clone(props.settings.roles.generate)),
    checker: untrack(() => clone(props.settings.roles.blindCheck)),
  });
  const [submitting, setSubmitting] = createSignal(false);
  const remnoteOn = () => props.settings.remnote.mode !== "off";
  const generator = () => form.generator ?? props.settings.roles.generate;
  const checker = () => form.checker ?? props.settings.roles.blindCheck;
  const problems = () =>
    [
      ["Gerador", roleProblem(catalogState(), generator())],
      ["Conferente", roleProblem(catalogState(), checker())],
    ].flatMap(([label, problem]) => (problem ? [`${label}: ${problem}`] : []));

  const attach = async () => {
    try {
      const picked = await api.pickAttachments();
      const current = form.sources.attachments;
      setForm("sources", "attachments", [...new Set([...current, ...picked])]);
    } catch (error) {
      showError(error);
    }
  };

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    setSubmitting(true);
    try {
      await api.generationStart(clone(unwrap(form)));
    } catch (error) {
      showError(error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <form class="card wide" onSubmit={(event) => void submit(event)} aria-label="Nova geração">
      <h2>Nova geração</h2>
      <div class="form-grid">
        <span class="label" id="topic-label">
          Assunto
        </span>
        <TopicPicker
          tree={props.tree}
          value={form.topic}
          onChange={(topic) => setForm("topic", topic)}
        />

        <label for="gen-count">Quantidade</label>
        <input
          id="gen-count"
          type="number"
          min="1"
          max="30"
          value={form.count}
          onInput={(event) => setForm("count", Number(event.currentTarget.value))}
        />
        <p class="hint muted">De 1 a 30. Cada questão passa pela conferência às cegas.</p>

        <label for="gen-board">Estilo da banca</label>
        <input
          id="gen-board"
          type="text"
          list="gen-boards"
          placeholder="Opcional"
          value={form.board ?? ""}
          onInput={(event) => setForm("board", event.currentTarget.value || null)}
        />
        <datalist id="gen-boards">
          <For each={BOARDS}>{(board) => <option value={board} />}</For>
        </datalist>

        <label for="gen-format">Formato</label>
        <select
          id="gen-format"
          value={form.format ?? ""}
          onChange={(event) =>
            setForm("format", (event.currentTarget.value || null) as QuestionType | null)
          }
        >
          <option value="">Deixar a IA misturar os dois</option>
          <option value="multiple">Só múltipla escolha</option>
          <option value="boolean">Só Certo/Errado</option>
        </select>

        <label for="gen-instructions">Instruções</label>
        <textarea
          id="gen-instructions"
          rows="3"
          placeholder="Opcional: foco, nível, tipo de pegadinha…"
          value={form.instructions ?? ""}
          onInput={(event) => setForm("instructions", event.currentTarget.value || null)}
        />

        <span class="label">Fontes</span>
        <div>
          <label>
            <input
              type="checkbox"
              checked={form.sources.remnote}
              disabled={!remnoteOn()}
              onChange={(event) => setForm("sources", "remnote", event.currentTarget.checked)}
            />{" "}
            RemNote
          </label>
          <Show when={!remnoteOn()}>
            <span class="muted"> (ligue em Configurações)</span>
          </Show>
          <br />
          <label>
            <input
              type="checkbox"
              checked={form.sources.web}
              onChange={(event) => setForm("sources", "web", event.currentTarget.checked)}
            />{" "}
            Pesquisar na web
          </label>
          <button type="button" onClick={() => void attach()}>
            Anexar arquivos
          </button>
          <Show when={form.sources.attachments.length > 0}>
            <ul class="attachments" aria-label="Anexos">
              <For each={form.sources.attachments}>
                {(path) => (
                  <li>
                    {fileName(path)}{" "}
                    <button
                      type="button"
                      class="link-button"
                      aria-label={`Remover ${fileName(path)}`}
                      onClick={() =>
                        setForm(
                          "sources",
                          "attachments",
                          form.sources.attachments.filter((p) => p !== path),
                        )
                      }
                    >
                      remover
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </div>

        <span class="label">Gerador</span>
        <RolePicker
          label="Gerador"
          value={form.generator ?? props.settings.roles.generate}
          onChange={(config) => setForm("generator", config)}
        />
        <span class="label">Conferente</span>
        <RolePicker
          label="Conferente"
          value={form.checker ?? props.settings.roles.blindCheck}
          onChange={(config) => setForm("checker", config)}
        />
      </div>
      <CatalogNotice agents={[generator().agent, checker().agent]} />
      <For each={problems()}>
        {(problem) => (
          <p role="alert" class="error">
            {problem}
          </p>
        )}
      </For>
      <button type="submit" class="primary" disabled={submitting() || problems().length > 0}>
        {submitting() ? "Iniciando…" : "Gerar"}
      </button>
    </form>
  );
}
