import { For, Show, createMemo, createSignal } from "solid-js";
import type { AreaStats } from "../../bindings/AreaStats";
import type { GroupStats } from "../../bindings/GroupStats";
import type { TopicStats } from "../../bindings/TopicStats";
import { formatNumber, formatPercent } from "../../format";
import StartSessionModal from "../solve/StartSessionModal";
import Sparkline from "./Sparkline";

/** `weakest` keeps the backend's order (weakest first); `alphabetical` sorts by name. */
export type StrengthOrder = "weakest" | "alphabetical";

const orderLabel: Record<StrengthOrder, string> = {
  weakest: "Mais fraco primeiro",
  alphabetical: "Ordem alfabética",
};
const orders = Object.keys(orderLabel) as StrengthOrder[];

const collator = new Intl.Collator("pt-BR", { sensitivity: "base", numeric: true });
const byName = <T extends { name: string }>(items: readonly T[]) =>
  [...items].sort((a, b) => collator.compare(a.name, b.name));

/** Areas, matters and topics sorted by name at every level. */
function alphabetical(areas: AreaStats[]): AreaStats[] {
  return byName(areas).map((area) => ({
    ...area,
    matters: byName(area.matters).map((matter) => ({ ...matter, topics: byName(matter.topics) })),
  }));
}

/** Marks a group whose percentages rest on very few answers. */
export function SmallSample(props: { stats: GroupStats }) {
  return (
    <Show when={props.stats.smallSample && props.stats.score.evaluated > 0}>
      <span class="tag" title="Menos de 5 respostas: ainda não dá para confiar nos percentuais.">
        amostra pequena
      </span>
    </Show>
  );
}

/** Achievement as a bar; the percentage beside it carries the value for screen readers. */
function MasteryBar(props: { stats: GroupStats }) {
  const achievement = () => props.stats.score.achievement;
  return (
    <span class="table-mastery">
      <span class="mastery-bar" aria-hidden="true">
        <span
          class="mastery-fill"
          style={{ width: `${Math.round((achievement() ?? 0) * 100)}%` }}
        />
      </span>
      <span class="mastery-value">{formatPercent(achievement())}</span>
    </span>
  );
}

function Cells(props: { stats: GroupStats }) {
  return (
    <>
      <td>
        <MasteryBar stats={props.stats} />
      </td>
      <td>{formatPercent(props.stats.score.accuracy)}</td>
      <td>{formatPercent(props.stats.coverage)}</td>
      <td>{formatNumber(props.stats.score.evaluated)}</td>
    </>
  );
}

/** Area › matter › topic, weakest first or by name. Areas and matters expand on demand. */
export default function StrengthTable(props: { areas: AreaStats[] }) {
  const [order, setOrder] = createSignal<StrengthOrder>("weakest");
  const areas = createMemo(() => (order() === "weakest" ? props.areas : alphabetical(props.areas)));
  const [open, setOpen] = createSignal<ReadonlySet<string>>(new Set());
  const toggle = (key: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  const [training, setTraining] = createSignal<{ topic: TopicStats; path: string }>();

  return (
    <>
      <fieldset class="order-choice">
        <legend>Ordenar</legend>
        <For each={orders}>
          {(value) => (
            <label>
              <input
                type="radio"
                name="strength-order"
                value={value}
                checked={order() === value}
                onChange={() => setOrder(value)}
              />
              {orderLabel[value]}
            </label>
          )}
        </For>
      </fieldset>
      <div
        class="table-scroll"
        role="region"
        tabindex="0"
        aria-label="Desempenho por área, matéria e assunto"
      >
        <table class="strength-table" aria-label="Desempenho por área, matéria e assunto">
          <thead>
            <tr>
              <th scope="col">Área › matéria › assunto</th>
              <th scope="col">Aproveitamento</th>
              <th scope="col">Acerto</th>
              <th scope="col">Cobertura</th>
              <th scope="col">Avaliadas</th>
              <th scope="col">Tendência</th>
              <th scope="col">
                <span class="sr-only">Ações</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <For each={areas()}>
              {(area) => {
                const areaKey = area.name;
                return (
                  <>
                    <tr class="level-area">
                      <th scope="row">
                        <button
                          type="button"
                          class="btn btn-link is-inline"
                          aria-expanded={open().has(areaKey)}
                          onClick={() => toggle(areaKey)}
                        >
                          {area.name}
                        </button>{" "}
                        <SmallSample stats={area.stats} />
                      </th>
                      <Cells stats={area.stats} />
                      <td />
                      <td />
                    </tr>
                    <Show when={open().has(areaKey)}>
                      <For each={area.matters}>
                        {(matter) => {
                          const matterKey = `${area.name}\u0000${matter.name}`;
                          return (
                            <>
                              <tr class="level-matter">
                                <th scope="row">
                                  <button
                                    type="button"
                                    class="btn btn-link is-inline"
                                    aria-expanded={open().has(matterKey)}
                                    onClick={() => toggle(matterKey)}
                                  >
                                    {matter.name}
                                  </button>{" "}
                                  <SmallSample stats={matter.stats} />
                                </th>
                                <Cells stats={matter.stats} />
                                <td />
                                <td />
                              </tr>
                              <Show when={open().has(matterKey)}>
                                <For each={matter.topics}>
                                  {(topic) => (
                                    <tr class="level-topic">
                                      <th scope="row">
                                        {topic.name} <SmallSample stats={topic.stats} />
                                      </th>
                                      <Cells stats={topic.stats} />
                                      <td>
                                        <Sparkline weeks={topic.trend} />
                                      </td>
                                      <td>
                                        <button
                                          type="button"
                                          class="btn btn-link is-inline"
                                          aria-label={`Treinar este assunto: ${topic.name}`}
                                          disabled={topic.stats.available === 0}
                                          onClick={() =>
                                            setTraining({
                                              topic,
                                              path: `${area.name} › ${matter.name} › ${topic.name}`,
                                            })
                                          }
                                        >
                                          Treinar
                                        </button>
                                      </td>
                                    </tr>
                                  )}
                                </For>
                              </Show>
                            </>
                          );
                        }}
                      </For>
                    </Show>
                  </>
                );
              }}
            </For>
          </tbody>
        </table>
      </div>
      <Show when={training()}>
        {(target) => (
          <StartSessionModal
            title={target().path}
            source={{ kind: "topic", topicId: target().topic.topicId }}
            available={target().topic.stats.available}
            onClose={() => setTraining(undefined)}
          />
        )}
      </Show>
    </>
  );
}
