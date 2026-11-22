import { For, Show } from "solid-js";
import type { ResponseView } from "../../bindings/ResponseView";

/** Questions per group; sets that fit in one group show no group labels. */
export const GROUP_SIZE = 10;

/** Map of every question, as a grid of answer-sheet bubbles in groups of ten. State is never
 * colour alone: a mark for the outcome, a flag for marked questions, a line for annulled ones. */
export default function Navigator(props: {
  questions: ResponseView[];
  current: number;
  onSelect: (index: number) => void;
}) {
  const describe = (q: ResponseView) => {
    const parts = [`Questão ${q.position + 1}`];
    if (q.choice !== null) parts.push("respondida");
    if (q.flagged) parts.push("marcada");
    if (q.reveal) {
      const label = { correct: "certa", wrong: "errada", blank: "em branco", annulled: "anulada" };
      parts.push(label[q.reveal.outcome]);
    }
    return parts.join(", ");
  };

  const groups = () => {
    const result: { first: number; items: ResponseView[] }[] = [];
    for (let i = 0; i < props.questions.length; i += GROUP_SIZE) {
      result.push({ first: i, items: props.questions.slice(i, i + GROUP_SIZE) });
    }
    return result;
  };
  const grouped = () => props.questions.length > GROUP_SIZE;

  return (
    <nav class="navigator" aria-label="Mapa de questões">
      <For each={groups()}>
        {(group) => {
          const last = () => group.first + group.items.length;
          const label = () =>
            group.items.length === 1 ? `${last()}` : `${group.first + 1}–${last()}`;
          return (
            <div
              class="navigator-group"
              role={grouped() ? "group" : undefined}
              aria-label={grouped() ? `Questões ${label()}` : undefined}
            >
              <Show when={grouped()}>
                <span class="navigator-label" aria-hidden="true">
                  {label()}
                </span>
              </Show>
              <div class="navigator-bubbles">
                <For each={group.items}>
                  {(q, offset) => {
                    const index = () => group.first + offset();
                    return (
                      <button
                        type="button"
                        classList={{
                          current: index() === props.current,
                          answered: q.choice !== null,
                          flagged: q.flagged,
                          [`outcome-${q.reveal?.outcome ?? "hidden"}`]: true,
                        }}
                        aria-label={describe(q)}
                        aria-current={index() === props.current ? "step" : undefined}
                        onClick={() => props.onSelect(index())}
                      >
                        {index() + 1}
                        <Show when={q.reveal?.outcome === "correct"}>
                          <span class="nav-mark" aria-hidden="true">
                            ✓
                          </span>
                        </Show>
                        <Show when={q.reveal?.outcome === "wrong"}>
                          <span class="nav-mark" aria-hidden="true">
                            ✕
                          </span>
                        </Show>
                        <Show when={q.flagged}>
                          <span class="nav-flag" aria-hidden="true">
                            ⚑
                          </span>
                        </Show>
                      </button>
                    );
                  }}
                </For>
              </div>
            </div>
          );
        }}
      </For>
    </nav>
  );
}
