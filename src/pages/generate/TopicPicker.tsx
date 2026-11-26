import { For, Show, createResource } from "solid-js";
import { api } from "../../api";
import { formatNumber } from "../../format";
import type { TopicPath } from "../../bindings/TopicPath";
import type { TopicTree } from "../../bindings/TopicTree";

/** Area › matéria › assunto: pick from the bank or type a new path. */
export default function TopicPicker(props: {
  tree: TopicTree;
  value: TopicPath;
  onChange: (topic: TopicPath) => void;
  /** The error shown for the three fields: the empty ones are marked invalid and described by it. */
  error?: string | null;
  errorId?: string;
  /** Focus left the whole group (not just moved between its fields). */
  onLeave?: () => void;
}) {
  const invalid = (text: string) => (props.error && !text.trim() ? "true" : undefined);
  const describedBy = (text: string) => (props.error && !text.trim() ? props.errorId : undefined);
  const [choices] = createResource(
    () => ({ ...props.value }),
    (path) => api.topicChoices(path),
  );
  const current = () => (choices.loading || choices.error ? undefined : choices());

  const existingQuestions = () => {
    const count = current()?.questions;
    return count == null ? undefined : { count };
  };
  return (
    <div
      class="topic-path"
      onFocusOut={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) props.onLeave?.();
      }}
    >
      <input
        class="input"
        type="text"
        id="gen-topic-area"
        aria-label="Área"
        aria-required="true"
        aria-invalid={invalid(props.value.area)}
        aria-describedby={describedBy(props.value.area)}
        placeholder="Área (ex.: Direito)"
        list="topic-areas"
        value={props.value.area}
        onInput={(event) => props.onChange({ ...props.value, area: event.currentTarget.value })}
      />
      <datalist id="topic-areas">
        <For each={props.tree.areas}>{(area) => <option value={area.name} />}</For>
      </datalist>
      <input
        class="input"
        type="text"
        id="gen-topic-matter"
        aria-label="Matéria"
        aria-required="true"
        aria-invalid={invalid(props.value.matter)}
        aria-describedby={describedBy(props.value.matter)}
        placeholder="Matéria (ex.: Constitucional)"
        list="topic-matters"
        value={props.value.matter}
        onInput={(event) => props.onChange({ ...props.value, matter: event.currentTarget.value })}
      />
      <datalist id="topic-matters">
        <For each={current()?.matters}>{(matter) => <option value={matter} />}</For>
      </datalist>
      <input
        class="input"
        type="text"
        id="gen-topic-topic"
        aria-label="Assunto"
        aria-required="true"
        aria-invalid={invalid(props.value.topic)}
        aria-describedby={describedBy(props.value.topic)}
        placeholder="Assunto (ex.: Controle de constitucionalidade)"
        list="topic-topics"
        value={props.value.topic}
        onInput={(event) => props.onChange({ ...props.value, topic: event.currentTarget.value })}
      />
      <datalist id="topic-topics">
        <For each={current()?.topics}>{(topic) => <option value={topic} />}</For>
      </datalist>
      <Show when={current()?.isNew}>
        <p class="muted" role="note">
          Assunto novo: será criado quando você importar o pacote.
        </p>
      </Show>
      <Show when={existingQuestions()}>
        {(found) => (
          <p class="muted" role="note">
            Já existem {formatNumber(found().count)} questões neste assunto; a IA vai evitar
            repeti-las.
          </p>
        )}
      </Show>
    </div>
  );
}
