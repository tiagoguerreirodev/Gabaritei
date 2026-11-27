import { For, Show, createResource, onMount } from "solid-js";
import { api } from "../../api";
import type { AttemptClosing } from "../../bindings/AttemptClosing";
import type { Score } from "../../bindings/Score";
import MasteryBar from "../../components/MasteryBar";
import { formatDate, formatNumber, formatPercent } from "../../format";
import { outcomeLabel } from "../../labels";
import { bancoHref, emptyFilter } from "../bank/filterUrl";
import { presets } from "../bank/presets";

/** End-of-attempt recap: points over evaluated questions, achievement and the four counts (the
 * counts carry a shape as well as a colour), then the comparison with the previous attempt, the
 * score per topic and the next steps. The extras are secondary: if they fail to load, the recap
 * and the links still show. */
export default function ScoreSummary(props: { attemptId: string; score: Score }) {
  let section: HTMLElement | undefined;
  onMount(() => section?.focus());
  const [closing] = createResource(
    () => props.attemptId,
    (id) => api.attemptClosing(id),
  );
  const loaded = () => (closing.state === "ready" ? closing() : undefined);
  return (
    <section class="card recap" aria-label="Resultado" tabindex="-1" ref={section}>
      <h2>Resultado</h2>
      <p class="recap-headline" data-testid="recap-points">
        <strong>
          {props.score.correct} de {props.score.evaluated}
        </strong>{" "}
        pontos · {formatPercent(props.score.achievement)} de aproveitamento
      </p>
      <Show when={loaded()?.previous}>
        {(previous) => <Comparison current={props.score} previous={previous()} />}
      </Show>
      <ul class="recap-counts" aria-label="Contagem por resultado">
        <li class="recap-correct">
          <span class="recap-glyph" aria-hidden="true">
            ✓
          </span>
          {props.score.correct} {outcomeLabel("correct", props.score.correct)}
        </li>
        <li class="recap-wrong">
          <span class="recap-glyph" aria-hidden="true">
            ✕
          </span>
          {props.score.wrong} {outcomeLabel("wrong", props.score.wrong)}
        </li>
        <li class="recap-blank">
          <span class="recap-glyph" aria-hidden="true">
            ○
          </span>
          {props.score.blank} {outcomeLabel("blank", props.score.blank)}
        </li>
        <li class="recap-annulled">
          <span class="recap-glyph recap-struck" aria-hidden="true">
            –
          </span>
          {props.score.annulled} {outcomeLabel("annulled", props.score.annulled)}
        </li>
      </ul>
      <dl class="facts">
        <dt>Acerto entre respondidas</dt>
        <dd>{formatPercent(props.score.accuracy)}</dd>
      </dl>
      <Show when={loaded()?.topics.length ? loaded() : undefined}>
        {(details) => <Topics topics={details().topics} />}
      </Show>
      <NextSteps attemptId={props.attemptId} wrong={props.score.wrong} />
    </section>
  );
}

type Previous = NonNullable<AttemptClosing["previous"]>;

function Comparison(props: { current: Score; previous: Previous }) {
  const text = () => {
    const { current, previous } = props;
    const reference = `tentativa anterior (${formatDate(previous.finishedAt)}: ${formatPercent(previous.score.achievement)})`;
    if (current.achievement === null || previous.score.achievement === null) {
      return `Comparação com a ${reference} indisponível.`;
    }
    const points = Math.round((current.achievement - previous.score.achievement) * 1000) / 10;
    if (points === 0) return `Igual à ${reference}.`;
    const direction = points > 0 ? "acima" : "abaixo";
    return `${formatNumber(Math.abs(points))} pontos percentuais ${direction} da ${reference}.`;
  };
  return (
    <p class="recap-comparison" data-testid="recap-comparison">
      {text()}
    </p>
  );
}

function Topics(props: { topics: AttemptClosing["topics"] }) {
  return (
    <>
      <h3>Por assunto</h3>
      <ul class="recap-topics" aria-label="Aproveitamento por assunto">
        <For each={props.topics}>
          {(topic) => (
            <li>
              <MasteryBar
                name={`${topic.path.matter} › ${topic.path.topic}`}
                achievement={topic.score.achievement}
              />
            </li>
          )}
        </For>
      </ul>
    </>
  );
}

function NextSteps(props: { attemptId: string; wrong: number }) {
  return (
    <nav class="recap-next" aria-label="Próximos passos">
      <Show when={props.wrong > 0}>
        <a
          class="btn btn-primary"
          href={bancoHref({
            filter: { ...emptyFilter, attemptId: props.attemptId, statuses: ["wrong"] },
          })}
        >
          Refazer só os erros
        </a>
      </Show>
      <a class="btn" href={bancoHref(presets.reviews)}>
        Ver revisões de hoje
      </a>
      <a class="btn" href="#/">
        Voltar ao Início
      </a>
    </nav>
  );
}
