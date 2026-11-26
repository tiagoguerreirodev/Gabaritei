import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import { EmptyState, Loading, ResourceView } from "../../components/states";
import type { TopicStats } from "../../bindings/TopicStats";
import { questionCount } from "../../format";
import StartSessionModal from "../solve/StartSessionModal";
import NodeSummary from "./NodeSummary";

/**
 * Area › matter › topic, each with its question count, coverage and achievement bar, and a
 * button on each topic to solve it.
 */
export default function TopicTreeView() {
  const [tree, { refetch }] = createResource(() => api.topicsTree());
  const [solving, setSolving] = createSignal<{ topic: TopicStats; path: string }>();

  return (
    <>
      <Show when={solving()}>
        {(target) => (
          <StartSessionModal
            title={target().path}
            source={{ kind: "topic", topicId: target().topic.topicId }}
            available={target().topic.stats.questions}
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
        placeholder={<Loading label="Carregando assuntos…" />}
        empty={
          <EmptyState title="Nenhuma questão no banco ainda.">
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
                        <NodeSummary name={area.name} stats={area.stats} />
                      </summary>
                      <ul>
                        <For each={area.matters}>
                          {(matter) => (
                            <li>
                              <details>
                                <summary>
                                  <NodeSummary name={matter.name} stats={matter.stats} />
                                </summary>
                                <ul>
                                  <For each={matter.topics}>
                                    {(topic) => (
                                      <li class="tree-leaf">
                                        <NodeSummary name={topic.name} stats={topic.stats} />
                                        <button
                                          type="button"
                                          class="btn btn-primary"
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
