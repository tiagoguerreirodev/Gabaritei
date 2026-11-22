/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Vitest does not hand out CSS as text through Vite, so read it from disk (tests run from the repo root).
const css = readFileSync("src/styles/shell.css", "utf8");

/** Every top-level rule as [selector list, body]. */
function rules(): [string, string][] {
  return [...css.matchAll(/^([^\s@/][^{}]*)\{([^{}]*)\}/gm)].map(([, selector, body]) => [
    (selector ?? "").trim(),
    body ?? "",
  ]);
}

describe("shell.css", () => {
  it("styles the active menu link on its own, without a column layout", () => {
    const rule = rules().find(([selector]) =>
      selector.split(",").some((s) => s.trim() === ".menu-link.active"),
    );
    expect(rule).toBeDefined();
    expect(rule?.[1]).toContain("--accent-soft");
    expect(rule?.[1]).not.toContain("flex-direction");
    expect(rule?.[0]).not.toContain(".sidebar-footer");
  });

  it("keeps the sidebar footer as a separate rule", () => {
    const footer = rules().find(([selector]) => selector === ".sidebar-footer");
    expect(footer).toBeDefined();
    expect(footer?.[1]).toContain("flex-direction: column");
  });
});
