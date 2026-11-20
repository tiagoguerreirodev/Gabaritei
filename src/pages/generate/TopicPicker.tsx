import { For, Show } from "solid-js";
import type { TopicPath } from "../../bindings/TopicPath";
import type { TopicTree } from "../../bindings/TopicTree";

/** Area › matéria › assunto: pick from the bank or type a new path. */
export default function TopicPicker(props: {
  tree: TopicTree;
  value: TopicPath;
  onChange: (topic: TopicPath) => void;
}) {
  const matters = () =>
    props.tree.areas.find((area) => area.name === props.value.area.trim())?.matters ?? [];
  const topics = () => matters().find((m) => m.name === props.value.matter.trim())?.topics ?? [];
  const isNew = () => {
    const { area, matter, topic } = props.value;
    if (!area.trim() || !matter.trim() || !topic.trim()) return false;
    return !topics().some((t) => t.name === topic.trim());
  };
  const existing = () => topics().find((t) => t.name === props.value.topic.trim());

  return (
    <div class="topic-path">
      <input
        type="text"
        aria-label="Área"
        placeholder="Área (ex.: Direito)"
        list="topic-areas"
        value={props.value.area}
        onInput={(event) => props.onChange({ ...props.value, area: event.currentTarget.value })}
      />
      <datalist id="topic-areas">
        <For each={props.tree.areas}>{(area) => <option value={area.name} />}</For>
      </datalist>
      <input
        type="text"
        aria-label="Matéria"
        placeholder="Matéria (ex.: Constitucional)"
        list="topic-matters"
        value={props.value.matter}
        onInput={(event) => props.onChange({ ...props.value, matter: event.currentTarget.value })}
      />
      <datalist id="topic-matters">
        <For each={matters()}>{(matter) => <option value={matter.name} />}</For>
      </datalist>
      <input
        type="text"
        aria-label="Assunto"
        placeholder="Assunto (ex.: Controle de constitucionalidade)"
        list="topic-topics"
        value={props.value.topic}
        onInput={(event) => props.onChange({ ...props.value, topic: event.currentTarget.value })}
      />
      <datalist id="topic-topics">
        <For each={topics()}>{(topic) => <option value={topic.name} />}</For>
      </datalist>
      <Show when={isNew()}>
        <p class="muted" role="note">
          Assunto novo: será criado quando você importar o pacote.
        </p>
      </Show>
      <Show when={existing()}>
        {(found) => (
          <p class="muted" role="note">
            Já existem {found().questions.toLocaleString("pt-BR")} questões neste assunto; a IA vai
            evitar repeti-las.
          </p>
        )}
      </Show>
    </div>
  );
}
