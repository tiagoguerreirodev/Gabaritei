import { For, Show, createSignal } from "solid-js";
import type { DraftSummary } from "../../bindings/DraftSummary";
import type { LineState } from "../../bindings/LineState";
import RichText from "../../components/RichText";
import { formatNumber, optionLetters } from "../../format";
import { VerdictSentence } from "./VerdictCounts";

const lineText: Record<LineState, string> = {
  agreed: "conferida",
  fixed: "corrigida e conferida",
  disputed: "contestada",
  unchecked: "sem conferência",
};

export default function QuestionLines(props: { summary: DraftSummary }) {
  const s = () => props.summary;
  const [onlyFlagged, setOnlyFlagged] = createSignal(false);
  const [expanded, setExpanded] = createSignal<ReadonlySet<number>>(new Set());
  const flagged = () => s().lines.filter((line) => line.content !== null);
  const allExpanded = () => flagged().every((line) => expanded().has(line.position));
  const setAll = (open: boolean) =>
    setExpanded(open ? new Set(flagged().map((line) => line.position)) : new Set<number>());
  const toggleLine = (position: number, open: boolean) =>
    setExpanded((current) => {
      if (current.has(position) === open) return current;
      const next = new Set(current);
      if (open) next.add(position);
      else next.delete(position);
      return next;
    });
  const visibleLines = () => (onlyFlagged() ? flagged() : s().lines);

  return (
    <section class="card card-wide" aria-labelledby="checks-title">
      <h2 id="checks-title">Conferência às cegas</h2>
      <VerdictSentence summary={s()} />
      <Show when={flagged().length > 0}>
        <div class="curate-toolbar">
          <label>
            <input
              type="checkbox"
              checked={onlyFlagged()}
              onChange={(event) => setOnlyFlagged(event.currentTarget.checked)}
            />{" "}
            Mostrar só as sinaladas ({formatNumber(flagged().length)})
          </label>
          <button type="button" class="btn" onClick={() => setAll(!allExpanded())}>
            {allExpanded() ? "Recolher todas" : "Expandir todas"}
          </button>
        </div>
      </Show>
      <ul class="question-lines" aria-label="Questões do pacote">
        <For each={visibleLines()}>
          {(line) => (
            <li
              class={`line-${line.state}`}
              classList={{ expanded: line.content !== null && expanded().has(line.position) }}
            >
              <Show
                when={line.content}
                fallback={
                  <>
                    Q{line.position} · {lineText[line.state]}
                  </>
                }
              >
                {(content) => (
                  <details
                    open={expanded().has(line.position)}
                    onToggle={(event) => toggleLine(line.position, event.currentTarget.open)}
                  >
                    <summary>
                      Q{line.position} · {lineText[line.state]}
                      {line.hasProblem ? " · com problema" : ""}
                    </summary>
                    <RichText text={content().stem} />
                    <ul class="flagged-options">
                      <For each={content().options}>
                        {(option, index) => (
                          <li>
                            <span class="option-letter">{optionLetters(content())[index()]}</span>{" "}
                            <RichText text={option.text} />
                          </li>
                        )}
                      </For>
                    </ul>
                  </details>
                )}
              </Show>
            </li>
          )}
        </For>
      </ul>
      <Show when={s().disputed > 0}>
        <p class="muted">
          Questões contestadas ganham um aviso quando você as responder, e você pode anulá-las.
        </p>
      </Show>
    </section>
  );
}
