import { A } from "@solidjs/router";
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createResource,
  createSignal,
  untrack,
} from "solid-js";
import { createStore, unwrap } from "solid-js/store";
import { api } from "../api";
import { createRoleEditor } from "../catalog";
import { CatalogNotice } from "../components/ModelOptions";
import type { AiSettings } from "../bindings/AiSettings";
import type { GenerateRequest } from "../bindings/GenerateRequest";
import type { QuestionType } from "../bindings/QuestionType";
import type { CountLimits } from "../bindings/CountLimits";
import type { TopicTree } from "../bindings/TopicTree";
import { ErrorState, Loading } from "../components/states";
import { showError } from "../components/toasts";
import GenerationProgress from "../components/GenerationProgress";
import {
  cancelledMessage,
  dismissGeneration,
  failedPrefix,
  generationStatus,
  isRecheckRun,
} from "../generation";
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
    const [tree, settings, counts] = await Promise.all([
      api.topicsTree(),
      api.aiSettings(),
      api.questionCountLimits({ kind: "generation" }),
    ]);
    return { tree, settings, counts };
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
          {(loaded) => (
            <GenerateView
              tree={loaded().tree}
              settings={loaded().settings}
              counts={loaded().counts}
            />
          )}
        </Show>
      </Show>
    </section>
  );
}

function GenerateView(props: { tree: TopicTree; settings: AiSettings; counts: CountLimits }) {
  const status = generationStatus;
  return (
    <Switch>
      <Match when={status()?.running}>
        <section class="card" aria-labelledby="running-title">
          <h2 id="running-title">
            {isRecheckRun() ? "Conferindo o pacote…" : "Gerando o pacote…"}
          </h2>
          <StepList states={["done", "done", "current"]} />
          <GenerationProgress />
          <p class="muted">
            Isto pode levar alguns minutos. Você pode usar outras páginas enquanto isso.
          </p>
        </section>
      </Match>
      <Match when={true}>
        <Outcome />
        <Form tree={props.tree} settings={props.settings} counts={props.counts} />
      </Match>
    </Switch>
  );
}

const STEPS = ["Assunto", "Opções", "Revisão"] as const;
type StepState = "done" | "current" | "pending";

/**
 * The steps from topic to review, as an ordered list. The caller says which are done, current
 * or still pending, so a step is never "concluída" just because it was not touched. The state
 * is text and a shape, never colour alone. Fontes is not a step: sources are optional.
 */
function StepList(props: { states: readonly StepState[] }) {
  return (
    <ol class="step-list" role="list" aria-label="Etapas">
      <For each={STEPS}>
        {(step, index) => (
          <li
            classList={{
              done: props.states[index()] === "done",
              current: props.states[index()] === "current",
            }}
            aria-current={props.states[index()] === "current" ? "step" : undefined}
          >
            {step}
            <span class="sr-only">
              {props.states[index()] === "done"
                ? " (concluída)"
                : props.states[index()] === "current"
                  ? " (etapa atual)"
                  : " (pendente)"}
            </span>
          </li>
        )}
      </For>
    </ol>
  );
}

