import { formatPercent } from "../format";

type MasteryProps = { name: string; achievement: number | null };

/**
 * Achievement of one group as a horizontal bar plus its number. The value is the backend's
 * `achievement` (points ÷ evaluated questions, 0 to 1); nothing is recomputed here. The bar's
 * length and the printed percentage carry the meaning, never color alone. `null` (nothing
 * evaluated yet) shows an empty track and "—".
 */
export default function MasteryBar(props: MasteryProps) {
  return (
    <div class="mastery">
      <span class="mastery-name" aria-hidden="true">
        {props.name}
      </span>
      <MasteryMeter name={props.name} achievement={props.achievement} />
    </div>
  );
}

/** The bar and number alone, for rows that already show the group's name. */
export function CompactMasteryBar(props: MasteryProps) {
  return (
    <span class="mastery compact">
      <MasteryMeter name={props.name} achievement={props.achievement} />
    </span>
  );
}

function MasteryMeter(props: MasteryProps) {
  const percent = () =>
    props.achievement === null ? 0 : Math.round(Math.min(1, Math.max(0, props.achievement)) * 100);
  return (
    <>
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
        <span class="mastery-fill" style={{ width: `${percent()}%` }} />
      </div>
      <strong class="mastery-value" aria-hidden="true">
        {formatPercent(props.achievement)}
      </strong>
    </>
  );
}
