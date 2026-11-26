import { Show } from "solid-js";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import { shortcutsEnabled } from "../../shortcuts";

export default function ShortcutHelp(props: {
  feedbackMode: FeedbackMode;
  open: boolean;
  panelRef: (element: HTMLDivElement) => void;
  toggleRef: (element: HTMLButtonElement) => void;
  onToggle: () => void;
}) {
  return (
    <div class="shortcuts" ref={props.panelRef}>
      <button
        type="button"
        class="btn"
        ref={props.toggleRef}
        aria-expanded={props.open}
        aria-controls="shortcuts-help"
        onClick={() => props.onToggle()}
      >
        Atalhos
      </button>
      <div
        id="shortcuts-help"
        class="shortcuts-panel"
        role="group"
        aria-label="Atalhos de teclado"
        hidden={!props.open}
      >
        <Show
          when={shortcutsEnabled()}
          fallback={<p>Os atalhos de uma tecla estão desativados em Configurações.</p>}
        >
          <ul>
            <li>
              <kbd>A</kbd>–<kbd>E</kbd> (ou <kbd>C</kbd>/<kbd>E</kbd>){" "}
              {props.feedbackMode === "study" ? "escolhe" : "responde"}
            </li>
            <Show when={props.feedbackMode === "study"}>
              <li>
                <kbd>Enter</kbd> confirma
              </li>
            </Show>
            <li>
              <kbd>X</kbd> e uma letra elimina
            </li>
            <li>
              <kbd>F</kbd> marca para revisar
            </li>
          </ul>
        </Show>
        <p>
          <kbd>←</kbd> <kbd>→</kbd> navegam entre as questões.
        </p>
      </div>
    </div>
  );
}
