import { For, Show } from "solid-js";
import type { Outcome } from "../../bindings/Outcome";
import type { ResponseView } from "../../bindings/ResponseView";
import { outcomeLabel } from "../../labels";

/** Questions per group: one block of the answer grid. */
export const GROUP_SIZE = 20;

/** Mark printed on a bubble so an outcome is never colour alone. */
const outcomeMark = (outcome: Outcome | undefined) =>
  outcome === "correct" ? "✓" : outcome === "wrong" ? "✗" : undefined;

/**
 * The question rail: progress, every question as a bubble in groups of twenty, and a legend.
 * A bubble shows an outcome only once the backend revealed it; before that it is empty or filled.
 */
export default function Navigator(props: {
  questions: ResponseView[];
  current: number;
  onSelect: (index: number) => void;
}) {
  const answered = () => props.questions.filter((q) => q.choice !== null).length;
  const total = () => props.questions.length;

  const describe = (q: ResponseView) => {
    const parts = [`Questão ${q.position + 1}`];
    if (q.choice !== null) parts.push("respondida");
    if (q.flagged) parts.push("marcada");
    if (q.reveal) parts.push(outcomeLabel(q.reveal.outcome, 1));
    return parts.join(", ");
  };

  const groups = () => {
    const result: { first: number; items: ResponseView[] }[] = [];
    for (let i = 0; i < props.questions.length; i += GROUP_SIZE) {
      result.push({ first: i, items: props.questions.slice(i, i + GROUP_SIZE) });
    }
    return result;
  };

  return (
    <nav class="rail" aria-label="Questões">
      <div class="rail-progress">
        <span class="label" data-testid="progress">
          {answered()} de {total()} respondidas
        </span>
        <span
          class="meter-track"
          role="progressbar"
          aria-label="Progresso da tentativa"
          aria-valuemin={0}
          aria-valuemax={total()}
          aria-valuenow={answered()}
          aria-valuetext={`${answered()} de ${total()} respondidas`}
        >
          <span
            class="meter-fill"
            style={{ width: `${total() === 0 ? 0 : (answered() / total()) * 100}%` }}
          />
        </span>
      </div>
      <For each={groups()}>
        {(group) => {
          const last = () => group.first + group.items.length;
          const label = () =>
            group.items.length === 1 ? `${last()}` : `${group.first + 1}–${last()}`;
          return (
            <div class="rail-group" role="group" aria-label={`Questões ${label()}`}>
              <span class="label" aria-hidden="true">
                Q {label()}
              </span>
              <div class="rail-grid">
                <For each={group.items}>
                  {(q, offset) => {
                    const index = () => group.first + offset();
                    const outcome = () => q.reveal?.outcome;
                    return (
                      <button
                        type="button"
                        classList={{
                          bubble: true,
                          "bubble-lg": true,
                          "is-filled": q.choice !== null && !q.reveal,
                          "is-current": index() === props.current,
                          "is-correct": outcome() === "correct",
                          "is-wrong": outcome() === "wrong",
                          "is-annulled": outcome() === "annulled",
                        }}
                        aria-label={describe(q)}
                        aria-current={index() === props.current ? "step" : undefined}
                        onClick={() => props.onSelect(index())}
                      >
                        {index() + 1}
                        <Show when={outcomeMark(outcome())}>
                          {(mark) => (
                            <span class="bubble-mark" aria-hidden="true">
                              {mark()}
                            </span>
                          )}
                        </Show>
                        <Show when={q.flagged}>
                          <span class="bubble-mark is-flag" aria-hidden="true">
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
      <ul class="rail-legend">
        <li>● Respondida</li>
        <li>✓ Certa · ✗ Errada</li>
        <li>◯ Em branco</li>
        <li>⚑ Marcada para revisar</li>
      </ul>
    </nav>
  );
}
