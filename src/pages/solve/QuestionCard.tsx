import { For, Show } from "solid-js";
import type { Outcome } from "../../bindings/Outcome";
import type { ResponseView } from "../../bindings/ResponseView";
import QuestionImage from "../../components/QuestionImage";
import RichText from "../../components/RichText";
import { optionLetters } from "./format";

const outcomeText: Record<Outcome, string> = {
  correct: "Você acertou.",
  wrong: "Você errou.",
  blank: "Questão em branco.",
  annulled: "Questão anulada: não entra na pontuação.",
};

/**
 * One question: stem, image, options with eliminate, flag, and, once revealed, the key and
 * explanation. Everything about the answer comes from `question.reveal`, which the backend
 * only fills in after the question is revealed.
 */
export default function QuestionCard(props: {
  question: ResponseView;
  number: number;
  total: number;
  /** No changes allowed (finished attempt, or a revealed Study question). */
  locked: boolean;
  /** The attempt is finished: the flag can't change either. */
  finished: boolean;
  /** `X` was pressed: the next letter eliminates instead of answering. */
  eliminating: boolean;
  onAnswer: (optionId: string) => void;
  onEliminate: (optionId: string) => void;
  onFlag: () => void;
}) {
  const letters = () => optionLetters(props.question);
  const reveal = () => props.question.reveal;
  const keyLetter = () => {
    const key = reveal()?.answerId;
    const index = props.question.options.findIndex((o) => o.id === key);
    return index >= 0 ? letters()[index] : undefined;
  };

  return (
    <article class="question-card" aria-label={`Questão ${props.number} de ${props.total}`}>
      <header class="question-header">
        <span class="muted">
          Questão {props.number} de {props.total}
        </span>
        <button
          type="button"
          class="flag-button"
          aria-pressed={props.question.flagged}
          disabled={props.finished}
          onClick={() => props.onFlag()}
          title="Marcar para revisar (F)"
        >
          {props.question.flagged ? "★ Marcada" : "☆ Marcar"}
        </button>
      </header>
      <RichText class="question-stem" text={props.question.stem} />
      <Show when={props.question.image}>
        {(image) => (
          <QuestionImage name={image()} alt={props.question.imageAlt ?? "Figura de apoio"} />
        )}
      </Show>
      <Show when={props.eliminating}>
        <p class="hint" role="status">
          Escolha a letra da alternativa a eliminar.
        </p>
      </Show>
      <ol class="options">
        <For each={props.question.options}>
          {(option, index) => {
            const letter = () => letters()[index()];
            const selected = () => props.question.choice === option.id;
            const eliminated = () => props.question.eliminated.includes(option.id);
            const isKey = () => reveal()?.answerId === option.id;
            return (
              <li
                classList={{
                  option: true,
                  selected: selected(),
                  eliminated: eliminated(),
                  "option-key": isKey(),
                  "option-wrong": selected() && reveal() !== null && !isKey(),
                }}
              >
                <button
                  type="button"
                  class="option-choose"
                  aria-pressed={selected()}
                  disabled={props.locked}
                  onClick={() => props.onAnswer(option.id)}
                >
                  <span class="option-letter">{letter()}</span>
                  <RichText text={option.text} />
                  <Show when={isKey()}>
                    <span class="option-mark">Gabarito</span>
                  </Show>
                </button>
                <button
                  type="button"
                  class="icon-button option-eliminate"
                  aria-label={`${eliminated() ? "Restaurar" : "Eliminar"} alternativa ${letter()}`}
                  aria-pressed={eliminated()}
                  disabled={props.locked}
                  onClick={() => props.onEliminate(option.id)}
                >
                  ✕
                </button>
              </li>
            );
          }}
        </For>
      </ol>
      <Show when={reveal()}>
        {(revealed) => (
          <section class={`reveal reveal-${revealed().outcome}`} aria-label="Gabarito e explicação">
            <p class="reveal-outcome">
              <strong>{outcomeText[revealed().outcome]}</strong>
              <Show when={revealed().outcome !== "annulled" && keyLetter()}>
                {(letter) => <> Gabarito: {letter()}.</>}
              </Show>
            </p>
            <Show
              when={revealed().explanation.trim()}
              fallback={<p class="muted">Esta questão não tem explicação.</p>}
            >
              <h3>Explicação</h3>
              <RichText class="explanation" text={revealed().explanation} />
            </Show>
            <Show when={revealed().refs.length > 0}>
              <h3>Referências</h3>
              <ul class="refs">
                <For each={revealed().refs}>{(ref) => <li>{ref}</li>}</For>
              </ul>
            </Show>
          </section>
        )}
      </Show>
    </article>
  );
}
