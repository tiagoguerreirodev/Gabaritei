import { A } from "@solidjs/router";
import { Match, Show, Switch, createSignal } from "solid-js";
import type { WeakTopic } from "../../bindings/WeakTopic";
import { formatDate, formatNumber, formatPercent } from "../../format";
import { modeLabel } from "../../labels";
import { openAttempt } from "../../navigation";
import StartSessionModal from "../solve/StartSessionModal";
import type { TodayAction } from "./todayAction";

/** The highlighted card at the top of Início: one main action, chosen by urgency. */
export default function TodayCard(props: { action: TodayAction }) {
  const [starting, setStarting] = createSignal<"reviews" | WeakTopic>();
  const topicPath = (topic: WeakTopic) => `${topic.area} › ${topic.matter} › ${topic.topic}`;

  return (
    <section class="card card-wide today-card" aria-labelledby="today-title">
      <Switch>
        <Match when={props.action.kind === "resume" && props.action.attempt} keyed>
          {(attempt) => (
            <>
              <p class="today-kicker">Continue de onde parou</p>
              <h2 id="today-title">{attempt.title}</h2>
              <p class="muted">
                {modeLabel[attempt.feedbackMode]} · {formatNumber(attempt.answered)} de{" "}
                {formatNumber(attempt.total)} respondidas · iniciada em{" "}
                {formatDate(attempt.startedAt)}
              </p>
              <button
                type="button"
                class="btn btn-primary"
                aria-label={`Retomar ${attempt.title}`}
                onClick={() => openAttempt(attempt.id)}
              >
                Retomar
              </button>
            </>
          )}
        </Match>
        <Match when={props.action.kind === "reviews" && props.action.due} keyed>
          {(due) => (
            <>
              <p class="today-kicker">Revisões de hoje</p>
              <h2 id="today-title">
                {due === 1 ? "1 revisão pendente" : `${formatNumber(due)} revisões pendentes`}
              </h2>
              <p class="muted">Revise agora para fixar o que você já estudou.</p>
              <button type="button" class="btn btn-primary" onClick={() => setStarting("reviews")}>
                Fazer revisões
              </button>
            </>
          )}
        </Match>
        <Match when={props.action.kind === "weakest" && props.action.topic} keyed>
          {(topic) => (
            <>
              <p class="today-kicker">Treine seu assunto mais fraco</p>
              <h2 id="today-title">{topic.topic}</h2>
              <p class="muted">
                {topic.area} › {topic.matter} · aproveitamento de{" "}
                {formatPercent(topic.stats.score.achievement)}
              </p>
              <button type="button" class="btn btn-primary" onClick={() => setStarting(topic)}>
                Treinar assunto
              </button>
            </>
          )}
        </Match>
        <Match when={props.action.kind === "start"}>
          <p class="today-kicker">Comece por aqui</p>
          <h2 id="today-title">Você ainda não finalizou nenhuma tentativa.</h2>
          <p class="muted">Comece um caderno ou um assunto no Banco.</p>
          <A class="btn btn-primary" href="/banco">
            Ir para o Banco
          </A>
        </Match>
      </Switch>

      <Show when={starting()} keyed>
        {(target) =>
          target === "reviews" ? (
            <StartSessionModal
              title="Praticar revisões"
              source={{ kind: "reviews" }}
              onClose={() => setStarting(undefined)}
            />
          ) : (
            <StartSessionModal
              title={topicPath(target)}
              source={{ kind: "topic", topicId: target.topicId }}
              available={target.stats.available}
              onClose={() => setStarting(undefined)}
            />
          )
        }
      </Show>
    </section>
  );
}
