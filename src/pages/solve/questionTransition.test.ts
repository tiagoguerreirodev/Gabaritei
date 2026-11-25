import { afterEach, describe, expect, it, vi } from "vitest";
import { playQuestionTransition } from "./questionTransition";

function stage() {
  const element = document.createElement("div");
  const animate = vi.fn();
  element.animate = animate;
  return { element, animate };
}

function prefersReducedMotion(matches: boolean) {
  window.matchMedia = vi.fn().mockReturnValue({ matches }) as typeof window.matchMedia;
}

describe("playQuestionTransition", () => {
  afterEach(() => {
    // jsdom has no matchMedia of its own.
    Reflect.deleteProperty(window, "matchMedia");
  });

  it("slides the next question in from the right and the previous one from the left", () => {
    prefersReducedMotion(false);
    const { element, animate } = stage();
    playQuestionTransition(element, 3);
    playQuestionTransition(element, -1);
    const starts = animate.mock.calls.map(([frames]) => (frames as Keyframe[])[0]?.transform);
    expect(starts).toEqual(["translateX(1.5rem)", "translateX(-1.5rem)"]);
  });

  it("does not move when the user asked for reduced motion", () => {
    prefersReducedMotion(true);
    const { element, animate } = stage();
    playQuestionTransition(element, 1);
    expect(animate).not.toHaveBeenCalled();
  });
});
