import type { AttemptView } from "../../bindings/AttemptView";

/** Apply the returned explanation and references, preserving answers and scoring edits. */
export function mergeExplanation(
  current: AttemptView | undefined,
  explained: AttemptView,
  position: number,
): AttemptView | undefined {
  if (!current || current.id !== explained.id) return current;
  const reveal = explained.questions.find((item) => item.position === position)?.reveal;
  if (!reveal) return current;

  return {
    ...current,
    questions: current.questions.map((item) =>
      item.position === position && item.reveal
        ? {
            ...item,
            reveal: { ...item.reveal, explanation: reveal.explanation, refs: reveal.refs },
          }
        : item,
    ),
  };
}
