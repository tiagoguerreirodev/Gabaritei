import { For, Match, Show, Switch, createEffect, createSignal, on } from "solid-js";
import type { RecheckView } from "../../bindings/RecheckView";
import type { ResponseView } from "../../bindings/ResponseView";
import { errorMessage } from "../../components/states";
import { optionLetters } from "./format";

type State =
  | { step: "idle" }
  | { step: "asking" }
  | { step: "done"; result: RecheckView }
  | { step: "error"; message: string };

/**
 * "Pedir revisão à IA" for a revealed question. The AI's answer is only a proposal: the key
 * changes only if the user confirms.
 */
export default function RecheckPanel(props: {
  question: ResponseView;
  /** Asks the AI. Resolves with its proposal. */
  onAsk: () => Promise<RecheckView>;
  /** The user confirmed the proposed key. */
  onApply: () => void;
  /** The user kept the current key. */
  onDismiss: () => void;
  onCancel: () => void;
}) {
  const [state, setState] = createSignal<State>({ step: "idle" });
  // A new question starts clean.
  createEffect(
    on(
      () => props.question.position,
      () => setState({ step: "idle" }),
      { defer: true },
    ),
  );

  const ask = async () => {
    // The card stays mounted when the user moves to another question: a late answer for the
    // question that was asked must not show up under the one on screen now.
    const asked = props.question.position;
    setState({ step: "asking" });
    try {
      const result = await props.onAsk();
      if (props.question.position === asked) setState({ step: "done", result });
    } catch (error) {
      if (props.question.position === asked) {
        setState({ step: "error", message: errorMessage(error) });
      }
    }
  };

  const letter = (optionId: string | null) => {
    const index = props.question.options.findIndex((o) => o.id === optionId);
    return index >= 0 ? optionLetters(props.question)[index] : undefined;
  };

  const verdictText = {
    correct: "A IA confirmou o gabarito.",
    wrong: "A IA discorda do gabarito.",
    ambiguous: "A IA considera a questão ambígua.",
  } as const;

  return (
    <div class="ai-panel">
      <Switch>
        <Match when={state().step === "idle" || state().step === "error"}>
          <button type="button" onClick={() => void ask()}>
            Pedir revisão à IA
          </button>
          <Show when={state().step === "error"}>
            <p class="error" role="alert">
              {(state() as { step: "error"; message: string }).message}
            </p>
          </Show>
        </Match>
        <Match when={state().step === "asking"}>
          <p role="status">Consultando a IA… isto pode levar alguns minutos.</p>
          <button type="button" onClick={() => props.onCancel()}>
            Cancelar
          </button>
        </Match>
        <Match when={state().step === "done"}>
          {(() => {
            const result = () => (state() as { step: "done"; result: RecheckView }).result;
            return (
              <section aria-label="Revisão da IA">
                <p>
                  <strong>{verdictText[result().verdict]}</strong>
                </p>
                <Show when={result().correctedKey}>
                  {(key) => (
                    <p>
                      Gabarito sugerido: <strong>{letter(key())}</strong>.
                    </p>
                  )}
                </Show>
                <p>{result().reasoning}</p>
                <Show when={result().sources.length > 0}>
                  <h4>Fontes</h4>
                  <ul class="refs">
                    <For each={result().sources}>{(source) => <li>{source}</li>}</For>
                  </ul>
                </Show>
                <Show
                  when={result().correctedKey}
                  fallback={
                    <>
                      <Show when={result().verdict === "ambiguous"}>
                        <p class="muted">
                          Se a questão estiver mal formulada, use “Anular questão”.
                        </p>
                      </Show>
                      <button
                        type="button"
                        onClick={() => {
                          props.onDismiss();
                          setState({ step: "idle" });
                        }}
                      >
                        Fechar
                      </button>
                    </>
                  }
                >
                  {(key) => (
                    <div class="reveal-actions">
                      <button
                        type="button"
                        class="primary"
                        onClick={() => {
                          props.onApply();
                          setState({ step: "idle" });
                        }}
                      >
                        Aplicar novo gabarito ({letter(key())})
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          props.onDismiss();
                          setState({ step: "idle" });
                        }}
                      >
                        Manter o gabarito atual
                      </button>
                      <span class="muted">
                        A troca vale para todas as tentativas, inclusive as anteriores.
                      </span>
                    </div>
                  )}
                </Show>
              </section>
            );
          })()}
        </Match>
      </Switch>
    </div>
  );
}
