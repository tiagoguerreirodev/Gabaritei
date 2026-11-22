/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Vitest does not hand out CSS as text through Vite, so read it from disk (tests run from the repo root).
const css = readFileSync("src/styles/tokens.css", "utf8");

/** The declarations of the first rule whose selector line is exactly `selector`. */
function block(selector: string, from = 0): Record<string, string> {
  const start = css.indexOf(`${selector} {`, from);
  if (start < 0) throw new Error(`Bloco não encontrado: ${selector}`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const tokens: Record<string, string> = {};
  for (const [, name, value] of body.matchAll(/--([\w-]+):\s*([^;]+);/g)) {
    if (name && value) tokens[name] = value.trim();
  }
  return tokens;
}

const light = block(":root");
// The dark block only overrides what differs; the rest (like --focus) inherits from :root.
const dark = block(':root[data-theme="dark"]');
const darkAll = { ...light, ...dark };
const darkFromSystem = block(':root:not([data-theme="light"])', css.indexOf("@media"));

function luminance(hex: string): number {
  const value = /^#([0-9a-f]{6})$/i.exec(hex)?.[1];
  if (!value) throw new Error(`Cor inválida: ${hex}`);
  const [r = 0, g = 0, b = 0] = [0, 2, 4].map((i) => {
    const channel = parseInt(value.slice(i, i + 2), 16) / 255;
    return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/** Resolves `var(--x)` one level, as `--focus` uses. */
function color(theme: Record<string, string>, name: string): string {
  const raw = theme[name] ?? "";
  const alias = /^var\(--([\w-]+)\)$/.exec(raw)?.[1];
  return alias ? color(theme, alias) : raw;
}

// [foreground, background, minimum ratio]: 4.5 for text, 3 for non-text parts of controls.
const pairs: [string, string, number][] = [
  ["text", "bg", 4.5],
  ["text", "surface", 4.5],
  ["muted", "bg", 4.5],
  ["muted", "surface", 4.5],
  ["accent", "bg", 4.5],
  ["accent", "surface", 4.5],
  ["accent", "accent-soft", 4.5],
  ["on-accent", "accent", 4.5],
  ["danger", "bg", 4.5],
  ["danger", "surface", 4.5],
  ["success", "bg", 4.5],
  ["success", "surface", 4.5],
  ["flag", "bg", 4.5],
  ["flag", "surface", 4.5],
  ["border-strong", "bg", 3],
  ["border-strong", "surface", 3],
  ["focus", "bg", 3],
  ["focus", "surface", 3],
  ["flag-bar", "surface", 3],
  ["flag-bar", "bg", 3],
];

describe.each([
  ["claro", light],
  ["escuro", darkAll],
] as const)("tema %s", (_name, theme) => {
  it.each(pairs)("%s sobre %s tem contraste de pelo menos %s:1", (fg, bg, minimum) => {
    const ratio = contrast(color(theme, fg), color(theme, bg));
    expect(ratio, `${fg} on ${bg}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(minimum);
  });
});

describe("dark theme tokens", () => {
  it("are identical for the system preference and for an explicit choice", () => {
    for (const [name, value] of Object.entries(darkFromSystem)) {
      if (name in dark) expect(value, name).toBe(dark[name]);
    }
    for (const name of Object.keys(dark)) {
      expect(darkFromSystem[name], name).toBe(dark[name]);
    }
  });
});
