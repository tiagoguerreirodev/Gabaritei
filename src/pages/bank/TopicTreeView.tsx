import { A } from "@solidjs/router";
import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import { EmptyState, Loading, ResourceView } from "../../components/states";
import type { TopicNode } from "../../bindings/TopicNode";
import { formatPercent, questionCount } from "../../format";
import StartSessionModal from "../solve/StartSessionModal";

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
      <ResourceView
        value={tree()?.areas.length ? tree() : undefined}
        error={() => tree.error}
        pending={tree.loading}
        retrying={tree.loading}
        onRetry={() => refetch()}
        placeholder={<Loading variant="skeleton" label="Carregando assuntos…" />}
        empty={
          <EmptyState
            title="Nenhuma questão no banco ainda."
            action={
              <A class="button-link primary" href="/gerar">
                Gerar questões
              </A>
            }
          >
            <p class="muted">Importe o banco do Caderno Local em Início ou em Configurações.</p>
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
