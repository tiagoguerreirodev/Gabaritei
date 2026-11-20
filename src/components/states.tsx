import type { JSX } from "solid-js";

/** Shown while data loads. */
export function Loading(props: { label?: string }) {
  return (
    <p class="state muted" role="status">
      {props.label ?? "Carregando…"}
    </p>
  );
}

/** Shown when there is nothing to list. */
export function EmptyState(props: { title: string; children?: JSX.Element }) {
  return (
    <div class="state empty-state">
      <p class="empty-title">{props.title}</p>
      {props.children}
    </div>
  );
}

/** pt-BR text for anything thrown: an `ApiError` carries the backend's message. */
export function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Ocorreu um erro inesperado. Tente novamente.";
}

/** Shown when loading failed. */
export function ErrorState(props: { error: unknown }) {
  return (
    <p class="state error" role="alert">
      {errorMessage(props.error)}
    </p>
  );
}
