/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import tokens from "../../docs/design-system/tokens.json";
import { renderTokensCss, type DesignTokens } from "../../scripts/tokens.ts";

const rendered = renderTokensCss(tokens);

/** The declarations of a rule, by its selector line. */
function declarations(css: string, selector: string): string[] {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`Bloco não encontrado: ${selector}`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  return body
    .split(";")
    .map((line) => line.trim())
    .filter(Boolean);
}

const themedNames = tokens.color.tokens
  .concat(tokens.shadow.tokens as never)
  .filter((token) => typeof token.value !== "string")
  .map((token) => token.name);

describe("tokens.css", () => {
  it("matches tokens.json (run npm run tokens)", () => {
    const committed = readFileSync("src/styles/tokens.css", "utf8");
    // Prettier reflows the generated file, so compare without whitespace.
    expect(committed.replace(/\s+/g, "")).toBe(rendered.replace(/\s+/g, ""));
  });

  it("renders an alias as a var() reference", () => {
    expect(rendered).toContain("--focus: var(--accent);");
  });

  it("writes the same dark values for the system preference and data-theme=dark", () => {
    const fromSystem = declarations(rendered, ':root:not([data-theme="light"])');
    const explicit = declarations(rendered, ':root[data-theme="dark"]');
    expect(fromSystem).toEqual(explicit);
  });

  it("gives every themed token a dark value", () => {
    const darkNames = declarations(rendered, ':root[data-theme="dark"]').map((line) =>
      line.slice(2, line.indexOf(":")),
    );
    expect(darkNames).toEqual(themedNames);
  });
});
