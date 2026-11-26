import { Show } from "solid-js";
import type { GroupStats } from "../../bindings/GroupStats";
import { CompactMasteryBar } from "../../components/MasteryBar";
import { formatPercent, questionCount } from "../../format";

/** A tree node's name, question count, coverage and achievement bar. */
export default function NodeSummary(props: { name: string; stats: GroupStats }) {
  const solved = () => props.stats.score.evaluated > 0;
  return (
    <>
      <span class="tree-name">{props.name}</span>
      <span class="tree-stats">
        <span class="chip">{questionCount(props.stats.questions)}</span>
        <Show when={solved()} fallback={<span class="chip">nunca resolvido</span>}>
          <span class="count" title="Questões respondidas ao menos uma vez">
            cobertura {formatPercent(props.stats.coverage)}
          </span>
        </Show>
        <CompactMasteryBar name={props.name} achievement={props.stats.score.achievement} />
      </span>
    </>
  );
}
