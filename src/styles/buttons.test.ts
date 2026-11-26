/// <reference types="node" />
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/** Every non-test .tsx file under src/ (tests run from the repo root). */
function sourceFiles(dir = "src"): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return path.endsWith(".tsx") && !path.endsWith(".test.tsx") ? [path] : [];
  });
}

/** The opening `<button …>` tags of a file, skipping `>` inside `{…}` expressions. */
function buttonTags(source: string): string[] {
  const tags: string[] = [];
  for (const match of source.matchAll(/<button\b/g)) {
    let depth = 0;
    let end = match.index;
    while (end < source.length && !(source[end] === ">" && depth === 0)) {
      if (source[end] === "{") depth++;
      if (source[end] === "}") depth--;
      end++;
    }
    tags.push(source.slice(match.index, end + 1));
  }
  return tags;
}

/** Drops nested `{…}` expressions so only the tag's own attributes are left. */
function ownAttributes(tag: string) {
  let flat = tag;
  while (flat.includes("{")) flat = flat.replace(/\{[^{}]*\}/g, "X");
  return flat;
}

describe("buttons", () => {
  it("gives every button a class, so none falls back to the browser's look", () => {
    const unstyled = sourceFiles().flatMap((file) =>
      buttonTags(readFileSync(file, "utf8"))
        .filter((tag) => !/\sclass(List)?=/.test(ownAttributes(tag)))
        .map((tag) => `${file}: ${tag.split("\n")[0]}`),
    );
    expect(unstyled).toEqual([]);
  });
});
