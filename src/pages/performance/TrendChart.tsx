import { A } from "@solidjs/router";
import { For, Show } from "solid-js";
import type { WeekPoint } from "../../bindings/WeekPoint";
import { EmptyState } from "../../components/states";
import { formatPercent } from "../../format";

const WIDTH = 640;
const HEIGHT = 220;
const PAD = { left: 44, right: 16, top: 12, bottom: 30 };

const time = (week: WeekPoint) => Date.parse(`${week.weekStart}T00:00:00Z`);
const WEEK = 7 * 24 * 60 * 60 * 1000;
const label = (week: WeekPoint) => {
  const [, month, day] = week.weekStart.split("-");
  return `${day}/${month}`;
};

/** Weekly accuracy as a lightweight SVG line (no chart library). */
export default function TrendChart(props: { weeks: WeekPoint[] }) {
  const innerWidth = WIDTH - PAD.left - PAD.right;
  const innerHeight = HEIGHT - PAD.top - PAD.bottom;
  const y = (accuracy: number) => PAD.top + (1 - accuracy) * innerHeight;

  /** Points are placed by date, so a skipped week leaves a visible gap. */
  const points = () => {
    const weeks = props.weeks.filter((w) => w.accuracy !== null);
    const first = weeks.length ? time(weeks[0]!) : 0;
    const span = weeks.length ? time(weeks[weeks.length - 1]!) - first : 0;
    return weeks.map((week) => ({
      week,
      x: PAD.left + (span === 0 ? innerWidth / 2 : ((time(week) - first) / span) * innerWidth),
      y: y(week.accuracy ?? 0),
    }));
  };
  /** Runs of consecutive weeks: a skipped week breaks the line instead of being bridged. */
  const segments = () => {
    const runs: ReturnType<typeof points>[] = [];
    for (const point of points()) {
      const run = runs[runs.length - 1];
      const previous = run?.[run.length - 1];
      if (run && previous && time(point.week) - time(previous.week) <= WEEK) run.push(point);
      else runs.push([point]);
    }
    return runs;
  };
  const describe = (week: WeekPoint) =>
    `Semana de ${label(week)}: ${formatPercent(week.accuracy)} (${week.correct} de ${week.answered} respostas)`;

  return (
    <Show
      when={props.weeks.length > 0}
      fallback={
        <EmptyState
          title="Ainda não há respostas para mostrar a evolução."
          action={
            <A class="btn btn-primary" href="/banco">
              Ir para o Banco
            </A>
          }
        >
          <p class="muted">Finalize uma tentativa para ver o acerto semanal.</p>
        </EmptyState>
      }
    >
      <svg
        class="trend-chart"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label="Acerto semanal"
      >
        <For each={[0, 0.5, 1]}>
          {(tick) => (
            <>
              <line class="grid" x1={PAD.left} x2={WIDTH - PAD.right} y1={y(tick)} y2={y(tick)} />
              <text class="axis" x={PAD.left - 6} y={y(tick) + 4} text-anchor="end">
                {Math.round(tick * 100)}%
              </text>
            </>
          )}
        </For>
        <For each={segments()}>
          {(segment) => (
            <polyline
              class="line"
              fill="none"
              points={segment.map((p) => `${p.x},${p.y}`).join(" ")}
            />
          )}
        </For>
        <For each={points()}>
          {(point, index) => (
            <>
              <circle class="dot" cx={point.x} cy={point.y} r="4">
                <title>{describe(point.week)}</title>
              </circle>
              <Show when={index() === 0 || index() === points().length - 1}>
                <text
                  class="axis"
                  x={point.x}
                  y={HEIGHT - 8}
                  text-anchor={points().length === 1 ? "middle" : index() === 0 ? "start" : "end"}
                >
                  {label(point.week)}
                </text>
              </Show>
            </>
          )}
        </For>
      </svg>
      <table class="sr-only">
        <caption>Acerto semanal</caption>
        <thead>
          <tr>
            <th scope="col">Semana de</th>
            <th scope="col">Respostas</th>
            <th scope="col">Acertos</th>
            <th scope="col">Acerto</th>
          </tr>
        </thead>
        <tbody>
          <For each={props.weeks}>
            {(week) => (
              <tr>
                <th scope="row">{label(week)}</th>
                <td>{week.answered}</td>
                <td>{week.correct}</td>
                <td>{formatPercent(week.accuracy)}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
      <p class="chart-note">
        Cada ponto é uma semana (de segunda a domingo); todas as respostas contam, não só a última
        de cada questão.
      </p>
    </Show>
  );
}
