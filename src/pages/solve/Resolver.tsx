import { Match, Show, Switch, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { api } from "../../api";
import type { AttemptView } from "../../bindings/AttemptView";
import type { Score } from "../../bindings/Score";
import Modal from "../../components/Modal";
import { ErrorState, Loading } from "../../components/states";
import { showError } from "../../components/toasts";
import Navigator from "./Navigator";
import QuestionCard from "./QuestionCard";
import { formatPercent, optionLetters } from "./format";

const modeLabel = { study: "Estudo", exam: "Simulado" } as const;

function ScoreSummary(props: { score: Score }) {
  return (
    <section class="card score" aria-label="Resultado">
      <h2>Resultado</h2>
      <dl class="facts">
        <dt>Acertos</dt>
        <dd>{props.score.correct}</dd>
        <dt>Erros</dt>
        <dd>{props.score.wrong}</dd>
        <dt>Em branco</dt>
        <dd>{props.score.blank}</dd>
        <dt>Anuladas</dt>
        <dd>{props.score.annulled}</dd>
        <dt>Aproveitamento</dt>
        <dd>{formatPercent(props.score.achievement)}</dd>
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

  createEffect(
    on(
      () => props.attemptId,
      (id) => {
        setView(undefined);
        setLoadError(undefined);
        setIndex(0);
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

  // Any action other than the letter after `X` cancels the pending elimination.
  const select = (i: number) => {
    setEliminating(false);
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
  const eliminate = (optionId: string) => {
    const q = current();
    setEliminating(false);
    if (!q || locked()) return;
    enqueue((id) => api.eliminate(id, q.position, optionId));
  };
  const flag = () => {
    const q = current();
    setEliminating(false);
    if (!q || finished()) return;
    enqueue((id) => api.flag(id, q.position, !q.flagged));
  };

  const finish = async () => {
    await queue;
    const latest = view();
    if (!latest) return;
    try {
      setView(await api.finish(latest.id, latest.revision));
      setDialog(null);
      setIndex(0);
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
      answer(option.id);
    }
    event.preventDefault();
  };
  onMount(() => window.addEventListener("keydown", onKeyDown));
  onCleanup(() => window.removeEventListener("keydown", onKeyDown));

  return (
    <Switch>
      <Match when={loadError()}>{(error) => <ErrorState error={error()} />}</Match>
      <Match when={!view()}>
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
                  onAnswer={answer}
                  onEliminate={eliminate}
                  onFlag={flag}
                />
              )}
            </Show>
            <footer class="resolver-footer">
              <button type="button" disabled={index() === 0} onClick={() => move(-1)}>
                ← Anterior
              </button>
              <span class="muted shortcuts">
                Atalhos: A–E (ou C/E) responde · X + letra elimina · F marca · ←/→ navega
              </span>
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
  );
}
