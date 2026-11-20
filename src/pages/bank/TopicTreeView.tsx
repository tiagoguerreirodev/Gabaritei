import { For, Match, Switch, createResource } from "solid-js";
import { api } from "../../api";
import { EmptyState, ErrorState, Loading } from "../../components/states";
import { questionCount } from "./format";

/** Area › matter › topic, each with its number of questions. */
export default function TopicTreeView() {
  const [tree] = createResource(() => api.topicsTree());

  return (
    <Switch>
      <Match when={tree.error as unknown}>{(error) => <ErrorState error={error()} />}</Match>
      <Match when={tree.loading}>
        <Loading label="Carregando assuntos…" />
      </Match>
      <Match when={tree()?.areas.length === 0}>
        <EmptyState title="Nenhuma questão no banco ainda.">
          <p class="muted">Importe o banco do Caderno Local em Início ou em Configurações.</p>
        </EmptyState>
      </Match>
      <Match when={tree()}>
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
      </Match>
    </Switch>
  );
}
