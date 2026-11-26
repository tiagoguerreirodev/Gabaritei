import { For, Match, Show, Switch, createMemo, type Accessor, type JSX } from "solid-js";

/**
 * Shown while data loads: grey placeholder lines (they hold still when the person asked for
 * reduced motion). The label stays in the DOM for screen readers.
 */
export function Loading(props: { label?: string; lines?: number }) {
  return (
    <div class="state skeleton" role="status">
      <span class="sr-only">{props.label ?? "Carregando…"}</span>
      <For each={Array.from({ length: props.lines ?? 3 })}>
        {(_, index) => (
          <span class="skeleton-line" aria-hidden="true" data-wide={index() % 2 === 0} />
        )}
      </For>
    </div>
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

export function InlineError(props: { children: JSX.Element }) {
  return (
    <p class="error" role="alert">
      {props.children}
    </p>
  );
}

/** Moves focus to the first heading of `root`, so focus does not fall to the body when a retry button disappears. */
function focusHeading(start: Element | null) {
  const selector = "[role='dialog'], section, main";
  let heading: HTMLElement | null = null;
  for (let root = start; root && !heading; root = root.parentElement?.closest(selector) ?? null) {
    heading = root.querySelector<HTMLElement>("h1, h2, h3");
  }
  if (!heading) return;
  heading.tabIndex = -1;
  heading.focus();
}

/**
 * Shown when loading failed. With `onRetry`, offers a "Tentar novamente" button. `busy` (wire it to
 * the resource's `loading`) disables the button while the new request runs: Solid keeps `error`
 * set until then. The alert is mounted again for every new error, so a repeated identical failure
 * is announced again. When a retry succeeds, this component goes away; focus then moves to the
 * heading of the surrounding page, section or dialog. `describedBy` names an element that tells
 * the button apart from other retry buttons on the page.
 */
export function ErrorState(props: {
  error: unknown;
  onRetry?: () => unknown;
  busy?: boolean;
  describedBy?: string;
}) {
  let root: HTMLDivElement | undefined;
  // A new box per error value: keying on it mounts the alert node again.
  const box = createMemo(() => ({ error: props.error }));
  const retry = async (run: () => unknown) => {
    const container = root?.closest("[role='dialog'], section, main") ?? null;
    await run();
    if (root && !root.isConnected) focusHeading(container);
  };
  return (
    <div class="state error-state" ref={root}>
      <Show when={box()} keyed>
        {(current) => <InlineError>{errorMessage(current.error)}</InlineError>}
      </Show>
      <Show when={props.onRetry}>
        {(run) => (
          <button
            type="button"
            class="btn"
            disabled={props.busy}
            aria-describedby={props.describedBy}
            onClick={() => void retry(run())}
          >
            {props.busy ? "Tentando…" : "Tentar novamente"}
          </button>
        )}
      </Show>
    </div>
  );
}

/** Renders errors, the initial load, and the empty or loaded view in that order. */
export function ResourceView<T>(props: {
  value: T | null | undefined;
  error?: Accessor<unknown>;
  pending: boolean;
  retrying?: boolean;
  onRetry?: () => unknown;
  describedBy?: string;
  placeholder?: JSX.Element;
  empty?: JSX.Element;
  children: (value: Accessor<NonNullable<T>>) => JSX.Element;
}) {
  const error = () => props.error?.();
  const available = () => {
    const value = props.value;
    return value == null ? undefined : { value };
  };
  return (
    <Switch fallback={props.empty}>
      <Match when={error() !== undefined}>
        <ErrorState
          error={error()}
          onRetry={props.onRetry}
          busy={props.retrying}
          describedBy={props.describedBy}
        />
      </Match>
      <Match when={props.pending}>{props.placeholder ?? <Loading />}</Match>
      <Match when={available()}>{(loaded) => props.children(() => loaded().value)}</Match>
    </Switch>
  );
}
