import { Show, createEffect, createMemo, createResource, createSignal, on } from "solid-js";
import { api } from "../../api";
import type { AttemptView } from "../../bindings/AttemptView";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import { Loading, ResourceView } from "../../components/states";
import { showError, showSuccess, showToast } from "../../components/toasts";
import { modeLabel } from "../../labels";
import { focusMode, toggleFocusMode, useFocusMode } from "../../focusMode";
import Navigator from "./Navigator";
import QuestionCard from "./QuestionCard";
import ResolverDialogs, { type ResolverDialog } from "./ResolverDialogs";
import ScoreSummary from "./ScoreSummary";
import ShortcutHelp from "./ShortcutHelp";
import { mergeExplanation } from "./mergeExplanation";
import { playQuestionTransition } from "./questionTransition";
import useResolverShortcuts from "./useResolverShortcuts";

/** The solve screen for one attempt (spec §3.3). */
/**
 * One primary action per screen: in Estudo mode it is "Confirmar resposta" on the question card,
 * so finishing the attempt steps back to a secondary button.
 */
const finishButtonClass: Record<FeedbackMode, string> = {
  study: "btn",
  exam: "btn btn-primary",
};

export default function Resolver(props: { attemptId: string; onLeave: () => void }) {
  const [attempt, { mutate, refetch }] = createResource(
    () => props.attemptId,
    (id) => api.attempt(id),
  );
  const loadError = () => attempt.error as unknown;
  const view = () => {
    if (attempt.loading || loadError()) return undefined;
    const loaded = attempt.latest;
    return loaded?.id === props.attemptId ? loaded : undefined;
  };
  const [index, setIndex] = createSignal(0);
  const [dialog, setDialog] = createSignal<ResolverDialog>(null);
  // Estudo: the option marked on this question but not yet confirmed with Enter.
  const [pending, setPending] = createSignal<string | null>(null);
  // The confirmed answer is on its way to the backend: no new mark or confirmation until it lands.
  const [sending, setSending] = createSignal(false);
  // Text for the polite live region: the question number on navigation, the outcome on answers.
  const [announcement, setAnnouncement] = createSignal("");
  // A newly loaded attempt resumes at the first unanswered question.
  const loadedId = createMemo(() => view()?.id);
  createEffect(
    on(loadedId, () => {
      const loaded = view();
      const next = loaded?.questions.findIndex((q) => q.choice === null) ?? -1;
      setIndex(loaded?.finishedAt === null && next >= 0 ? next : 0);
      setPending(null);
    }),
  );

  // Changes run one after another, so `finish` always quotes the latest revision.
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = (change: (id: string) => Promise<AttemptView>) => {
    const id = view()?.id;
    if (!id) return;
    queue = queue.then(() =>
      change(id).then((next) => {
        if (next.id === props.attemptId) mutate(next);
      }, showError),
    );
  };

  const finished = () => view()?.finishedAt != null;
  const current = () => view()?.questions[index()];
  const locked = () => finished() || current()?.reveal != null;
  const total = () => view()?.questions.length ?? 0;
  const answered = () => view()?.questions.filter((q) => q.choice !== null).length ?? 0;
  useFocusMode(() => view() !== undefined && !finished());

  let stage: HTMLDivElement | undefined;
  createEffect(
    on(
      () => current()?.position,
      (position, previous) => {
        if (stage && position !== undefined && previous !== undefined && position !== previous) {
          playQuestionTransition(stage, position - previous);
        }
      },
    ),
  );

  // Only the question number is announced here. A confirmed answer hands the focus to the reveal
  // section, which reads the outcome; annulling and key changes have their own toast. Finishing
  // is announced by `finish()`.
  createEffect(
    on(
      () => current()?.position,
      (position, previous) => {
        if (position === undefined || previous === undefined) return;
        setAnnouncement(`Questão ${position + 1} de ${total()}.`);
      },
    ),
  );

  // Any action other than the letter after `X` cancels the pending elimination.
  const select = (i: number) => {
    shortcuts.resetOptionAction();
    setPending(null);
    setIndex(i);
  };
  const move = (delta: number) =>
    select(Math.min(Math.max(index() + delta, 0), Math.max(total() - 1, 0)));

  const answer = (optionId: string) => {
    const q = current();
    shortcuts.resetOptionAction();
    if (!q || locked()) return;
    // In Simulado, choosing the selected option again clears it.
    const choice = q.choice === optionId && view()?.feedbackMode === "exam" ? null : optionId;
    enqueue((id) => api.answer(id, q.position, choice));
  };
  /** Estudo locks and reveals on the first answer, so a choice is only marked until the user
   * confirms it. Simulado saves it straight away: it can still be changed or cleared. */
  const choose = (optionId: string) => {
    const q = current();
    shortcuts.resetOptionAction();
    if (!q || locked() || sending()) return;
    if (view()?.feedbackMode === "study") setPending(optionId);
    else answer(optionId);
  };
  const confirm = () => {
    const optionId = pending();
    if (optionId === null || locked() || sending()) return;
    setPending(null);
    setSending(true);
    answer(optionId);
    void queue.then(() => setSending(false));
  };
  const eliminate = (optionId: string) => {
    const q = current();
    shortcuts.resetOptionAction();
    if (!q || locked()) return;
    if (pending() === optionId) setPending(null);
    enqueue((id) => api.eliminate(id, q.position, optionId));
  };
  const flag = () => {
    const q = current();
    shortcuts.resetOptionAction();
    if (!q || finished()) return;
    enqueue((id) => api.flag(id, q.position, !q.flagged));
  };

  const updateAnnulment = (change: typeof api.annul, message: string) => {
    const q = current();
    if (!q?.reveal) return;
    enqueue(async (id) => {
      const next = await change(id, q.position);
      showToast(message);
      return next;
    });
  };
  const annul = () => updateAnnulment(api.annul, "Questão anulada.");
  const unannul = () => updateAnnulment(api.unannul, "Anulação desfeita.");

  const recheck = () => {
    const q = current();
    const latest = view();
    if (!q?.reveal || !latest) return Promise.reject(new Error("Questão não revelada."));
    return api.recheckAsk(latest.id, q.position);
  };

  const applyRecheck = () => {
    const q = current();
    if (!q?.reveal) return;
    enqueue(async (id) => {
      const next = await api.recheckApply(id, q.position);
      showSuccess("Gabarito atualizado. As pontuações foram recalculadas.");
      return next;
    });
  };

  /** Asks the AI for the missing explanation and merges only that question's answer into the
   * view, so answers given in the meantime aren't undone. */
  const explain = async () => {
    const q = current();
    const latest = view();
    if (!q?.reveal || !latest) return;
    const next = await api.explainAsk(latest.id, q.position);
    const currentView = view();
    const merged = mergeExplanation(currentView, next, q.position);
    if (merged === currentView) return;
    mutate(merged);
    showSuccess("Explicação adicionada à questão.");
  };

  const dismissRecheck = () => {
    const q = current();
    const latest = view();
    if (q?.reveal && latest) void api.recheckDismiss(latest.id, q.position).catch(showError);
  };

  const finish = async () => {
    await queue;
    const latest = view();
    if (!latest) return;
    try {
      const next = await api.finish(latest.id, latest.revision);
      if (next.id !== props.attemptId) return;
      mutate(next);
      setDialog(null);
      setPending(null);
      setIndex(0);
      setAnnouncement("Tentativa finalizada.");
    } catch (error) {
      showError(error);
    }
  };
  const abandon = async () => {
    await queue;
    const latest = view();
    if (!latest) return;
    try {
      await api.abandon(latest.id);
      setDialog(null);
      props.onLeave();
    } catch (error) {
      showError(error);
    }
  };

  const shortcuts = useResolverShortcuts({
    dialogOpen: () => dialog() !== null,
    question: current,
    finished,
    locked,
    feedbackMode: () => view()?.feedbackMode,
    pending,
    move,
    choose,
    confirm,
    eliminate,
    flag,
  });

  return (
    <>
      <div class="sr-only" aria-live="polite" aria-atomic="true" data-testid="live-region">
        {announcement()}
      </div>
      <Show when={!view()}>
        <h1 id="resolver-title">Resolver</h1>
      </Show>
      <ResourceView
        value={view()}
        error={loadError}
        pending={!view()}
        retrying={attempt.loading}
        onRetry={() => refetch()}
        describedBy="resolver-title"
        placeholder={<Loading label="Carregando tentativa…" />}
      >
        {(attempt) => (
          <div class="resolver">
            <header class="resolver-header">
              <div>
                <h1>{attempt().title}</h1>
                <p class="muted">
                  <span class="badge">{modeLabel[attempt().feedbackMode]}</span>{" "}
                  <Show when={finished()}> · finalizada</Show>
                </p>
              </div>
              <div class="resolver-actions">
                <Show
                  when={!finished()}
                  fallback={
                    <button type="button" class="btn" onClick={() => props.onLeave()}>
                      Voltar ao Banco
                    </button>
                  }
                >
                  <button
                    type="button"
                    class="btn"
                    aria-pressed={focusMode()}
                    title={focusMode() ? "Mostrar o menu lateral" : "Esconder o menu lateral"}
                    onClick={toggleFocusMode}
                  >
                    Modo foco
                  </button>
                  <button type="button" class="btn" onClick={() => setDialog("abandon")}>
                    Abandonar
                  </button>
                  <button
                    type="button"
                    class={finishButtonClass[attempt().feedbackMode]}
                    onClick={() => setDialog("finish")}
                  >
                    Finalizar
                  </button>
                </Show>
              </div>
            </header>
            <Show when={attempt().score}>
              {(score) => <ScoreSummary attemptId={attempt().id} score={score()} />}
            </Show>
            <div class="solve-layout">
              <div class="question-stage" ref={stage}>
                <Show when={current()}>
                  {(q) => (
                    <QuestionCard
                      question={q()}
                      number={index() + 1}
                      total={total()}
                      feedbackMode={attempt().feedbackMode}
                      finishedAt={attempt().finishedAt}
                      optionAction={shortcuts.optionAction()}
                      pending={pending()}
                      onAnswer={choose}
                      onConfirm={confirm}
                      onEliminate={eliminate}
                      onFlag={flag}
                      onAnnul={annul}
                      onUnannul={unannul}
                      onExplain={explain}
                      onRecheck={recheck}
                      onRecheckApply={applyRecheck}
                      onRecheckDismiss={dismissRecheck}
                      onAiCancel={() => void api.aiRequestCancel().catch(showError)}
                    >
                      <button
                        type="button"
                        class="btn"
                        disabled={index() === 0}
                        onClick={() => move(-1)}
                      >
                        ← Anterior
                      </button>
                      <ShortcutHelp
                        feedbackMode={attempt().feedbackMode}
                        open={shortcuts.helpOpen()}
                        panelRef={shortcuts.panelRef}
                        toggleRef={shortcuts.toggleRef}
                        onToggle={shortcuts.toggleHelp}
                      />
                      <button
                        type="button"
                        class="btn"
                        disabled={index() >= total() - 1}
                        onClick={() => move(1)}
                      >
                        Próxima →
                      </button>
                    </QuestionCard>
                  )}
                </Show>
              </div>
              <Navigator questions={attempt().questions} current={index()} onSelect={select} />
            </div>
            <ResolverDialogs
              dialog={dialog()}
              answered={answered()}
              total={total()}
              onClose={() => setDialog(null)}
              onFinish={() => void finish()}
              onAbandon={() => void abandon()}
            />
          </div>
        )}
      </ResourceView>
    </>
  );
}
