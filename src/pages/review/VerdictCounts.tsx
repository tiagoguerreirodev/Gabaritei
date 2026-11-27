import { For, Show } from "solid-js";
import type { DraftSummary } from "../../bindings/DraftSummary";
import { formatNumber } from "../../format";

function counts(s: DraftSummary) {
  return [
    { kind: "agreed", label: "Aprovadas", count: s.agreed, glyph: "✓", waiting: s.checking },
    {
      kind: "fixed",
      label: "Aprovadas após correção",
      count: s.fixed,
      glyph: "✓",
      waiting: s.checking,
    },
    {
      kind: "disputed",
      label: "Contestadas",
      count: s.disputed,
      glyph: undefined,
      badge: "badge badge-danger badge-dashed",
      waiting: s.checking,
    },
    {
      kind: "problems",
      label: "Com problemas",
      count: s.problems.length,
      glyph: "✗",
      badge: "badge badge-danger",
      waiting: false,
    },
    { kind: "unchecked", label: "Não verificadas", count: s.unchecked, glyph: "—", waiting: false },
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
      <ul class="stat-cards" aria-label="Contagens da conferência">
        <For each={rows()}>
          {(row) => (
            <li class="card card-compact stat">
              <span class="stat-name">
                <span class={"badge" in row ? row.badge : undefined}>
                  <Show when={row.glyph}>
                    <span aria-hidden="true">{row.glyph} </span>
                  </Show>
                  {row.label}
                </span>
              </span>
              <strong class="stat-value">{row.waiting ? "…" : formatNumber(row.count)}</strong>
            </li>
          )}
        </For>
      </ul>
    </section>
  );
}
