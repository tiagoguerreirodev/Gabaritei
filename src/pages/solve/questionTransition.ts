const SHIFT_REM = 1.5;
const DURATION_MS = 180;

const prefersReducedMotion = () =>
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Slides the new question in from the side it comes from (`direction` is the sign of the
 * move). Skipped when the user asked for less motion or the browser can't animate. */
export function playQuestionTransition(element: HTMLElement, direction: number) {
  if (prefersReducedMotion() || typeof element.animate !== "function") return;
  const offset = `${Math.sign(direction) * SHIFT_REM}rem`;
  element.animate(
    [
      { opacity: 0, transform: `translateX(${offset})` },
      { opacity: 1, transform: "translateX(0)" },
    ],
    { duration: DURATION_MS, easing: "cubic-bezier(0.2, 0, 0, 1)" },
  );
}
