import { For, Show } from "solid-js";
import { A } from "@solidjs/router";
import { formatNumber } from "../../format";
import type { TopicPath } from "../../bindings/TopicPath";
import type { TopicTree } from "../../bindings/TopicTree";

/** Área › matéria › assunto: pick from the topics created in Banco. */
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
  const area = () => props.tree.areas.find((candidate) => candidate.name === props.value.area);
  const matter = () => area()?.matters.find((candidate) => candidate.name === props.value.matter);
  const topic = () => matter()?.topics.find((candidate) => candidate.name === props.value.topic);

  return (
    <div
      class="topic-path"
      onFocusOut={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) props.onLeave?.();
      }}
    >
      <Show
        when={props.tree.areas.length > 0}
        fallback={
          <p class="muted" role="note">
            Nenhum assunto criado ainda. Crie a área, a matéria e o assunto em{" "}
            <A href="/banco">Banco</A>.
          </p>
        }
      >
        <select
          class="input"
          id="gen-topic-area"
          aria-label="Área"
          aria-required="true"
          aria-invalid={invalid(props.value.area)}
          aria-describedby={describedBy(props.value.area)}
          value={props.value.area}
          onChange={(event) =>
            props.onChange({ area: event.currentTarget.value, matter: "", topic: "" })
          }
        >
          <option value="">Área</option>
          <For each={props.tree.areas}>
            {(item) => <option value={item.name}>{item.name}</option>}
          </For>
        </select>
        <select
          class="input"
          id="gen-topic-matter"
          aria-label="Matéria"
          aria-required="true"
          aria-invalid={invalid(props.value.matter)}
          aria-describedby={describedBy(props.value.matter)}
          disabled={!area()}
          value={props.value.matter}
          onChange={(event) =>
            props.onChange({ ...props.value, matter: event.currentTarget.value, topic: "" })
          }
        >
          <option value="">Matéria</option>
          <For each={area()?.matters}>
            {(item) => <option value={item.name}>{item.name}</option>}
          </For>
        </select>
        <select
          class="input"
          id="gen-topic-topic"
          aria-label="Assunto"
          aria-required="true"
          aria-invalid={invalid(props.value.topic)}
          aria-describedby={describedBy(props.value.topic)}
          disabled={!matter()}
          value={props.value.topic}
          onChange={(event) => props.onChange({ ...props.value, topic: event.currentTarget.value })}
        >
          <option value="">Assunto</option>
          <For each={matter()?.topics}>
            {(item) => <option value={item.name}>{item.name}</option>}
          </For>
        </select>
      </Show>
      <Show when={topic()}>
        {(found) => (
          <p class="muted" role="note">
            {found().stats.questions > 0
              ? `Já existem ${formatNumber(found().stats.questions)} questões neste assunto; a IA vai evitar repeti-las.`
              : "Este assunto ainda não tem questões."}
          </p>
        )}
      </Show>
    </div>
  );
}
