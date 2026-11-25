import { Show, type JSX } from "solid-js";

export default function FieldError(props: { id?: string; role?: "alert"; children: JSX.Element }) {
  return (
    <Show when={props.children}>
      {(message) => (
        <p class="field-error" id={props.id} role={props.role}>
          <span aria-hidden="true">✕ </span>
          {message()}
        </p>
      )}
    </Show>
  );
}
