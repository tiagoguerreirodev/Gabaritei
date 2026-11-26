/// <reference types="node" />
import { describe, expect, it } from "vitest";
import tokens from "../../docs/design-system/tokens.json";

type Themed = { light: string; dark: string } | string;
const tokenValues = new Map<string, Themed>(
  tokens.color.tokens.map((token) => [token.name, token.value]),
);

function theme(mode: "light" | "dark"): Record<string, string> {
  return Object.fromEntries(
    [...tokenValues].map(([name, value]) => [
      name,
      typeof value === "string" ? value : value[mode],
    ]),
  );
}

const light = theme("light");
const darkAll = theme("dark");

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

/** Resolves a `{name}` alias, as `focus` uses. */
function color(theme: Record<string, string>, name: string): string {
  const raw = theme[name] ?? "";
  const alias = /^\{([\w-]+)\}$/.exec(raw)?.[1];
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
  ["on-accent", "success", 4.5], // letter on a filled correct bubble
  ["on-accent", "danger", 4.5], // letter on a filled wrong bubble
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
  ["text", "success-soft", 4.5],
  ["muted", "success-soft", 4.5],
  ["success", "success-soft", 4.5],
  ["text", "danger-soft", 4.5],
  ["muted", "danger-soft", 4.5],
  ["danger", "danger-soft", 4.5],
  ["text", "explain-bg", 4.5],
  ["muted", "explain-bg", 4.5],
  ["accent", "explain-bg", 4.5],
  ["success", "explain-bg", 4.5],
  ["text", "accent-soft", 4.5],
  ["muted", "accent-soft", 4.5],
  ["flag", "success-soft", 3],
  ["flag", "danger-soft", 3],
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
