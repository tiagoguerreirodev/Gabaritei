import { Show } from "solid-js";
import type { CountLimits } from "../../bindings/CountLimits";

export default function CountField(props: {
  value: number;
  counts: CountLimits;
  error: string | null;
  onInput: (value: number) => void;
  onBlur: () => void;
}) {
  return (
    <>
      <label for="gen-count">Quantidade</label>
      <input
        id="gen-count"
        type="number"
        min={props.counts.min}
        max={props.counts.max}
        required
        aria-required="true"
        aria-invalid={props.error ? "true" : undefined}
        aria-describedby={props.error ? "gen-count-hint gen-count-error" : "gen-count-hint"}
        value={props.value}
        onInput={(event) => props.onInput(Number(event.currentTarget.value))}
        onBlur={props.onBlur}
      />
      <p class="hint muted" id="gen-count-hint">
        Obrigatório. De {props.counts.min} a {props.counts.max}. Cada questão passa pela conferência
        às cegas.
      </p>
      <Show when={props.error}>
        {(message) => (
          <p class="field-error" id="gen-count-error">
            <span aria-hidden="true">✕ </span>
            {message()}
          </p>
        )}
      </Show>
    </>
  );
}
