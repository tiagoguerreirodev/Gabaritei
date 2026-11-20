import { For, Show, createSignal } from "solid-js";
import type { AreaStats } from "../../bindings/AreaStats";
import type { GroupStats } from "../../bindings/GroupStats";
import { formatNumber } from "../bank/format";
import { formatPercent } from "../solve/format";

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

function Cells(props: { stats: GroupStats }) {
  return (
    <>
      <td>{formatPercent(props.stats.score.achievement)}</td>
      <td>{formatPercent(props.stats.score.accuracy)}</td>
      <td>{formatPercent(props.stats.coverage)}</td>
      <td>{formatNumber(props.stats.score.evaluated)}</td>
    </>
  );
}

/** Area › matter › topic, weakest first. Areas and matters expand on demand. */
export default function StrengthTable(props: { areas: AreaStats[] }) {
  const [open, setOpen] = createSignal<ReadonlySet<string>>(new Set());
  const toggle = (key: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  return (
    <table class="strength-table">
      <thead>
        <tr>
          <th scope="col">Área › matéria › assunto</th>
          <th scope="col">Aproveitamento</th>
          <th scope="col">Acerto</th>
          <th scope="col">Cobertura</th>
          <th scope="col">Avaliadas</th>
        </tr>
      </thead>
      <tbody>
        <For each={props.areas}>
          {(area) => {
            const areaKey = area.name;
            return (
              <>
                <tr class="level-area">
                  <th scope="row">
                    <button
                      type="button"
                      class="link-button"
                      aria-expanded={open().has(areaKey)}
                      onClick={() => toggle(areaKey)}
                    >
                      {area.name}
                    </button>{" "}
                    <SmallSample stats={area.stats} />
                  </th>
                  <Cells stats={area.stats} />
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
                                class="link-button"
                                aria-expanded={open().has(matterKey)}
                                onClick={() => toggle(matterKey)}
                              >
                                {matter.name}
                              </button>{" "}
                              <SmallSample stats={matter.stats} />
                            </th>
                            <Cells stats={matter.stats} />
                          </tr>
                          <Show when={open().has(matterKey)}>
                            <For each={matter.topics}>
                              {(topic) => (
                                <tr class="level-topic">
                                  <th scope="row">
                                    {topic.name} <SmallSample stats={topic.stats} />
                                  </th>
                                  <Cells stats={topic.stats} />
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
  );
}
