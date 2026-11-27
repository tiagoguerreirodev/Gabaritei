import { A } from "@solidjs/router";
import { Match, Switch } from "solid-js";
import { formatDate, formatNumber, formatPercent } from "../../format";
import { modeLabel } from "../../labels";
import { openAttempt } from "../../navigation";
import { bancoPath, emptyFilter } from "../bank/filterUrl";
import { presets } from "../bank/presets";
import type { TodayAction } from "./todayAction";

/** The highlighted card at the top of Início: one main action, chosen by urgency. */
export default function TodayCard(props: { action: TodayAction }) {
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
              <p class="muted">Revise agora para fixar o que já estudou.</p>
              <A class="btn btn-primary" href={bancoPath(presets.reviews)}>
                Fazer revisões
              </A>
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
              <A
                class="btn btn-primary"
                href={bancoPath({ filter: { ...emptyFilter, topicIds: [topic.topicId] } })}
              >
                Treinar assunto
              </A>
            </>
          )}
        </Match>
        <Match when={props.action.kind === "start"}>
          <p class="today-kicker">Comece por aqui</p>
          <h2 id="today-title">Nenhuma tentativa finalizada ainda.</h2>
          <p class="muted">Comece um caderno ou um assunto no Banco.</p>
          <A class="btn btn-primary" href="/banco">
            Ir para o Banco
          </A>
        </Match>
      </Switch>
    </section>
  );
}
