import { For, Show } from "solid-js";
import type { DraftSummary } from "../../bindings/DraftSummary";
import { formatNumber } from "../../format";

function counts(s: DraftSummary) {
  return [
    {
      kind: "agreed",
      label: "Conferidas",
      count: s.agreed,
      icon: "✓",
      style: "verdict-ok",
      waiting: s.checking,
    },
    {
      kind: "fixed",
      label: "Corrigidas e conferidas",
      count: s.fixed,
      icon: "✓",
      style: "verdict-ok",
      waiting: s.checking,
    },
    {
      kind: "disputed",
      label: "Contestadas",
      count: s.disputed,
      icon: "▲",
      style: "verdict-disputed",
      waiting: s.checking,
    },
    {
      kind: "problems",
      label: "Problemas",
      count: s.problems.length,
      icon: "✕",
      style: "verdict-problems",
      waiting: false,
    },
    {
      kind: "unchecked",
      label: "Sem conferência",
      count: s.unchecked,
      icon: "○",
      style: undefined,
      waiting: false,
    },
  ] as const;
}

export function VerdictSentence(props: { summary: DraftSummary }) {
  const text = () =>
    counts(props.summary)
      .filter((row) => row.kind !== "problems" && (row.kind === "agreed" || row.count > 0))
      .map((row) => `${formatNumber(row.count)} ${row.label.toLocaleLowerCase("pt-BR")}`)
      .join(", ");
  return (
    <p>
      <Show
        when={!props.summary.checking}
        fallback="Conferência em andamento. Os resultados aparecem quando ela terminar."
      >
        {text()}.
      </Show>
    </p>
  );
}

export default function VerdictCounts(props: { summary: DraftSummary }) {
  const rows = () =>
    counts(props.summary).filter((row) => {
      switch (row.kind) {
        case "fixed":
          return row.waiting || row.count > 0;
        case "unchecked":
          return row.count > 0;
        default:
          return true;
      }
    });
  return (
    <section aria-labelledby="verdict-title">
      <h2 id="verdict-title">Resumo da conferência</h2>
      <Show when={props.summary.checking}>
        <p class="muted">Conferência em andamento: os números aparecem quando ela terminar.</p>
      </Show>
      <dl class="verdict-counts">
        <For each={rows()}>
          {(row) => (
            <div class={`card card-compact stat ${row.style ?? ""}`}>
              <dt class="stat-name">
                <span aria-hidden="true">{row.icon} </span>
                {row.label}
              </dt>
              <dd class="stat-value">{row.waiting ? "…" : formatNumber(row.count)}</dd>
            </div>
          )}
        </For>
      </dl>
    </section>
  );
}
