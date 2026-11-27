/** `<at-rule context> <single selector>` → property → value, after merging repeated rules. */
export type Rules = Map<string, Map<string, string>>;

/**
 * Flattens CSS so two stylesheets compare by what each selector ends up declaring, however the
 * rules are grouped or split. Later declarations win, as in the cascade for equal specificity.
 */
export function parseRules(css: string): Rules {
  const rules: Rules = new Map();
  collect(stripComments(css), "", rules);
  return rules;
}

function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, "");
}

function collect(css: string, context: string, rules: Rules): void {
  let position = 0;
  while (position < css.length) {
    const open = css.indexOf("{", position);
    if (open === -1) break;
    const prelude = squash(css.slice(position, open));
    const close = matchingBrace(css, open);
    const body = css.slice(open + 1, close);
    if (prelude.startsWith("@")) {
      collect(body, `${context}${prelude} `, rules);
    } else {
      addRule(rules, context, prelude, body);
    }
    position = close + 1;
  }
}

function addRule(rules: Rules, context: string, selectorList: string, body: string): void {
  for (const selector of selectorList.split(",").map(squash)) {
    const key = `${context}${selector}`;
    const merged = rules.get(key) ?? new Map<string, string>();
    for (const [property, value] of declarations(body)) merged.set(property, value);
    rules.set(key, merged);
  }
}

function matchingBrace(css: string, open: number): number {
  let depth = 0;
  for (let index = open; index < css.length; index++) {
    if (css[index] === "{") depth++;
    if (css[index] === "}" && --depth === 0) return index;
  }
  throw new Error("Unbalanced braces in CSS");
}

function declarations(body: string): [string, string][] {
  return body
    .split(";")
    .map(squash)
    .filter((declaration) => declaration !== "")
    .map((declaration) => {
      const colon = declaration.indexOf(":");
      return [declaration.slice(0, colon).trim(), declaration.slice(colon + 1).trim()];
    });
}

function squash(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}
