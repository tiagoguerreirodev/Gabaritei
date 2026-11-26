import FieldError from "./generate/FieldError";
import SourcesFields from "./generate/SourcesFields";
import CountField from "./generate/CountField";
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
import type { GenerationOutcome } from "../bindings/GenerationOutcome";
import type { QuestionType } from "../bindings/QuestionType";
import type { CountLimits } from "../bindings/CountLimits";
import type { TopicTree } from "../bindings/TopicTree";
import { InlineError, ResourceView } from "../components/states";
import { showError } from "../components/toasts";
import GenerationProgress from "../components/GenerationProgress";
import { cloneJson } from "../json";
import {
  cancelledMessage,
  dismissGeneration,
  failedPrefix,
  generationStatus,
  isRecheckRun,
} from "../generation";
import RolePicker from "./generate/RolePicker";
import TargetFields from "./generate/TargetFields";
import TopicPicker from "./generate/TopicPicker";

/** The boards used by existing collections. Without them the form still works ("Outra"). */
async function collectionBoards(): Promise<string[]> {
  try {
    const page = await api.collections({
      search: "",
      contest: "",
      board: "",
      page: 0,
      pageSize: 1,
    });
    return page.boards;
  } catch {
    return [];
  }
}

/** Gerar: the generation form, and progress while it runs. */
export default function Generate() {
  const [data, { refetch }] = createResource(async () => {
    const [tree, settings, counts, boards] = await Promise.all([
      api.topicsTree(),
      api.aiSettings(),
      api.questionCountLimits({ kind: "generation" }),
      collectionBoards(),
    ]);
    return { tree, settings, counts, boards };
  });

  return (
    <section class="page">
      <h1>Gerar</h1>
      <p class="muted">
        Escolha um assunto e a quantidade de questões. Uma IA elabora o pacote e outra confere o
        gabarito às cegas, sem ver a resposta.
      </p>
      <ResourceView
        value={data()}
        error={() => data.error}
        pending={data.state === "pending"}
        retrying={data.loading}
        onRetry={() => refetch()}
      >
        {(loaded) => (
          <GenerateView
            tree={loaded().tree}
            settings={loaded().settings}
            counts={loaded().counts}
            boards={loaded().boards}
          />
        )}
      </ResourceView>
    </section>
  );
}

function GenerateView(props: {
  tree: TopicTree;
  settings: AiSettings;
  counts: CountLimits;
  boards: string[];
}) {
  const status = generationStatus;
  return (
    <Switch>
      <Match when={status()?.running}>
        <section class="card" aria-labelledby="running-title">
          <h2 id="running-title">
            {isRecheckRun() ? "Conferindo o pacote…" : "Gerando o pacote…"}
          </h2>
          <StepList states={reviewSteps} />
          <GenerationProgress />
          <p class="muted">
            Isto pode levar alguns minutos. Você pode usar outras páginas enquanto isso.
          </p>
        </section>
      </Match>
      <Match when={true}>
        <Outcome />
        <Form
          tree={props.tree}
          settings={props.settings}
          counts={props.counts}
          boards={props.boards}
        />
      </Match>
    </Switch>
  );
}

const STEPS = [
  { id: "topic", label: "Assunto" },
  { id: "options", label: "Opções" },
  { id: "review", label: "Revisão" },
] as const;
type StepState = "done" | "current" | "pending";
type StepStates = Record<(typeof STEPS)[number]["id"], StepState>;
const reviewSteps: StepStates = { topic: "done", options: "done", review: "current" };
const stepStateLabels: Record<StepState, string> = {
  done: " (concluída)",
  current: " (etapa atual)",
  pending: " (pendente)",
};

/**
 * The steps from topic to review, as an ordered list. The caller says which are done, current
 * or still pending, so a step is never "concluída" just because it was not touched. The state
 * is text and a shape, never colour alone. Fontes is not a step: sources are optional.
 */
function StepList(props: { states: StepStates }) {
  return (
    <ol class="step-list" role="list" aria-label="Etapas">
      <For each={STEPS}>
        {(step) => (
          <li
            classList={{
              done: props.states[step.id] === "done",
              current: props.states[step.id] === "current",
            }}
            aria-current={props.states[step.id] === "current" ? "step" : undefined}
          >
            {step.label}
            <span class="sr-only">{stepStateLabels[props.states[step.id]]}</span>
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
          {outcomeContent(outcome())}
          <button type="button" class="secondary" onClick={() => void dismissGeneration()}>
            Dispensar
          </button>
        </section>
      )}
    </Show>
  );
}

function outcomeContent(outcome: GenerationOutcome) {
  switch (outcome.kind) {
    case "draft":
      return (
        <>
          <StepList states={reviewSteps} />
          <p>O pacote ficou pronto.</p>
          <A
            href={`/revisar-pacote?id=${encodeURIComponent(outcome.draftId)}`}
            onClick={() => void dismissGeneration()}
          >
            Revisar pacote
          </A>
        </>
      );
    case "failed":
      return (
        <InlineError>
          {failedPrefix()} {outcome.message}
        </InlineError>
      );
    case "cancelled":
      return <p>{cancelledMessage()}</p>;
  }
}