function Outcome() {
  return (
    <Show when={generationStatus()?.outcome}>
      {(outcome) => (
        <section class="card" aria-label="Resultado da última geração">
          <Switch>
            <Match when={outcome().kind === "draft"}>
              <StepList states={["done", "done", "current"]} />
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
                {failedPrefix()} {(outcome() as { kind: "failed"; message: string }).message}
              </p>
            </Match>
            <Match when={outcome().kind === "cancelled"}>
              <p>{cancelledMessage()}</p>
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

function Form(props: { tree: TopicTree; settings: AiSettings; counts: CountLimits }) {
  const [form, setForm] = createStore<GenerateRequest>({
    topic: { area: "", matter: "", topic: "" },
    count: untrack(() => props.counts.initial),
    board: null,
    format: null,
    instructions: null,
    sources: { remnote: false, web: false, attachments: [] },
    // Prefilled from the role defaults once; later changes in Configurações apply next time.
    generator: untrack(() => clone(props.settings.roles.generate)),
    checker: untrack(() => clone(props.settings.roles.blindCheck)),
  });
  const [submitting, setSubmitting] = createSignal(false);
  // The errors on screen. They change on submit and on blur, not on every keystroke.
  const [shownTopicError, setShownTopicError] = createSignal<string | null>(null);
  const [shownCountError, setShownCountError] = createSignal<string | null>(null);
  const [advancedOpen, setAdvancedOpen] = createSignal(false);
  const [optionsTouched, setOptionsTouched] = createSignal(false);
  const remnoteOn = () => props.settings.remnote.mode !== "off";
  const generator = () => form.generator ?? props.settings.roles.generate;
  const checker = () => form.checker ?? props.settings.roles.blindCheck;
  const generatorEditor = createRoleEditor(generator, (config) => setForm("generator", config));
  const checkerEditor = createRoleEditor(checker, (config) => setForm("checker", config));
  const changingRole = () => generatorEditor.changing() || checkerEditor.changing();
  const problems = () =>
    [
      ["Gerador", generatorEditor.options()?.problem],
      ["Conferente", checkerEditor.options()?.problem],
    ].flatMap(([label, problem]) => (problem ? [`${label}: ${problem}`] : []));
  // The role pickers that fix a problem live in the advanced options: show them.
  createEffect(() => {
    if (problems().length > 0) setAdvancedOpen(true);
  });

  const attach = async () => {
    try {
      const picked = await api.pickAttachments();
      const current = form.sources.attachments;
      setForm("sources", "attachments", [...new Set([...current, ...picked])]);
    } catch (error) {
      showError(error);
    }
  };

  const topicError = () =>
    form.topic.area.trim() && form.topic.matter.trim() && form.topic.topic.trim()
      ? null
      : "Informe a área, a matéria e o assunto.";
  const countError = () =>
    Number.isInteger(form.count) && form.count >= props.counts.min && form.count <= props.counts.max
      ? null
      : `A quantidade deve ser um número inteiro de ${props.counts.min} a ${props.counts.max}.`;
  const shownErrors = () =>
    [shownTopicError(), shownCountError()].flatMap((error) => (error ? [error] : []));
  const validateTopic = () => setShownTopicError(topicError());
  const validateCount = () => setShownCountError(countError());
  const stepStates = (): StepState[] => [
    topicError() ? "current" : "done",
    optionsTouched() ? "done" : "pending",
    "pending",
  ];

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (changingRole() || problems().length > 0) return;
    validateTopic();
    validateCount();
    if (topicError() || countError()) {
      const { area, matter } = form.topic;
      const target = topicError()
        ? !area.trim()
          ? "gen-topic-area"
          : !matter.trim()
            ? "gen-topic-matter"
            : "gen-topic-topic"
        : "gen-count";
      document.getElementById(target)?.focus();
      return;
    }
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
    <form
      class="card wide generate-form"
      onSubmit={(event) => void submit(event)}
      aria-label="Nova geração"
      noValidate
    >
      <h2>Nova geração</h2>
      <StepList states={stepStates()} />
      <p class="muted">Campos obrigatórios: assunto e quantidade. O resto tem valores prontos.</p>

      <div class="form-grid">
        <span class="label" id="topic-label">
          Assunto <span class="required">(obrigatório)</span>
        </span>
        <TopicPicker
          tree={props.tree}
          value={form.topic}
          onChange={(topic) => setForm("topic", topic)}
          error={shownTopicError()}
          errorId="gen-topic-error"
          onLeave={() => {
            const { area, matter, topic } = form.topic;
            if (shownTopicError() || area.trim() || matter.trim() || topic.trim()) validateTopic();
          }}
        />
        <Show when={shownTopicError()}>
          {(message) => (
            <p class="field-error" id="gen-topic-error">
              <span aria-hidden="true">✕ </span>
              {message()}
            </p>
          )}
        </Show>

        <label for="gen-count">Quantidade</label>
        <input
          id="gen-count"
          type="number"
          min={props.counts.min}
          max={props.counts.max}
          required
          aria-required="true"
          aria-invalid={shownCountError() ? "true" : undefined}
          aria-describedby={shownCountError() ? "gen-count-hint gen-count-error" : "gen-count-hint"}
          value={form.count}
          onInput={(event) => {
            setOptionsTouched(true);
            setForm("count", Number(event.currentTarget.value));
          }}
          onBlur={validateCount}
        />
        <p class="hint muted" id="gen-count-hint">
          Obrigatório. De {props.counts.min} a {props.counts.max}. Cada questão passa pela
          conferência às cegas.
        </p>
        <Show when={shownCountError()}>
          {(message) => (
            <p class="field-error" id="gen-count-error">
              <span aria-hidden="true">✕ </span>
              {message()}
            </p>
          )}
        </Show>

        <label for="gen-format">Formato</label>
        <select
          id="gen-format"
          value={form.format ?? ""}
          onChange={(event) => {
            setOptionsTouched(true);
            setForm("format", (event.currentTarget.value || null) as QuestionType | null);
          }}
        >
          <option value="">Deixar a IA misturar os dois</option>
          <option value="multiple">Só múltipla escolha</option>
          <option value="boolean">Só Certo/Errado</option>
        </select>
      </div>

      <details
        class="advanced"
        open={advancedOpen()}
        onToggle={(event) => {
          setAdvancedOpen(event.currentTarget.open);
          if (event.currentTarget.open) setOptionsTouched(true);
        }}
      >
        <summary>Opções avançadas: estilo da banca, instruções, fontes e modelos</summary>
        <div class="form-grid">
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
          <RolePicker label="Gerador" editor={generatorEditor} />
          <span class="label">Conferente</span>
          <RolePicker label="Conferente" editor={checkerEditor} />
        </div>
      </details>
      <CatalogNotice agents={[generator().agent, checker().agent]} />
      <Show when={problems().length > 0}>
        <div role="alert" class="error">
          <For each={problems()}>{(problem) => <p>{problem}</p>}</For>
          <p>Corrija em “Opções avançadas”, no Gerador e no Conferente, para poder gerar.</p>
        </div>
      </Show>
      <Show when={shownErrors().length > 0}>
        <p class="field-error" role="alert">
          <span aria-hidden="true">✕ </span>
          Falta preencher {shownErrors().length === 1 ? "um campo" : "dois campos"} para gerar.
        </p>
      </Show>
      <button
        type="submit"
        class="primary"
        disabled={submitting() || changingRole() || problems().length > 0}
      >
        {submitting() ? "Iniciando…" : "Gerar"}
      </button>
    </form>
  );
}
