/// <reference types="node" />
import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";

const DS_DIR = "src/styles/ds";
const files = readdirSync(DS_DIR).map((name) => ({
  name,
  css: readFileSync(`${DS_DIR}/${name}`, "utf8"),
}));

describe("design-system styles", () => {
  it("use rem font sizes only, so the text-size setting scales them", () => {
    const withPixels = files.filter(({ css }) => /font-size:\s*[\d.]+px/.test(css));
    expect(withPixels.map(({ name }) => name)).toEqual([]);
  });

  it("load nothing remote", () => {
    const remote = files.filter(({ css }) => /@import\s+url\(|https?:/.test(css));
    expect(remote.map(({ name }) => name)).toEqual([]);
  });

  it("set no font size on the root element", () => {
    const onRoot = files.filter(({ css }) => /:root\s*\{[^}]*font-size/.test(css));
    expect(onRoot.map(({ name }) => name)).toEqual([]);
  });

  it("never mix outcome tints with color-mix()", () => {
    const mixed = files.filter(({ css }) => css.includes("color-mix("));
    expect(mixed.map(({ name }) => name)).toEqual([]);
  });

  it("no_legacy_token_names_remain", () => {
    const legacy =
      /--radius\)|--radius[ ;]|--shadow-toast|--shadow-modal|--flag-bar|--text-(xs|sm|base|md|lg|xl|2xl)\b|--leading-|--space-0-5|color-mix\(/;
    const stylesheets = readdirSync("src/styles", { recursive: true, encoding: "utf8" })
      .filter((name) => name.endsWith(".css"))
      .map((name) => ({ name, css: readFileSync(`src/styles/${name}`, "utf8") }));
    expect(stylesheets.filter(({ css }) => legacy.test(css)).map(({ name }) => name)).toEqual([]);
  });
});