function Form(props: {
  tree: TopicTree;
  settings: AiSettings;
  counts: CountLimits;
  boards: string[];
}) {
  const [form, setForm] = createStore<GenerateRequest>({
    topic: { area: "", matter: "", topic: "" },
    count: untrack(() => props.counts.initial),
    board: null,
    contest: null,
    position: null,
    format: null,
    instructions: null,
    sources: { remnote: false, web: false, attachments: [], styleReferences: [] },
    // Prefilled from the role defaults once; later changes in Configurações apply next time.
    generator: untrack(() => cloneJson(props.settings.roles.generate)),
    checker: untrack(() => cloneJson(props.settings.roles.blindCheck)),
  });
  const [submitting, setSubmitting] = createSignal(false);
  // The errors on screen. They change on submit and on blur, not on every keystroke.
  const [shownTopicError, setShownTopicError] = createSignal<string | null>(null);
  const [shownCountError, setShownCountError] = createSignal<string | null>(null);
  const [advancedOpen, setAdvancedOpen] = createSignal(false);
  const [optionsTouched, setOptionsTouched] = createSignal(false);
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

  const topicError = () =>
    form.topic.area.trim() && form.topic.matter.trim() && form.topic.topic.trim()
      ? null
      : "Selecione a área, a matéria e o assunto.";
  const countError = () =>
    Number.isInteger(form.count) && form.count >= props.counts.min && form.count <= props.counts.max
      ? null
      : `A quantidade deve ser um número inteiro de ${props.counts.min} a ${props.counts.max}.`;
  const shownErrors = () =>
    [shownTopicError(), shownCountError()].flatMap((error) => (error ? [error] : []));
  const validateTopic = () => setShownTopicError(topicError());
  const validateCount = () => setShownCountError(countError());
  const stepStates = (): StepStates => ({
    topic: topicError() ? "current" : "done",
    options: optionsTouched() ? "done" : "pending",
    review: "pending",
  });

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (changingRole() || problems().length > 0) return;
    validateTopic();
    validateCount();
    if (topicError() || countError()) {
      const fields = [
        [form.topic.area, "gen-topic-area"],
        [form.topic.matter, "gen-topic-matter"],
        [form.topic.topic, "gen-topic-topic"],
      ] as const;
      const target = fields.find(([value]) => !value.trim())?.[1] ?? "gen-count";
      document.getElementById(target)?.focus();
      return;
    }
    setSubmitting(true);
    try {
      await api.generationStart(cloneJson(unwrap(form)));
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
      <p class="muted">
        Campos obrigatórios: assunto e quantidade. Banca, concurso e cargo ajudam a imitar a prova.
        O resto tem valores prontos.
      </p>

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
        <FieldError id="gen-topic-error">{shownTopicError()}</FieldError>

        <CountField
          value={form.count}
          counts={props.counts}
          error={shownCountError()}
          onBlur={validateCount}
          onInput={(value) => {
            setOptionsTouched(true);
            setForm("count", value);
          }}
        />

        <label for="gen-format">Formato</label>
        <select
          id="gen-format"
          value={form.format ?? ""}
          onChange={(event) => {
            setOptionsTouched(true);
            setForm("format", (event.currentTarget.value || null) as QuestionType | null);
          }}
        >
          <option value="">Seguir a prova de referência ou misturar</option>
          <option value="multiple">Só múltipla escolha</option>
          <option value="boolean">Só Certo/Errado</option>
        </select>

        <TargetFields
          boards={props.boards}
          board={form.board}
          contest={form.contest}
          position={form.position}
          onBoard={(board) => setForm("board", board)}
          onContest={(contest) => setForm("contest", contest)}
          onPosition={(position) => setForm("position", position)}
        />
      </div>

      <details
        class="advanced"
        open={advancedOpen()}
        onToggle={(event) => {
          setAdvancedOpen(event.currentTarget.open);
          if (event.currentTarget.open) setOptionsTouched(true);
        }}
      >
        <summary>Opções avançadas: instruções, fontes e modelos</summary>
        <div class="form-grid">
          <label for="gen-instructions">Instruções</label>
          <textarea
            id="gen-instructions"
            rows="3"
            placeholder="Opcional: foco, nível, tipo de pegadinha…"
            value={form.instructions ?? ""}
            onInput={(event) => setForm("instructions", event.currentTarget.value || null)}
          />

          <SourcesFields
            value={form.sources}
            remnoteOn={props.settings.remnote.mode !== "off"}
            onChange={(sources) => setForm("sources", sources)}
          />

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
      <FieldError role="alert">
        {shownErrors().length > 0 &&
          `Falta preencher ${shownErrors().length === 1 ? "um campo" : "dois campos"} para gerar.`}
      </FieldError>
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
