/// <reference types="node" />
import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { parseRules, type Rules } from "./cssRules";

const BUNDLE = "docs/design-system/components/bundle.css";
const DS_DIR = "src/styles/ds";

/**
 * Rules that may exist in the published bundle without being in `src/styles/ds/`, matched against
 * `<at-rule context> <selector>`. The bundle documents these families, but the app still styles
 * them in a per-screen stylesheet. Move a family into `ds/` and delete its line here.
 */
const BUNDLE_ONLY: { pattern: RegExp; reason: string }[] = [
  { pattern: /^\.page > \.card$/, reason: "the app stacks cards with `.card` in shell.css" },
  { pattern: /^\.data-table\.is-text /, reason: "text tables: Configurações, not moved yet" },
  { pattern: /^(@media \(max-width: 40rem\) )?\.settings-/, reason: "SettingsRow, settings.css" },
  { pattern: /^\.steps/, reason: "Steps, still in the Gerar stylesheet" },
  { pattern: /^\.recap/, reason: "Recap, resolver.css" },
  { pattern: /^(\.shortcuts|kbd$)/, reason: "Shortcuts, resolver.css" },
  { pattern: /^@media \((prefers-reduced-motion|forced-colors)/, reason: "global, base.css" },
];

describe("design-system bundle and src/styles/ds", () => {
  const bundle = parseRules(readFileSync(BUNDLE, "utf8"));
  const shipped = parseRules(
    readdirSync(DS_DIR)
      .sort()
      .map((name) => readFileSync(`${DS_DIR}/${name}`, "utf8"))
      .join("\n"),
  );

  it("define the same selectors", () => {
    const onlyInBundle = [...bundle.keys()].filter(
      (selector) => !shipped.has(selector) && !isBundleOnly(selector),
    );
    const onlyInApp = [...shipped.keys()].filter((selector) => !bundle.has(selector));
    expect({ onlyInBundle, onlyInApp }).toEqual({ onlyInBundle: [], onlyInApp: [] });
  });

  it("declare the same properties and values in every shared selector", () => {
    expect(differences(bundle, shipped)).toEqual([]);
  });

  it("list no exception that matches nothing in the bundle", () => {
    const unused = BUNDLE_ONLY.filter(
      ({ pattern }) => ![...bundle.keys()].some((selector) => pattern.test(selector)),
    );
    expect(unused.map(({ reason }) => reason)).toEqual([]);
  });
});

function isBundleOnly(selector: string): boolean {
  return BUNDLE_ONLY.some(({ pattern }) => pattern.test(selector));
}

function differences(bundle: Rules, shipped: Rules): string[] {
  return [...bundle.entries()]
    .filter(([selector]) => shipped.has(selector))
    .flatMap(([selector, bundled]) => {
      const applied = shipped.get(selector)!;
      const properties = new Set([...bundled.keys(), ...applied.keys()]);
      return [...properties]
        .filter((property) => bundled.get(property) !== applied.get(property))
        .map(
          (property) =>
            `${selector} { ${property}: bundle=${bundled.get(property)} app=${applied.get(property)} }`,
        );
    });
}
