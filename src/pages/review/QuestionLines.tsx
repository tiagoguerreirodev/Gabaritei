import { For, Show, createSignal } from "solid-js";
import type { DraftSummary } from "../../bindings/DraftSummary";
import type { LineState } from "../../bindings/LineState";
import type { QuestionLine } from "../../bindings/QuestionLine";
import RichText from "../../components/RichText";
import { formatNumber, optionLetters } from "../../format";
import { VerdictSentence } from "./VerdictCounts";

type VerdictFilter = "all" | "disputed" | "problems" | "approved";

const FILTERS: { id: VerdictFilter; label: string }[] = [
  { id: "all", label: "Todas" },
  { id: "disputed", label: "Contestadas" },
  { id: "problems", label: "Com problemas" },
  { id: "approved", label: "Aprovadas" },
];

const VERDICT_BADGES: Record<LineState, { label: string; classes: string }> = {
  agreed: { label: "Aprovada", classes: "badge badge-success" },
  fixed: { label: "Aprovada, corrigida", classes: "badge badge-success" },
  disputed: { label: "Contestada", classes: "badge badge-danger badge-dashed" },
  unchecked: { label: "Não verificada", classes: "badge" },
};

const matchesFilter: Record<VerdictFilter, (line: QuestionLine) => boolean> = {
  all: () => true,
  disputed: (line) => line.state === "disputed",
  problems: (line) => line.hasProblem,
  approved: (line) => line.state === "agreed" || line.state === "fixed",
};

export default function QuestionLines(props: { summary: DraftSummary }) {
  const s = () => props.summary;
  const [filter, setFilter] = createSignal<VerdictFilter>("all");
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
  const countFor = (id: VerdictFilter) => s().lines.filter(matchesFilter[id]).length;
  const visibleLines = () => s().lines.filter(matchesFilter[filter()]);
  const topicPath = () => `${s().topic.area} › ${s().topic.matter} › ${s().topic.topic}`;

  return (
    <section aria-labelledby="checks-title">
      <h2 id="checks-title">Conferência às cegas</h2>
      <VerdictSentence summary={s()} />
      <div class="review-bar">
        <ul class="filters" aria-label="Filtrar por conferência">
          <For each={FILTERS}>
            {(item) => (
              <li>
                <button
                  type="button"
                  class="filter"
                  aria-pressed={filter() === item.id}
                  onClick={() => setFilter(item.id)}
                >
                  {item.label} <span class="filter-count">{formatNumber(countFor(item.id))}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
        <Show when={flagged().length > 0}>
          <button type="button" class="btn btn-link" onClick={() => setAll(!allExpanded())}>
            {allExpanded() ? "Recolher todas" : "Expandir todas"}
          </button>
        </Show>
      </div>
      <Show
        when={visibleLines().length > 0}
        fallback={<p class="muted">Nenhuma questão neste filtro.</p>}
      >
        <ul class="review-list" aria-label="Questões do pacote">
          <For each={visibleLines()}>
            {(line) => (
              <li>
                <Show
                  when={line.content}
                  fallback={
                    <div class="review-row">
                      <div class="review-row-head">
                        <LineHead line={line} topic={topicPath()} />
                        <span class="review-row-chevron" aria-hidden="true" />
                      </div>
                    </div>
                  }
                >
                  {(content) => (
                    <details
                      class="review-row"
                      open={expanded().has(line.position)}
                      onToggle={(event) => toggleLine(line.position, event.currentTarget.open)}
                    >
                      <summary>
                        <LineHead line={line} topic={topicPath()} />
                        <span class="review-row-chevron" aria-hidden="true">
                          ›
                        </span>
                      </summary>
                      <div class="review-row-body">
                        <div class="sheet-stem">
                          <RichText text={content().stem} />
                        </div>
                        <ol class="review-row-options">
                          <For each={content().options}>
                            {(option, index) => (
                              <li>
                                <span class="option-letter">
                                  {optionLetters(content())[index()]}
                                </span>{" "}
                                <RichText text={option.text} />
                              </li>
                            )}
                          </For>
                        </ol>
                      </div>
                    </details>
                  )}
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show when={s().disputed > 0}>
        <p class="muted">
          Questões contestadas ganham um aviso quando você as responder, e você pode anulá-las.
        </p>
      </Show>
    </section>
  );
}

function LineHead(props: { line: QuestionLine; topic: string }) {
  const verdict = () => VERDICT_BADGES[props.line.state];
  return (
    <>
      <span class="review-row-number">Q{props.line.position}</span>
      <span class="review-row-topic">{props.topic}</span>
      <span class="review-row-badges">
        <span class={verdict().classes}>{verdict().label}</span>
        <Show when={props.line.hasProblem}>
          <span class="badge badge-danger">Com problemas</span>
        </Show>
      </span>
    </>
  );
}
