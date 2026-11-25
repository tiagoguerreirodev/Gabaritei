import { onMount } from "solid-js";
import type { Score } from "../../bindings/Score";
import { formatPercent } from "../../format";

/** End-of-attempt recap: points over evaluated questions, achievement and the four counts. The
 * counts carry a shape as well as a colour. Comparing with the previous attempt is left out:
 * no existing command returns the scores of earlier attempts. */
export default function ScoreSummary(props: { score: Score }) {
  let section: HTMLElement | undefined;
  onMount(() => section?.focus());
  return (
    <section class="card score recap" aria-label="Resultado" tabindex="-1" ref={section}>
      <h2>Resultado</h2>
      <p class="recap-headline" data-testid="recap-points">
        <strong>
          {props.score.correct} de {props.score.evaluated}
        </strong>{" "}
        pontos · {formatPercent(props.score.achievement)} de aproveitamento
      </p>
      <ul class="recap-counts" aria-label="Contagem por resultado">
        <li class="recap-correct">
          <span class="recap-glyph" aria-hidden="true">
            ✓
          </span>
          {props.score.correct} {props.score.correct === 1 ? "certa" : "certas"}
        </li>
        <li class="recap-wrong">
          <span class="recap-glyph" aria-hidden="true">
            ✕
          </span>
          {props.score.wrong} {props.score.wrong === 1 ? "errada" : "erradas"}
        </li>
        <li class="recap-blank">
          <span class="recap-glyph" aria-hidden="true">
            ○
          </span>
          {props.score.blank} em branco
        </li>
        <li class="recap-annulled">
          <span class="recap-glyph recap-struck" aria-hidden="true">
            –
          </span>
          {props.score.annulled} {props.score.annulled === 1 ? "anulada" : "anuladas"}
        </li>
      </ul>
      <dl class="facts">
        <dt>Acerto entre respondidas</dt>
        <dd>{formatPercent(props.score.accuracy)}</dd>
      </dl>
    </section>
  );
}
