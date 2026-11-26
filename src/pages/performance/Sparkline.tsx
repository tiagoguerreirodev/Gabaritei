import { Show } from "solid-js";
import type { WeekPoint } from "../../bindings/WeekPoint";
import { formatPercent } from "../../format";

const WIDTH = 72;
const HEIGHT = 20;
const PAD = 3;

/** A topic's recent weekly accuracy as a tiny line, with its first and last values in words. */
export default function Sparkline(props: { weeks: WeekPoint[] }) {
  const weeks = () => props.weeks.filter((week) => week.accuracy !== null);
  const points = () => {
    const shown = weeks();
    const step = shown.length > 1 ? (WIDTH - 2 * PAD) / (shown.length - 1) : 0;
    return shown.map((week, index) => ({
      x: shown.length > 1 ? PAD + index * step : WIDTH / 2,
      y: PAD + (1 - (week.accuracy ?? 0)) * (HEIGHT - 2 * PAD),
    }));
  };
  const description = () => {
    const shown = weeks();
    const first = shown[0];
    const last = shown[shown.length - 1];
    if (!first || !last) return "";
    return shown.length === 1
      ? `Acerto de ${formatPercent(last.accuracy)} na única semana com respostas`
      : `Acerto de ${formatPercent(first.accuracy)} a ${formatPercent(last.accuracy)} nas últimas ${shown.length} semanas com respostas`;
  };

  return (
    <Show when={weeks().length > 0} fallback={<span class="muted">—</span>}>
      <svg
        class="sparkline"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        width={WIDTH}
        height={HEIGHT}
        role="img"
        aria-label={description()}
      >
        <title>{description()}</title>
        <polyline
          class="line"
          fill="none"
          points={points()
            .map((p) => `${p.x},${p.y}`)
            .join(" ")}
        />
        <circle class="dot" cx={points().at(-1)?.x} cy={points().at(-1)?.y} r="2" />
      </svg>
    </Show>
  );
}
