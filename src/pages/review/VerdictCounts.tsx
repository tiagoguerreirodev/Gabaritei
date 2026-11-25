import { Show } from "solid-js";
import type { DraftSummary } from "../../bindings/DraftSummary";
import { formatNumber } from "../../format";

export default function VerdictCounts(props: { summary: DraftSummary }) {
  const s = () => props.summary;
  const checking = () => s().checking;
  return (
    <section class="card wide verdict-summary" aria-labelledby="verdict-title">
      <h2 id="verdict-title">Resumo da conferência</h2>
      <Show when={checking()}>
        <p class="muted">Conferência em andamento: os números aparecem quando ela terminar.</p>
      </Show>
      <dl class="verdict-counts">
        <div class="verdict-ok">
          <dt>
            <span aria-hidden="true">✓ </span>Conferidas
          </dt>
          <dd>{checking() ? "…" : formatNumber(s().agreed)}</dd>
        </div>
        <Show when={checking() || s().fixed > 0}>
          <div class="verdict-ok">
            <dt>
              <span aria-hidden="true">✓ </span>Corrigidas e conferidas
            </dt>
            <dd>{checking() ? "…" : formatNumber(s().fixed)}</dd>
          </div>
        </Show>
        <div class="verdict-disputed">
          <dt>
            <span aria-hidden="true">▲ </span>Contestadas
          </dt>
          <dd>{checking() ? "…" : formatNumber(s().disputed)}</dd>
        </div>
        <div class="verdict-problems">
          <dt>
            <span aria-hidden="true">✕ </span>Problemas
          </dt>
          <dd>{formatNumber(s().problems.length)}</dd>
        </div>
        <Show when={s().unchecked > 0}>
          <div>
            <dt>
              <span aria-hidden="true">○ </span>Sem conferência
            </dt>
            <dd>{formatNumber(s().unchecked)}</dd>
          </div>
        </Show>
      </dl>
    </section>
  );
}
