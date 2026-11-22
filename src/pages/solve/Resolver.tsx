import { Match, Show, Switch, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { api } from "../../api";
import type { AttemptView } from "../../bindings/AttemptView";
import type { Score } from "../../bindings/Score";
import Modal from "../../components/Modal";
import { ErrorState, Loading } from "../../components/states";
import { showError, showToast } from "../../components/toasts";
import Navigator from "./Navigator";
import QuestionCard from "./QuestionCard";
import { loadShortcuts, shortcutsEnabled } from "../../shortcuts";
import { formatPercent, optionLetters } from "./format";

const modeLabel = { study: "Estudo", exam: "Simulado" } as const;

/** End-of-attempt recap: points over evaluated questions, achievement and the four counts. The
 * counts carry a shape as well as a colour. Comparing with the previous attempt is left out:
 * no existing command returns the scores of earlier attempts. */
function ScoreSummary(props: { score: Score }) {
  let section: HTMLElement | undefined;
  onMount(() => section?.focus());
  return (
    <section class="card score recap" aria-label="Resultado" tabindex="-1" ref={section}>
      <h2>Resultado</h2>
      <p class="recap-headline" data-testid="recap-points">
        <strong>
          {props.score.correct} de {props.score.evaluated}
        </strong>{" "}
        pontos · {formatPercent(props.score.achievement)} de aproveitamento
      </p>
      <ul class="recap-counts" aria-label="Contagem por resultado">
        <li class="recap-correct">
          <span class="recap-glyph" aria-hidden="true">
            ✓
          </span>
          {props.score.correct} {props.score.correct === 1 ? "certa" : "certas"}
        </li>
        <li class="recap-wrong">
          <span class="recap-glyph" aria-hidden="true">
            ✕
          </span>
          {props.score.wrong} {props.score.wrong === 1 ? "errada" : "erradas"}
        </li>
        <li class="recap-blank">
          <span class="recap-glyph" aria-hidden="true">
            ○
          </span>
          {props.score.blank} em branco
        </li>
        <li class="recap-annulled">
          <span class="recap-glyph recap-struck" aria-hidden="true">
            –
          </span>
          {props.score.annulled} {props.score.annulled === 1 ? "anulada" : "anuladas"}
        </li>
      </ul>
      <dl class="facts">
        <dt>Acerto entre respondidas</dt>
        <dd>{formatPercent(props.score.accuracy)}</dd>
      </dl>
    </section>
  );
}

/** Whether a key press belongs to a text field, where shortcuts must not fire. */
const typing = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));

