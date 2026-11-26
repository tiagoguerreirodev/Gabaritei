import { createEffect, createSignal, onCleanup, onMount, type Accessor } from "solid-js";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { ResponseView } from "../../bindings/ResponseView";
import { optionLetters } from "../../format";
import { loadShortcuts, shortcutsEnabled } from "../../shortcuts";

export type OptionAction = "answer" | "eliminate";

type ShortcutOptions = {
  dialogOpen: Accessor<boolean>;
  question: Accessor<ResponseView | undefined>;
  finished: Accessor<boolean>;
  locked: Accessor<boolean>;
  feedbackMode: Accessor<FeedbackMode | undefined>;
  pending: Accessor<string | null>;
  move: (delta: number) => void;
  choose: (optionId: string) => void;
  confirm: () => void;
  eliminate: (optionId: string) => void;
  flag: () => void;
};

const typing = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));

export default function useResolverShortcuts(options: ShortcutOptions) {
  const [helpOpen, setHelpOpen] = createSignal(false);
  const [optionAction, setOptionAction] = createSignal<OptionAction>("answer");
  let panel: HTMLDivElement | undefined;
  let toggle: HTMLButtonElement | undefined;

  const closeHelp = () => setHelpOpen(false);
  const resetOptionAction = () => setOptionAction("answer");

  const handleLetter = (key: string, question: ResponseView): boolean => {
    const option = question.options[optionLetters(question).indexOf(key)];
    if (optionAction() === "eliminate") {
      resetOptionAction();
      if (!option) return false;
      options.eliminate(option.id);
      return true;
    }

    switch (key) {
      case "X":
        if (!options.locked()) setOptionAction("eliminate");
        return true;
      case "F":
        options.flag();
        return true;
      default:
        if (!option) return false;
        options.choose(option.id);
        // Estudo: keep the focus on the marked option, so Enter confirms it.
        if (options.feedbackMode() === "study") {
          document
            .querySelectorAll<HTMLElement>(".option")
            .forEach((button) => button.dataset.optionId === option.id && button.focus());
        }
        return true;
    }
  };

  const handleConfirm = (target: EventTarget | null): boolean => {
    // A focused button keeps Enter for its own action, except the option that was just marked.
    const onOtherButton =
      target instanceof HTMLButtonElement && !target.matches(".option[aria-pressed=true]");
    if (options.pending() === null || onOtherButton) return false;
    options.confirm();
    return true;
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (options.dialogOpen()) return;
    if (event.key === "Escape" && helpOpen()) {
      const focusedInside = panel?.contains(document.activeElement);
      closeHelp();
      if (focusedInside) toggle?.focus();
      return;
    }
    if (event.ctrlKey || event.metaKey || event.altKey || typing(event.target)) return;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      options.move(event.key === "ArrowLeft" ? -1 : 1);
      return;
    }
    // A held key would answer, toggle or flag over and over.
    const question = options.question();
    if (!question || options.finished() || event.repeat) return;
    const handled =
      event.key === "Enter"
        ? handleConfirm(event.target)
        : shortcutsEnabled() && !event.shiftKey && handleLetter(event.key.toUpperCase(), question);
    if (handled) event.preventDefault();
  };

  onMount(() => {
    void loadShortcuts();
    window.addEventListener("keydown", onKeyDown);
  });
  onCleanup(() => window.removeEventListener("keydown", onKeyDown));

  createEffect(() => {
    if (!helpOpen()) return;
    const closeOutside = (event: PointerEvent) => {
      if (!(event.target instanceof Node && panel?.contains(event.target))) {
        closeHelp();
      }
    };
    document.addEventListener("pointerdown", closeOutside);
    onCleanup(() => document.removeEventListener("pointerdown", closeOutside));
  });

  return {
    helpOpen,
    optionAction,
    resetOptionAction,
    toggleHelp: () => setHelpOpen(!helpOpen()),
    panelRef: (element: HTMLDivElement) => {
      panel = element;
    },
    toggleRef: (element: HTMLButtonElement) => {
      toggle = element;
    },
  };
}
