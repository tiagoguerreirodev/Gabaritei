import { For, Show, createEffect, createSignal, on, onCleanup } from "solid-js";
import type { RecheckView } from "../../bindings/RecheckView";
import type { ResponseView } from "../../bindings/ResponseView";
import QuestionImage from "../../components/QuestionImage";
import RichText from "../../components/RichText";
import { optionLetters } from "../../format";
import { outcomeText } from "../../labels";

const ANSWER_POP_MS = 600;
import RecheckPanel from "./RecheckPanel";
import { showError } from "../../components/toasts";

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
  /** Estudo: the option marked but not yet confirmed. */
  pending: string | null;
  /** Estudo: choosing only marks an option; the button or Enter sends the answer. */
  confirmRequired: boolean;
  /** Chooses an option: answers it, or only marks it when confirmation is required. */
  onAnswer: (optionId: string) => void;
  onConfirm: () => void;
  onEliminate: (optionId: string) => void;
  onFlag: () => void;
  /** Annuls the revealed question, or restores it with `false`. */
  onAnnul: (annulled: boolean) => void;
  /** Asks the AI to re-check the key; resolves with its proposal. */
  onRecheck: () => Promise<RecheckView>;
  onRecheckApply: () => void;
  onRecheckDismiss: () => void;
  onAiCancel: () => void;
  /** Asks the AI to write the missing explanation. */
  onExplain: () => Promise<void>;
}) {
  let revealSection: HTMLElement | undefined;
  // The right-answer pop plays only while this is set: right after a confirmation, never on
  // navigation or when reviewing a finished attempt.
  const [popping, setPopping] = createSignal(false);
  let popTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(popTimer));
  const [explaining, setExplaining] = createSignal(false);
  // Moving to another question ends the "explaining" state of this card.
  createEffect(
    on(
      () => props.question.position,
      (position, previous) => {
        // The question object changes on every answer; only a different question resets.
        if (previous === undefined || position === previous) return;
        setExplaining(false);
        clearTimeout(popTimer);
        setPopping(false);
      },
    ),
  );
  const explain = async () => {
    const asked = props.question.position;
    setExplaining(true);
    try {
      await props.onExplain();
    } catch (error) {
      showError(error);
    } finally {
      if (props.question.position === asked) setExplaining(false);
    }
  };
  // A confirmed answer hands the focus to the reveal, so a screen reader reads it (the option
  // button is disabled by then). Navigating to an already revealed question leaves focus alone.
  createEffect(
    on(
      () => [props.question.position, props.question.reveal === null] as const,
      ([position, hidden], previous) => {
        if (previous && previous[0] === position && previous[1] && !hidden && !props.finished) {
          queueMicrotask(() => revealSection?.focus());
          setPopping(true);
          clearTimeout(popTimer);
          popTimer = setTimeout(() => setPopping(false), ANSWER_POP_MS);
        }
      },
    ),
  );
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
          <span aria-hidden="true">⚑ </span>
          {props.question.flagged ? "Marcada" : "Marcar"}
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
            // A mark that was never confirmed is not an answer once the question is locked.
            const marked = () => !props.locked && props.pending === option.id;
            const selected = () => props.question.choice === option.id || marked();
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
                  // Confirmed right answer: the bubble fills with a small pop.
                  "option-correct": selected() && isKey(),
                  "option-pop": popping() && selected() && isKey(),
                }}
              >
                <button
                  type="button"
                  class="option-choose"
                  data-option-id={option.id}
                  aria-pressed={selected()}
                  disabled={props.locked}
                  onClick={() => props.onAnswer(option.id)}
                >
                  <span class="option-bubble">
                    <span class="option-letter">{letter()}</span>
                  </span>
                  <RichText text={option.text} />
                  <Show when={isKey()}>
                    <span class="option-mark">
                      <span aria-hidden="true">✓ </span>Gabarito
                    </span>
                  </Show>
                  <Show when={selected() && reveal() !== null && !isKey()}>
                    <span class="option-mark option-mark-wrong">
                      <span aria-hidden="true">✕ </span>Sua resposta
                    </span>
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
      <Show when={props.confirmRequired && !props.locked}>
        <div class="confirm-row">
          <button
            type="button"
            class="primary"
            disabled={props.pending === null}
            onClick={() => props.onConfirm()}
          >
            Confirmar resposta (Enter)
          </button>
        </div>
      </Show>
      <Show when={reveal()}>
        {(revealed) => (
          <section
            ref={revealSection}
            tabindex="-1"
            class={`reveal reveal-${revealed().outcome}`}
            aria-label="Gabarito e explicação"
          >
            <Show when={revealed().disputed}>
              <p class="disputed-badge" role="note">
                Gabarito contestado na conferência: a IA conferente discordou da resposta. Se algo
                parecer errado, use “Pedir revisão à IA” ou “Anular questão”.
              </p>
            </Show>
            <p class="reveal-outcome">
              <strong>{outcomeText[revealed().outcome]}</strong>
              <Show when={revealed().outcome !== "annulled" && keyLetter()}>
                {(letter) => <> Gabarito: {letter()}.</>}
              </Show>
            </p>
            <Show
              when={revealed().explanation.trim()}
              fallback={
                <div class="ai-panel">
                  <p class="muted">Esta questão não tem explicação.</p>
                  <button type="button" disabled={explaining()} onClick={() => void explain()}>
                    {explaining() ? "Escrevendo a explicação…" : "Explicar"}
                  </button>
                  <Show when={explaining()}>
                    <button type="button" onClick={() => props.onAiCancel()}>
                      Cancelar
                    </button>
                  </Show>
                </div>
              }
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
            <RecheckPanel
              question={props.question}
              onAsk={props.onRecheck}
              onApply={props.onRecheckApply}
              onDismiss={props.onRecheckDismiss}
              onCancel={props.onAiCancel}
            />
            <div class="reveal-actions">
              <button type="button" onClick={() => props.onAnnul(!revealed().annulled)}>
                {revealed().annulled ? "Desfazer anulação" : "Anular questão"}
              </button>
              <span class="muted">
                {revealed().annulled
                  ? "Fora de todas as pontuações, inclusive das tentativas anteriores."
                  : "Use se a questão estiver errada ou ambígua: ela sai de todas as pontuações."}
              </span>
            </div>
          </section>
        )}
      </Show>
    </article>
  );
}
