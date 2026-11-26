/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Vitest does not hand out CSS as text through Vite, so read it from disk (tests run from the repo root).
const css = readFileSync("src/styles/ds/nav.css", "utf8");

/** Every top-level rule as [selector list, body]. */
function rules(): [string, string][] {
  return [...css.matchAll(/^([^\s@/][^{}]*)\{([^{}]*)\}/gm)].map(([, selector, body]) => [
    (selector ?? "").trim(),
    body ?? "",
  ]);
}

describe("nav.css", () => {
  it("styles the current page link with the accent tint, without a column layout", () => {
    const rule = rules().find(([selector]) => selector === '.nav-item[aria-current="page"]');
    expect(rule?.[1]).toContain("--accent-soft");
    expect(rule?.[1]).not.toContain("flex-direction");
  });

  it("keeps the nav footer as a separate rule", () => {
    const footer = rules().find(([selector]) => selector === ".nav-footer");
    expect(footer?.[1]).toContain("flex-direction: column");
  });

  it("writes the icon-only tooltip once, under the collapsed shell", () => {
    const tooltips = rules().filter(([, body]) => body.includes("transform: translateY(-50%)"));
    expect(tooltips.map(([selector]) => selector)).toEqual([
      '.shell[data-nav="collapsed"] .nav-item:hover .nav-label,\n.shell[data-nav="collapsed"] .nav-item:focus-visible .nav-label',
    ]);
  });
});
