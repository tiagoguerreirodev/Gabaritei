import { Show, createUniqueId, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";

/**
 * A modal dialog. Closes with Escape, the close button or a click on the backdrop, and
 * returns focus to whatever had it before opening.
 */
export default function Modal(props: {
  title: string;
  onClose: () => void;
  children: JSX.Element;
  footer?: JSX.Element;
}) {
  const titleId = createUniqueId();
  let dialog: HTMLDivElement | undefined;
  const previouslyFocused = document.activeElement as HTMLElement | null;

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      props.onClose();
    }
  };

  onMount(() => dialog?.focus());
  onCleanup(() => previouslyFocused?.focus());

  return (
    <Portal>
      <div
        class="modal-backdrop"
        onClick={(event) => {
          if (event.target === event.currentTarget) props.onClose();
        }}
      >
        <div
          ref={dialog}
          class="modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabIndex={-1}
          onKeyDown={onKeyDown}
        >
          <header class="modal-header">
            <h2 id={titleId}>{props.title}</h2>
            <button
              type="button"
              class="icon-button"
              aria-label="Fechar"
              onClick={() => props.onClose()}
            >
              ×
            </button>
          </header>
          <div class="modal-body">{props.children}</div>
          <Show when={props.footer}>
            <footer class="modal-footer">{props.footer}</footer>
          </Show>
        </div>
      </div>
    </Portal>
  );
}
