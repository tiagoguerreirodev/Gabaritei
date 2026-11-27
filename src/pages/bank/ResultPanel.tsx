import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { QuestionFilter } from "../../bindings/QuestionFilter";
import type { SelectionSummary } from "../../bindings/SelectionSummary";
import type { SessionOrder } from "../../bindings/SessionOrder";
import type { SessionSource } from "../../bindings/SessionSource";
import { InlineError, errorMessage } from "../../components/states";
import { formatNumber, questionCount } from "../../format";
import {
  feedbackModes,
  modeHint,
  modeLabel,
  sessionOrderLabels,
  studyStatusLabels,
} from "../../labels";
import { openAttempt } from "../../navigation";

/** What the filters match (counts only) and the settings to start a session from it. */
export default function ResultPanel(props: {
  summary: SelectionSummary;
  filter: QuestionFilter;
  order: SessionOrder | null;
  mode: FeedbackMode;
  onOrder: (order: SessionOrder) => void;
  onMode: (mode: FeedbackMode) => void;
}) {
  const [wholeCollection, setWholeCollection] = createSignal(true);
  const [enteredCount, setEnteredCount] = createSignal<number>();
  const [busy, setBusy] = createSignal(false);
  const [startError, setStartError] = createSignal<string>();

  const available = () => props.summary.matching - props.summary.inOpenAttempt;
  const onlyCollection = () => {
    const { topicIds, statuses, collectionId, attemptId } = props.filter;
    return collectionId !== null && topicIds.length === 0 && statuses.length === 0 && !attemptId;
  };
  const startsWholeCollection = () => onlyCollection() && wholeCollection();
  const order = (): SessionOrder | undefined => {
    const allowed = props.summary.allowedOrders;
    return props.order && allowed.includes(props.order) ? props.order : allowed[0];
  };

  const [limits] = createResource(available, (count) =>
    count > 0 ? api.questionCountLimits({ kind: "selection", available: count }) : undefined,
  );
  const [openAttemptId] = createResource(
    () => (startsWholeCollection() ? props.filter.collectionId : null),
    async (id) => (await api.collection(id)).attempts.find((a) => a.finishedAt === null)?.id,
  );

  const count = () => enteredCount() ?? limits()?.initial ?? 0;
  const countValid = () => {
    const range = limits.error ? undefined : limits();
    return !!range && Number.isInteger(count()) && count() >= range.min && count() <= range.max;
  };
  const continuing = () => (startsWholeCollection() ? openAttemptId() : undefined);
  const blockedReason = () => {
    if (continuing()) return undefined;
    if (props.summary.matching === 0) return "Nenhuma questão com estes filtros.";
    if (available() === 0) return "Todas estas questões estão em uma tentativa aberta.";
    return undefined;
  };
  const canStart = () =>
    !busy() && !blockedReason() && (startsWholeCollection() || (countValid() && !!order()));

  const source = (): SessionSource | undefined => {
    const { collectionId } = props.filter;
    if (startsWholeCollection() && collectionId) return { kind: "collection", collectionId };
    const chosenOrder = order();
    return chosenOrder
      ? { kind: "filter", filter: props.filter, count: count(), order: chosenOrder }
      : undefined;
  };

  const start = async () => {
    setStartError(undefined);
    const existing = continuing();
    if (existing) return openAttempt(existing);
    const chosen = source();
    if (!chosen) return;
    setBusy(true);
    try {
      const view = await api.sessionStart({ source: chosen, feedbackMode: props.mode });
      openAttempt(view.id);
    } catch (error) {
      setStartError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="card result-panel" aria-label="Resultado">
      <div>
        <p class="result-count" aria-live="polite">
          {questionCount(props.summary.matching)}
        </p>
        <Show when={props.summary.inOpenAttempt > 0}>
          <p class="result-note">
            {formatNumber(props.summary.inOpenAttempt)} em tentativa aberta ficam de fora.
          </p>
        </Show>
      </div>
      <ul class="breakdown" aria-label="Por área e status">
        <For each={props.summary.byArea}>
          {(area) => (
            <li class="card card-compact stat">
              <span class="stat-name">{area.area}</span>
              <span class="stat-value">{formatNumber(area.questions)}</span>
            </li>
          )}
        </For>
        <For each={props.summary.byStatus}>
          {(status) => (
            <li class="card card-compact stat">
              <span class="stat-name">{studyStatusLabels[status.status]}</span>
              <span class="stat-value">{formatNumber(status.questions)}</span>
            </li>
          )}
        </For>
      </ul>
      <form
        class="result-settings"
        onSubmit={(event) => {
          event.preventDefault();
          if (canStart() || continuing()) void start();
        }}
      >
        <Show when={onlyCollection()}>
          <fieldset class="radio-row">
            <legend>Questões</legend>
            <label>
              <input
                type="radio"
                name="collection-scope"
                checked={wholeCollection()}
                onChange={() => setWholeCollection(true)}
              />
              Todas as questões do caderno
            </label>
            <label>
              <input
                type="radio"
                name="collection-scope"
                checked={!wholeCollection()}
                onChange={() => setWholeCollection(false)}
              />
              Escolher quantas
            </label>
          </fieldset>
        </Show>
        <Show when={!startsWholeCollection() && available() > 0}>
          <div class="form-row">
            <label class="field">
              <span class="field-label">Quantas</span>
              <input
                class="input"
                type="number"
                min={limits()?.min}
                max={limits()?.max}
                step="1"
                value={count()}
                aria-invalid={!countValid()}
                onInput={(event) => setEnteredCount(Number(event.currentTarget.value))}
              />
              <Show when={limits()}>
                {(range) => (
                  <span class="field-hint">
                    De {range().min} a {range().max}.
                  </span>
                )}
              </Show>
            </label>
            <label class="field">
              <span class="field-label">Ordem</span>
              <select
                class="input"
                value={order()}
                onChange={(event) => props.onOrder(event.currentTarget.value as SessionOrder)}
              >
                <For each={props.summary.allowedOrders}>
                  {(allowed) => <option value={allowed}>{sessionOrderLabels[allowed]}</option>}
                </For>
              </select>
            </label>
          </div>
        </Show>
        <fieldset class="radio-row">
          <legend>Modo</legend>
          <For each={feedbackModes}>
            {(mode) => (
              <label title={modeHint[mode]}>
                <input
                  type="radio"
                  name="feedback-mode"
                  checked={props.mode === mode}
                  onChange={() => props.onMode(mode)}
                />
                {modeLabel[mode]}
              </label>
            )}
          </For>
        </fieldset>
        <Show when={blockedReason()}>{(reason) => <p class="result-note">{reason()}</p>}</Show>
        <Show when={startError()}>{(message) => <InlineError>{message()}</InlineError>}</Show>
        <div class="result-actions">
          <button type="submit" class="btn btn-primary" disabled={!canStart() && !continuing()}>
            {continuing() ? "Continuar" : "Resolver estas"}
          </button>
        </div>
      </form>
    </section>
  );
}
