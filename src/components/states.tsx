import { For, Show, type JSX } from "solid-js";

/**
 * Shown while data loads. `variant="skeleton"` draws grey placeholder lines (they hold still when
 * the person asked for reduced motion); the label stays in the DOM for screen readers either way.
 */
export function Loading(props: { label?: string; variant?: "text" | "skeleton"; lines?: number }) {
  const label = () => props.label ?? "Carregando…";
  return (
    <Show
      when={props.variant === "skeleton"}
      fallback={
        <p class="state muted" role="status">
          {label()}
        </p>
      }
    >
      <div class="state skeleton" role="status" aria-busy="true">
        <span class="sr-only">{label()}</span>
        <For each={Array.from({ length: props.lines ?? 3 })}>
          {(_, index) => (
            <span class="skeleton-line" aria-hidden="true" data-wide={index() % 2 === 0} />
          )}
        </For>
      </div>
    </Show>
  );
}

/** Shown when there is nothing to list. `action` is the one thing worth doing next. */
export function EmptyState(props: { title: string; children?: JSX.Element; action?: JSX.Element }) {
  return (
    <div class="state empty-state">
      <p class="empty-title">{props.title}</p>
      {props.children}
      <Show when={props.action}>
        <div class="state-action">{props.action}</div>
      </Show>
    </div>
  );
}

/** pt-BR text for anything thrown: an `ApiError` carries the backend's message. */
export function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Ocorreu um erro inesperado. Tente novamente.";
}

/** Shown when loading failed. With `onRetry`, offers a "Tentar novamente" button. */
export function ErrorState(props: { error: unknown; onRetry?: () => void }) {
  return (
    <div class="state error-state">
      <p class="error" role="alert">
        {errorMessage(props.error)}
      </p>
      <Show when={props.onRetry}>
        {(retry) => (
          <button type="button" class="secondary" onClick={() => retry()()}>
            Tentar novamente
          </button>
        )}
      </Show>
    </div>
  );
}
