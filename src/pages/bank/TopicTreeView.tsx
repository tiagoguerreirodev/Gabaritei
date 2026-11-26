import { For, Show, createResource, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { api } from "../../api";
import { EmptyState, Loading, ResourceView } from "../../components/states";
import { showError, showToast } from "../../components/toasts";
import type { TopicPath } from "../../bindings/TopicPath";
import type { TopicTree } from "../../bindings/TopicTree";
import type { TopicNode } from "../../bindings/TopicNode";
import { formatPercent, questionCount } from "../../format";
import StartSessionModal from "../solve/StartSessionModal";

/** Creates a topic (and the area and matter above it, if new) so Gerar can offer it. */
function NewTopicForm(props: { tree?: TopicTree; onCreated: () => void }) {
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
        type="text"
        aria-label="Novo assunto"
        placeholder="Assunto (ex.: Controle de constitucionalidade)"
        value={path.topic}
        onInput={(event) => setPath("topic", event.currentTarget.value)}
      />
      <button type="submit" disabled={saving()}>
        Criar assunto
      </button>
    </form>
  );
}

/** Area › matter › topic, each with its number of questions and a button to solve it. */
export default function TopicTreeView() {
  const [tree, { refetch }] = createResource(() => api.topicsTree());
  const [solving, setSolving] = createSignal<{ topic: TopicNode; path: string }>();

  return (
    <>
      <Show when={solving()}>
        {(target) => (
          <StartSessionModal
            title={target().path}
            source={{ kind: "topic", topicId: target().topic.id }}
            available={target().topic.questions}
            onClose={() => setSolving(undefined)}
          />
        )}
      </Show>
      <NewTopicForm
        tree={tree.state === "ready" ? tree() : undefined}
        onCreated={() => void refetch()}
      />
      <ResourceView
        value={tree()?.areas.length ? tree() : undefined}
        error={() => tree.error}
        pending={tree.loading}
        retrying={tree.loading}
        onRetry={() => refetch()}
        placeholder={<Loading variant="skeleton" label="Carregando assuntos…" />}
        empty={
          <EmptyState title="Nenhuma questão no banco ainda.">
            <p class="muted">
              Crie um assunto acima ou importe o banco do Caderno Local em Início ou em
              Configurações.
            </p>
          </EmptyState>
        }
      >
        {(loaded) => (
          <>
            <p class="muted">
              {questionCount(loaded().questions)} em {loaded().areas.length}{" "}
              {loaded().areas.length === 1 ? "área" : "áreas"}.
            </p>
            <ul class="tree" aria-label="Assuntos">
              <For each={loaded().areas}>
                {(area) => (
                  <li>
                    <details>
                      <summary>
                        <span class="tree-name">{area.name}</span>
                        <span class="count">{questionCount(area.questions)}</span>
                      </summary>
                      <ul>
                        <For each={area.matters}>
                          {(matter) => (
                            <li>
                              <details>
                                <summary>
                                  <span class="tree-name">{matter.name}</span>
                                  <span class="count">{questionCount(matter.questions)}</span>
                                </summary>
                                <ul>
                                  <For each={matter.topics}>
                                    {(topic) => (
                                      <li class="tree-leaf">
                                        <span class="tree-name">{topic.name}</span>
                                        <span class="count">{questionCount(topic.questions)}</span>
                                        <Show when={topic.accuracy !== null}>
                                          <span class="count" title="Acerto entre respondidas">
                                            acerto {formatPercent(topic.accuracy)}
                                          </span>
                                        </Show>
                                        <Show when={topic.questions > 0}>
                                          <button
                                            type="button"
                                            class="link-button"
                                            aria-label={`Resolver ${topic.name}`}
                                            onClick={() =>
                                              setSolving({
                                                topic,
                                                path: `${area.name} › ${matter.name} › ${topic.name}`,
                                              })
                                            }
                                          >
                                            Resolver
                                          </button>
                                        </Show>
                                      </li>
                                    )}
                                  </For>
                                </ul>
                              </details>
                            </li>
                          )}
                        </For>
                      </ul>
                    </details>
                  </li>
                )}
              </For>
            </ul>
          </>
        )}
      </ResourceView>
    </>
  );
}
