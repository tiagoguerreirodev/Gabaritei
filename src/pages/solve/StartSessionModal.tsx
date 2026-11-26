import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { TopicId } from "../../bindings/TopicId";
import type { SessionSource } from "../../bindings/SessionSource";
import { ResourceView } from "../../components/states";
import Modal from "../../components/Modal";
import { showError } from "../../components/toasts";
import { feedbackModes, modeHint, modeLabel } from "../../labels";
import { openAttempt } from "../../navigation";

/**
 * Chooses the feedback mode (and, for a topic, how many questions) and starts a session.
 * `source` with kind `topic` takes its count from the form.
 */
export default function StartSessionModal(props: {
  title: string;
  source: Exclude<SessionSource, { kind: "topic" }> | { kind: "topic"; topicId: TopicId };
  /** Questions available, for the topic count limit. */
  available?: number;
  /** Cancel: closes this dialog. */
  onClose: () => void;
  /** Called once the session started, before opening it. Defaults to `onClose`. */
  onStarted?: () => void;
}) {
  const [mode, setMode] = createSignal<FeedbackMode>("study");
  const [limits, { refetch }] = createResource(
    () => (props.source.kind === "topic" ? { available: props.available ?? 0 } : false),
    ({ available }) => api.questionCountLimits({ kind: "topic", available }),
  );
  const [enteredCount, setCount] = createSignal<number>();
  const count = () => enteredCount() ?? limits()?.initial ?? 0;
  const [busy, setBusy] = createSignal(false);
  const countValid = () => {
    const range = !limits.error && limits();
    return range && Number.isInteger(count()) && count() >= range.min && count() <= range.max;
  };

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
          <button type="button" class="btn" onClick={() => props.onClose()}>
            Cancelar
          </button>
          <button
            type="button"
            class="btn btn-primary"
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
        <For each={feedbackModes}>
          {(m) => (
            <label>
              <input
                type="radio"
                name="feedback-mode"
                value={m}
                checked={mode() === m}
                onChange={() => setMode(m)}
              />
              <span>
                <strong>{modeLabel[m]}</strong>
                <span class="muted"> — {modeHint[m]}</span>
              </span>
            </label>
          )}
        </For>
      </fieldset>
      <Show when={props.source.kind === "topic"}>
        <ResourceView
          value={limits()}
          error={() => limits.error}
          pending={limits.loading}
          retrying={limits.loading}
          onRetry={() => refetch()}
        >
          {(range) => (
            <label class="count-field">
              Quantidade de questões
              <input
                class="input"
                type="number"
                min={range().min}
                max={range().max}
                step="1"
                value={count()}
                aria-invalid={!countValid()}
                onInput={(event) => setCount(Number(event.currentTarget.value))}
              />
              <span class="muted">
                Um número inteiro de {range().min} a {range().max}. Primeiro as inéditas, depois as
                respondidas há mais tempo.
              </span>
            </label>
          )}
        </ResourceView>
      </Show>
    </Modal>
  );
}
