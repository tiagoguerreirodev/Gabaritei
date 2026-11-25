import { For, Show, createSignal } from "solid-js";
import { api } from "../../api";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { SessionSource } from "../../bindings/SessionSource";
import Modal from "../../components/Modal";
import { showError } from "../../components/toasts";
import { openAttempt } from "../../navigation";

const modes: { id: FeedbackMode; label: string; hint: string }[] = [
  {
    id: "study",
    label: "Estudo",
    hint: "Mostra o gabarito e a explicação logo após cada resposta.",
  },
  { id: "exam", label: "Simulado", hint: "Mostra o gabarito e as explicações só ao finalizar." },
];

/** Largest topic session the backend accepts. */
export const MAX_TOPIC_SESSION = 200;

/**
 * Chooses the feedback mode (and, for a topic, how many questions) and starts a session.
 * `source` with kind `topic` takes its count from the form.
 */
export default function StartSessionModal(props: {
  title: string;
  source: SessionSource;
  /** Questions available, for the topic count limit. */
  available?: number;
  /** Cancel: closes this dialog. */
  onClose: () => void;
  /** Called once the session started, before opening it. Defaults to `onClose`. */
  onStarted?: () => void;
}) {
  const [mode, setMode] = createSignal<FeedbackMode>("study");
  const max = () => Math.min(MAX_TOPIC_SESSION, props.available ?? MAX_TOPIC_SESSION);
  const [count, setCount] = createSignal(Math.min(10, max()));
  const [busy, setBusy] = createSignal(false);
  const countValid = () => Number.isInteger(count()) && count() >= 1 && count() <= max();

  const start = async () => {
    const source: SessionSource =
      props.source.kind === "topic" ? { ...props.source, count: count() } : props.source;
    setBusy(true);
    try {
      const view = await api.sessionStart({ source, feedbackMode: mode() });
      (props.onStarted ?? props.onClose)();
      openAttempt(view.id);
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      title={props.title}
      onClose={() => props.onClose()}
      footer={
        <>
          <button type="button" onClick={() => props.onClose()}>
            Cancelar
          </button>
          <button
            type="button"
            class="primary"
            disabled={busy() || (props.source.kind === "topic" && !countValid())}
            onClick={() => void start()}
          >
            Começar
          </button>
        </>
      }
    >
      <fieldset class="mode-choice">
        <legend>Modo</legend>
        <For each={modes}>
          {(m) => (
            <label>
              <input
                type="radio"
                name="feedback-mode"
                value={m.id}
                checked={mode() === m.id}
                onChange={() => setMode(m.id)}
              />
              <span>
                <strong>{m.label}</strong>
                <span class="muted"> — {m.hint}</span>
              </span>
            </label>
          )}
        </For>
      </fieldset>
      <Show when={props.source.kind === "topic"}>
        <label class="count-field">
          Quantidade de questões
          <input
            type="number"
            min="1"
            max={max()}
            step="1"
            value={count()}
            aria-invalid={!countValid()}
            onInput={(event) => setCount(Number(event.currentTarget.value))}
          />
          <span class="muted">
            Um número inteiro de 1 a {max()}. Primeiro as inéditas, depois as respondidas há mais
            tempo.
          </span>
        </label>
      </Show>
    </Modal>
  );
}
