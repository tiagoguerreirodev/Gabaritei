import { For, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { api } from "../../api";
import { showError, showToast } from "../../components/toasts";
import type { TopicPath } from "../../bindings/TopicPath";
import type { TopicTree } from "../../bindings/TopicTree";

/** Creates a topic (and the area and matter above it, if new) so Gerar can offer it. */
export default function NewTopicForm(props: { tree?: TopicTree; onCreated: () => void }) {
  const [path, setPath] = createStore<TopicPath>({ area: "", matter: "", topic: "" });
  const [saving, setSaving] = createSignal(false);
  const matters = () => props.tree?.areas.find((area) => area.name === path.area.trim())?.matters;

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    setSaving(true);
    try {
      const created = await api.createTopic({ ...path });
      showToast(`Assunto criado: ${created.area} › ${created.matter} › ${created.topic}.`);
      setPath("topic", "");
      props.onCreated();
    } catch (error) {
      showError(error);
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      class="topic-path"
      aria-label="Cadastro de assunto"
      onSubmit={(event) => void submit(event)}
    >
      <input
        class="input"
        type="text"
        aria-label="Nova área"
        placeholder="Área (ex.: Direito)"
        list="new-topic-areas"
        value={path.area}
        onInput={(event) => setPath("area", event.currentTarget.value)}
      />
      <datalist id="new-topic-areas">
        <For each={props.tree?.areas}>{(area) => <option value={area.name} />}</For>
      </datalist>
      <input
        class="input"
        type="text"
        aria-label="Nova matéria"
        placeholder="Matéria (ex.: Constitucional)"
        list="new-topic-matters"
        value={path.matter}
        onInput={(event) => setPath("matter", event.currentTarget.value)}
      />
      <datalist id="new-topic-matters">
        <For each={matters()}>{(matter) => <option value={matter.name} />}</For>
      </datalist>
      <input
        class="input"
        type="text"
        aria-label="Novo assunto"
        placeholder="Assunto (ex.: Controle de constitucionalidade)"
        value={path.topic}
        onInput={(event) => setPath("topic", event.currentTarget.value)}
      />
      <button type="submit" class="btn" disabled={saving()}>
        Criar assunto
      </button>
    </form>
  );
}
