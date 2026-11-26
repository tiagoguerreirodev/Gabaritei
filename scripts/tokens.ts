/** Turns docs/design-system/tokens.json into src/styles/tokens.css. Pure: no file access. */

type ThemedValue = { light: string; dark: string };
type TokenValue = string | ThemedValue;
type Token = { name: string; value: TokenValue };

export type DesignTokens = {
  color: { tokens: Token[] };
  type: { families: Record<string, string> };
  spacing: { tokens: Token[] };
  radius: { tokens: Token[] };
  shadow: { tokens: Token[] };
  size: { tokens: Token[] };
  duration: { tokens: Token[] };
};

const HEADER =
  "/* Generated from docs/design-system/tokens.json by `npm run tokens`. Do not edit. */";
const EASING = "cubic-bezier(0.2, 0, 0, 1)";

export function renderTokensCss(tokens: DesignTokens): string {
  const all = allTokens(tokens);
  const lightLines = [
    ...all.map((token) => declaration(token.name, lightValue(token.value))),
    ...Object.entries(tokens.type.families).map(([name, value]) =>
      declaration(`font-${name}`, value),
    ),
    declaration("ease", EASING),
  ];
  const darkLines = all
    .filter((token): token is Token & { value: ThemedValue } => isThemed(token.value))
    .map((token) => declaration(token.name, token.value.dark));

  return [
    HEADER,
    block(":root", lightLines),
    "@media (prefers-color-scheme: dark) {",
    block(':root:not([data-theme="light"])', darkLines),
    "}",
    block(':root[data-theme="dark"]', darkLines),
  ].join("\n\n");
}

function allTokens(tokens: DesignTokens): Token[] {
  return [
    tokens.color,
    tokens.spacing,
    tokens.radius,
    tokens.shadow,
    tokens.size,
    tokens.duration,
  ].flatMap((group) => group.tokens);
}

function isThemed(value: TokenValue): value is ThemedValue {
  return typeof value !== "string";
}

function lightValue(value: TokenValue): string {
  return isThemed(value) ? value.light : value;
}

/** `"{accent}"` becomes `var(--accent)`; anything else is written as is. */
function declaration(name: string, value: string): string {
  const alias = /^\{([\w-]+)\}$/.exec(value)?.[1];
  return `--${name}: ${alias ? `var(--${alias})` : value};`;
}

function block(selector: string, lines: string[]): string {
  return `${selector} {\n${lines.map((line) => `  ${line}`).join("\n")}\n}`;
}
