import { formatPercent } from "../pages/solve/format";

/**
 * Achievement of one group as a horizontal bar plus its number. The value is the backend's
 * `achievement` (points ÷ evaluated questions, 0 to 1); nothing is recomputed here. The bar's
 * length and the printed percentage carry the meaning, never color alone. `null` (nothing
 * evaluated yet) shows an empty track and "—".
 */
export default function MasteryBar(props: { name: string; achievement: number | null }) {
  const percent = () =>
    props.achievement === null ? 0 : Math.round(Math.min(1, Math.max(0, props.achievement)) * 100);
  return (
    <div class="mastery">
      <span class="mastery-name">{props.name}</span>
      <div
        class="mastery-track"
        role="meter"
        aria-label={`Aproveitamento em ${props.name}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent()}
        aria-valuetext={
          props.achievement === null
            ? "Sem questões avaliadas"
            : `${formatPercent(props.achievement)} de aproveitamento`
        }
      >
        <span
          class="mastery-fill"
          style={{
            width: `${percent()}%`,
            "border-right-width": percent() === 0 ? "0" : undefined,
          }}
        />
      </div>
      <strong class="mastery-value">{formatPercent(props.achievement)}</strong>
    </div>
  );
}