/** The solve screen for one attempt (spec §3.3). */
export default function Resolver(props: { attemptId: string; onLeave: () => void }) {
  const [view, setView] = createSignal<AttemptView>();
  const [loadError, setLoadError] = createSignal<unknown>();
  const [index, setIndex] = createSignal(0);
  const [dialog, setDialog] = createSignal<"finish" | "abandon" | null>(null);
  const [eliminating, setEliminating] = createSignal(false);
  // Estudo: the option marked on this question but not yet confirmed with Enter.
  const [pending, setPending] = createSignal<string | null>(null);
  // The confirmed answer is on its way to the backend: no new mark or confirmation until it lands.
  const [sending, setSending] = createSignal(false);
  // Text for the polite live region: the question number on navigation, the outcome on answers.
  const [announcement, setAnnouncement] = createSignal("");
  const [helpOpen, setHelpOpen] = createSignal(false);
  let shortcutsBox: HTMLDivElement | undefined;
  let shortcutsToggle: HTMLButtonElement | undefined;

  createEffect(
    on(
      () => props.attemptId,
      (id) => {
        setView(undefined);
        setLoadError(undefined);
        setIndex(0);
        setPending(null);
        api.attempt(id).then((loaded) => {
          setView(loaded);
          // Resume at the first unanswered question.
          const next = loaded.questions.findIndex((q) => q.choice === null);
          setIndex(loaded.finishedAt === null && next >= 0 ? next : 0);
        }, setLoadError);
      },
    ),
  );

  // Changes run one after another, so `finish` always quotes the latest revision.
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = (change: (id: string) => Promise<AttemptView>) => {
    const id = view()?.id;
    if (!id) return;
    queue = queue.then(() => change(id).then(setView, showError));
  };

  const finished = () => view()?.finishedAt != null;
  const current = () => view()?.questions[index()];
  const locked = () => finished() || current()?.reveal != null;
  const total = () => view()?.questions.length ?? 0;
  const answered = () => view()?.questions.filter((q) => q.choice !== null).length ?? 0;

  // Only the question number is announced here. A confirmed answer hands the focus to the reveal
  // section, which reads the outcome; annulling and key changes have their own toast. Finishing
  // is announced by `finish()`.
  createEffect(
    on(
      () => current()?.position,
      (position, previous) => {
        if (position === undefined || previous === undefined || finished()) return;
        setAnnouncement(`Questão ${position + 1} de ${total()}.`);
      },
    ),
  );

  // Any action other than the letter after `X` cancels the pending elimination.
  const select = (i: number) => {
    setEliminating(false);
    setPending(null);
    setIndex(i);
  };
  const move = (delta: number) =>
    select(Math.min(Math.max(index() + delta, 0), Math.max(total() - 1, 0)));

  const answer = (optionId: string) => {
    const q = current();
    setEliminating(false);
    if (!q || locked()) return;
    // In Simulado, choosing the selected option again clears it.
    const choice = q.choice === optionId && view()?.feedbackMode === "exam" ? null : optionId;
    enqueue((id) => api.answer(id, q.position, choice));
  };
  /** Estudo locks and reveals on the first answer, so a choice is only marked until the user
   * confirms it. Simulado saves it straight away: it can still be changed or cleared. */
  const choose = (optionId: string) => {
    const q = current();
    setEliminating(false);
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
    setEliminating(false);
    if (!q || locked()) return;
    if (pending() === optionId) setPending(null);
    enqueue((id) => api.eliminate(id, q.position, optionId));
  };
  const flag = () => {
    const q = current();
    setEliminating(false);
    if (!q || finished()) return;
    enqueue((id) => api.flag(id, q.position, !q.flagged));
  };

  const annul = (annulled: boolean) => {
    const q = current();
    if (!q?.reveal) return;
    enqueue(async (id) => {
      const next = await (annulled ? api.annul(id, q.position) : api.unannul(id, q.position));
      showToast(annulled ? "Questão anulada." : "Anulação desfeita.");
      return next;
    });
  };

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
      showToast("Gabarito atualizado. As pontuações foram recalculadas.");
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
    const reveal = next.questions.find((item) => item.position === q.position)?.reveal;
    if (!reveal) return;
    setView((now) =>
      now
        ? {
            ...now,
            questions: now.questions.map((item) =>
              item.position === q.position && item.reveal
                ? {
                    ...item,
                    reveal: { ...item.reveal, explanation: reveal.explanation, refs: reveal.refs },
                  }
                : item,
            ),
          }
        : now,
    );
    showToast("Explicação adicionada à questão.");
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
      setView(await api.finish(latest.id, latest.revision));
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

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape" && helpOpen() && !dialog()) {
      const inside = shortcutsBox?.contains(document.activeElement);
      setHelpOpen(false);
      if (inside) shortcutsToggle?.focus();
      return;
    }
    if (dialog() || event.ctrlKey || event.metaKey || event.altKey || typing(event.target)) return;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      move(event.key === "ArrowLeft" ? -1 : 1);
      return;
    }
    // A held key would answer, toggle or flag over and over.
    if (event.repeat) return;
    const q = current();
    if (!q || finished()) return;
    if (event.key === "Enter") {
      // Enter keeps working on a focused button, except on the marked option itself, where it
      // confirms (the click that marked it left the focus there).
      const target = event.target;
      const onOtherButton =
        target instanceof HTMLButtonElement && !target.matches(".option-choose[aria-pressed=true]");
      if (pending() === null || onOtherButton) return;
      event.preventDefault();
      confirm();
      return;
    }
    // Single letters are a convenience that can be turned off, and never fire with Shift.
    if (!shortcutsEnabled() || event.shiftKey) return;
    const key = event.key.toUpperCase();
    const optionAt = (letter: string) => q.options[optionLetters(q).indexOf(letter)];
    if (eliminating()) {
      setEliminating(false);
      const option = optionAt(key);
      if (option) {
        event.preventDefault();
        eliminate(option.id);
      }
      return;
    }
    if (key === "X") {
      if (!locked()) setEliminating(true);
    } else if (key === "F") {
      flag();
    } else {
      const option = optionAt(key);
      if (!option) return;
      choose(option.id);
      // Estudo: keep the focus on the marked option, so Enter confirms it.
      if (view()?.feedbackMode === "study") {
        document
          .querySelectorAll<HTMLElement>(".option-choose")
          .forEach((button) => button.dataset.optionId === option.id && button.focus());
      }
    }
    event.preventDefault();
  };
  onMount(() => {
    void loadShortcuts();
    window.addEventListener("keydown", onKeyDown);
  });
  onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  // Closes the shortcut help when the pointer goes down anywhere outside it.
  createEffect(() => {
    if (!helpOpen()) return;
    const close = (event: PointerEvent) => {
      if (!(event.target instanceof Node && shortcutsBox?.contains(event.target))) {
        setHelpOpen(false);
      }
    };
    document.addEventListener("pointerdown", close);
    onCleanup(() => document.removeEventListener("pointerdown", close));
  });

  return (
    <>
      <div class="sr-only" aria-live="polite" aria-atomic="true" data-testid="live-region">
        {announcement()}
      </div>
      <Switch>
        <Match when={loadError()}>
          {(error) => (
            <>
              <h1>Resolver</h1>
              <ErrorState error={error()} />
            </>
          )}
        </Match>
        <Match when={!view()}>
          <h1>Resolver</h1>
          <Loading label="Carregando tentativa…" />
        </Match>
        <Match when={view()}>
          {(attempt) => (
            <div class="resolver">
              <header class="resolver-header">
                <div>
                  <h1>{attempt().title}</h1>
                  <p class="muted">
                    <span class="badge">{modeLabel[attempt().feedbackMode]}</span>{" "}
                    <span data-testid="progress">
                      {answered()} de {total()} respondidas
                    </span>
                    <Show when={finished()}> · finalizada</Show>
                  </p>
                  <div
                    class="progress-bar"
                    role="progressbar"
                    aria-label="Progresso da tentativa"
                    aria-valuemin={0}
                    aria-valuemax={total()}
                    aria-valuenow={answered()}
                    aria-valuetext={`${answered()} de ${total()} respondidas`}
                  >
                    <div
                      class="progress-fill"
                      style={{ width: `${total() === 0 ? 0 : (answered() / total()) * 100}%` }}
                    />
                  </div>
                </div>
                <div class="resolver-actions">
                  <Show
                    when={!finished()}
                    fallback={
                      <button type="button" onClick={() => props.onLeave()}>
                        Voltar ao Banco
                      </button>
                    }
                  >
                    <button type="button" onClick={() => setDialog("abandon")}>
                      Abandonar
                    </button>
                    <button type="button" class="primary" onClick={() => setDialog("finish")}>
                      Finalizar
                    </button>
                  </Show>
                </div>
              </header>
              <Show when={attempt().score}>{(score) => <ScoreSummary score={score()} />}</Show>
              <Navigator questions={attempt().questions} current={index()} onSelect={select} />
              <Show when={current()}>
                {(q) => (
                  <QuestionCard
                    question={q()}
                    number={index() + 1}
                    total={total()}
                    locked={locked()}
                    finished={finished()}
                    eliminating={eliminating()}
                    pending={pending()}
                    confirmRequired={attempt().feedbackMode === "study"}
                    onAnswer={choose}
                    onConfirm={confirm}
                    onEliminate={eliminate}
                    onFlag={flag}
                    onAnnul={annul}
                    onExplain={explain}
                    onRecheck={recheck}
                    onRecheckApply={applyRecheck}
                    onRecheckDismiss={dismissRecheck}
                    onAiCancel={() => void api.aiRequestCancel().catch(showError)}
                  />
                )}
              </Show>
              <footer class="resolver-footer">
                <button type="button" disabled={index() === 0} onClick={() => move(-1)}>
                  ← Anterior
                </button>
                <div class="shortcuts" ref={shortcutsBox}>
                  <button
                    type="button"
                    class="shortcuts-toggle"
                    ref={shortcutsToggle}
                    aria-expanded={helpOpen()}
                    aria-controls="shortcuts-help"
                    onClick={() => setHelpOpen(!helpOpen())}
                  >
                    Atalhos
                  </button>
                  <div
                    id="shortcuts-help"
                    class="shortcuts-panel"
                    role="group"
                    aria-label="Atalhos de teclado"
                    hidden={!helpOpen()}
                  >
                    <Show
                      when={shortcutsEnabled()}
                      fallback={<p>Os atalhos de uma tecla estão desativados em Configurações.</p>}
                    >
                      <ul>
                        <li>
                          <kbd>A</kbd>–<kbd>E</kbd> (ou <kbd>C</kbd>/<kbd>E</kbd>){" "}
                          {attempt().feedbackMode === "study" ? "escolhe" : "responde"}
                        </li>
                        <Show when={attempt().feedbackMode === "study"}>
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
                <button type="button" disabled={index() >= total() - 1} onClick={() => move(1)}>
                  Próxima →
                </button>
              </footer>
              <Show when={dialog() === "finish"}>
                <Modal
                  title="Finalizar tentativa?"
                  onClose={() => setDialog(null)}
                  footer={
                    <>
                      <button type="button" onClick={() => setDialog(null)}>
                        Continuar resolvendo
                      </button>
                      <button type="button" class="primary" onClick={() => void finish()}>
                        Finalizar
                      </button>
                    </>
                  }
                >
                  <p data-testid="finish-summary">
                    {answered()} respondidas e {total() - answered()} em branco.
                  </p>
                  <p class="muted">Depois de finalizar, as respostas não podem mais mudar.</p>
                </Modal>
              </Show>
              <Show when={dialog() === "abandon"}>
                <Modal
                  title="Abandonar tentativa?"
                  onClose={() => setDialog(null)}
                  footer={
                    <>
                      <button type="button" onClick={() => setDialog(null)}>
                        Voltar
                      </button>
                      <button type="button" class="primary danger" onClick={() => void abandon()}>
                        Abandonar
                      </button>
                    </>
                  }
                >
                  <p>A tentativa e suas respostas serão apagadas e não entram no histórico.</p>
                </Modal>
              </Show>
            </div>
          )}
        </Match>
      </Switch>
    </>
  );
}
