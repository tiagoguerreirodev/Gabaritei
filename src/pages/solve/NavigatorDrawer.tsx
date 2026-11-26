import { Show, createEffect, createSignal, onCleanup } from "solid-js";
import type { ResponseView } from "../../bindings/ResponseView";
import Navigator from "./Navigator";

const PANEL_ID = "navigator-drawer";

/** The question map as a drawer: a toggle that shows the count of answers, and a panel that
 * closes on Escape, on a click outside, or once a question is picked. */
export default function NavigatorDrawer(props: {
  questions: ResponseView[];
  current: number;
  answered: number;
  onSelect: (index: number) => void;
}) {
  const [open, setOpen] = createSignal(false);
  let toggle: HTMLButtonElement | undefined;
  let panel: HTMLDivElement | undefined;

  const close = (returnFocus: boolean) => {
    setOpen(false);
    if (returnFocus) toggle?.focus();
  };

  createEffect(() => {
    if (!open()) return;
    queueMicrotask(() => panel?.querySelector<HTMLElement>("[aria-current='step']")?.focus());
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      close(panel?.contains(document.activeElement) ?? false);
    };
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && (panel?.contains(target) || toggle?.contains(target))) return;
      close(false);
    };
    window.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    onCleanup(() => {
      window.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    });
  });

  return (
    <div class="navigator-drawer-root">
      <button
        ref={toggle}
        type="button"
        class="btn navigator-toggle"
        aria-expanded={open()}
        aria-controls={PANEL_ID}
        onClick={() => setOpen(!open())}
      >
        Mapa de questões{" "}
        <span class="muted">
          ({props.answered}/{props.questions.length})
        </span>
      </button>
      <Show when={open()}>
        <div ref={panel} id={PANEL_ID} class="navigator-drawer">
          <Navigator
            questions={props.questions}
            current={props.current}
            onSelect={(index) => {
              props.onSelect(index);
              close(true);
            }}
          />
        </div>
      </Show>
    </div>
  );
}
