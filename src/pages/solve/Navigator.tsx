import { For } from "solid-js";
import type { ResponseView } from "../../bindings/ResponseView";

/** Map of every question: current, answered, flagged and, once revealed, the outcome. */
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

  return (
    <nav class="navigator" aria-label="Mapa de questões">
      <For each={props.questions}>
        {(q, index) => (
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
          </button>
        )}
      </For>
    </nav>
  );
}
