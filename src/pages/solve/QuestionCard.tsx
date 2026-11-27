import { For, Show, createEffect, createSignal, on, onCleanup, type JSX } from "solid-js";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { Outcome } from "../../bindings/Outcome";
import type { RecheckView } from "../../bindings/RecheckView";
import type { ResponseView } from "../../bindings/ResponseView";
import QuestionImage from "../../components/QuestionImage";
import RichText from "../../components/RichText";
import { optionLetters } from "../../format";
import { outcomeText, questionTypeLabel } from "../../labels";
import RecheckPanel from "./RecheckPanel";
import type { OptionAction } from "./useResolverShortcuts";
import { showError } from "../../components/toasts";

const ANSWER_POP_MS = 600;

/**
 * One question as a sheet of the answer booklet: stem, image, options with eliminate, flag, and, once revealed, the key and
 * explanation. Everything about the answer comes from `question.reveal`, which the backend
 * only fills in after the question is revealed.
 */
export default function QuestionCard(props: {
  question: ResponseView;
  number: number;
  total: number;
  feedbackMode: FeedbackMode;
  finishedAt: string | null;
  /** `X` changes the next shortcut from answering to eliminating. */
  optionAction: OptionAction;
  /** Estudo: the option marked but not yet confirmed. */
  pending: string | null;
  /** Chooses an option: answers it, or only marks it when confirmation is required. */
  onAnswer: (optionId: string) => void;
  onConfirm: () => void;
  onEliminate: (optionId: string) => void;
  onFlag: () => void;
  /** Annuls the revealed question. */
  onAnnul: () => void;
  /** Restores an annulled question. */
  onUnannul: () => void;
  /** Asks the AI to re-check the key; resolves with its proposal. */
  onRecheck: () => Promise<RecheckView>;
  onRecheckApply: () => void;
  onRecheckDismiss: () => void;
  onAiCancel: () => void;
  /** Asks the AI to write the missing explanation. */
  onExplain: () => Promise<void>;
  /** The sheet footer: previous and next, and the shortcut help. */
  children?: JSX.Element;
}) {
  const finished = () => props.finishedAt !== null;
  const locked = () => finished() || props.question.reveal !== null;
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
        if (previous && previous[0] === position && previous[1] && !hidden && !finished()) {
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
  // An annulled question is out of every score, so its options carry no right or wrong marks.
  const outcomeShown = () => {
    const revealed = reveal();
    return revealed !== null && revealed.outcome !== "annulled";
  };
  const keyLetter = () => {
    const key = reveal()?.answerId;
    const index = props.question.options.findIndex((o) => o.id === key);
    return index >= 0 ? letters()[index] : undefined;
  };

  return (
    <article class="sheet" aria-label={`Questão ${props.number} de ${props.total}`}>
      <header class="sheet-header">
        <span class="label">
          Questão {props.number} de {props.total}
        </span>
        <span class="sheet-header-meta">
          <span class="label muted">{questionTypeLabel[props.question.type]}</span>
          <button
            type="button"
            class="btn btn-ghost"
            aria-pressed={props.question.flagged}
            disabled={finished()}
            onClick={() => props.onFlag()}
            title="Marcar para revisar (F)"
          >
            <span aria-hidden="true">⚑ </span>
            {props.question.flagged ? "Marcada" : "Marcar"}
          </button>
        </span>
      </header>
      <span class="sheet-number" aria-hidden="true">
        {props.number}
      </span>
      <RichText class="sheet-stem" text={props.question.stem} />
      <Show when={props.question.image}>
        {(image) => (
          <QuestionImage name={image()} alt={props.question.imageAlt ?? "Figura de apoio"} />
        )}
      </Show>
      <Show when={props.optionAction === "eliminate"}>
        <p class="field-hint" role="status">
          Escolha a letra da alternativa a eliminar.
        </p>
      </Show>
      <ol class="options">
        <For each={props.question.options}>
          {(option, index) => {
            const letter = () => letters()[index()];
            // A mark that was never confirmed is not an answer once the question is locked.
            const marked = () => !locked() && props.pending === option.id;
            const selected = () => props.question.choice === option.id || marked();
            const eliminated = () => props.question.eliminated.includes(option.id);
            const isKey = () => outcomeShown() && reveal()?.answerId === option.id;
            const isWrong = () => outcomeShown() && selected() && !isKey();
            return (
              <li>
                <button
                  type="button"
                  classList={{
                    option: true,
                    "is-selected": selected() && !outcomeShown(),
                    "is-eliminated": eliminated(),
                    "is-correct": isKey(),
                    "is-wrong": isWrong(),
                  }}
                  data-option-id={option.id}
                  aria-pressed={selected()}
                  disabled={locked()}
                  onClick={() => props.onAnswer(option.id)}
                >
                  <span
                    classList={{
                      bubble: true,
                      "is-filled": selected(),
                      "is-correct": isKey(),
                      "is-wrong": isWrong(),
                      "is-key": isKey() && !selected(),
                      // Confirmed right answer: the bubble fills with a small pop.
                      "is-popping": popping() && selected() && isKey(),
                    }}
                  >
                    {letter()}
                  </span>
                  <RichText class="option-text" text={option.text} />
                  <Show when={isKey()}>
                    <span class="option-note">
                      <span aria-hidden="true">✓ </span>Gabarito
                    </span>
                  </Show>
                  <Show when={isWrong()}>
                    <span class="option-note">
                      <span aria-hidden="true">✗ </span>Sua resposta
                    </span>
                  </Show>
                </button>
                <button
                  type="button"
                  class="btn btn-ghost option-eliminate"
                  aria-label={`${eliminated() ? "Restaurar" : "Eliminar"} alternativa ${letter()}`}
                  aria-pressed={eliminated()}
                  disabled={locked()}
                  onClick={() => props.onEliminate(option.id)}
                >
                  ✕
                </button>
              </li>
            );
          }}
        </For>
      </ol>
      <Show when={props.feedbackMode === "study" && !locked()}>
        <div class="confirm-row">
          <button
            type="button"
            class="btn btn-primary"
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
            class="explanation"
            aria-label="Gabarito e explicação"
          >
            <Show when={revealed().disputed}>
              <p class="badge badge-danger disputed-badge" role="note">
                Gabarito contestado na conferência: a IA conferente discordou da resposta. Se algo
                parecer errado, use “Pedir revisão à IA” ou “Anular questão”.
              </p>
            </Show>
            <section>
              <span class="label">Gabarito</span>
              <p
                classList={{
                  "explanation-verdict": true,
                  "is-correct": revealed().outcome === "correct",
                  "is-wrong": revealed().outcome === "wrong",
                }}
              >
                <Show when={verdictGlyph(revealed().outcome)}>
                  {(glyph) => <span aria-hidden="true">{glyph()}</span>}
                </Show>
                <span>
                  {outcomeText[revealed().outcome]}
                  <Show when={revealed().outcome !== "annulled" && keyLetter()}>
                    {(letter) => <> Gabarito: {letter()}.</>}
                  </Show>
                </span>
              </p>
            </section>
            <Show
              when={revealed().explanation.trim()}
              fallback={
                <div class="ai-panel">
                  <p class="muted">Esta questão não tem explicação.</p>
                  <button
                    type="button"
                    class="btn"
                    disabled={explaining()}
                    onClick={() => void explain()}
                  >
                    {explaining() ? "Escrevendo a explicação…" : "Explicar"}
                  </button>
                  <Show when={explaining()}>
                    <button type="button" class="btn" onClick={() => props.onAiCancel()}>
                      Cancelar
                    </button>
                  </Show>
                </div>
              }
            >
              <section>
                <span class="label">Explicação</span>
                <RichText class="explanation-body" text={revealed().explanation} />
              </section>
            </Show>
            <Show when={revealed().refs.length > 0}>
              <section>
                <span class="label">Referências</span>
                <ul class="explanation-refs">
                  <For each={revealed().refs}>{(ref) => <li>{ref}</li>}</For>
                </ul>
              </section>
            </Show>
            <RecheckPanel
              question={props.question}
              onAsk={props.onRecheck}
              onApply={props.onRecheckApply}
              onDismiss={props.onRecheckDismiss}
              onCancel={props.onAiCancel}
            />
            <div class="explanation-actions">
              <button
                type="button"
                class="btn"
                onClick={() => (revealed().annulled ? props.onUnannul() : props.onAnnul())}
              >
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
      <footer class="sheet-footer">{props.children}</footer>
    </article>
  );
}

const verdictGlyph = (outcome: Outcome) =>
  outcome === "correct" ? "✓" : outcome === "wrong" ? "✗" : undefined;
