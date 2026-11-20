/**
 * Safe rendering of question text (legacy `N-H1`, `O-H1`, `O-H2`).
 *
 * Text is split into plain text, code spans and formulas. Plain text and code are rendered
 * as text nodes, never as HTML. Formulas go through KaTeX with `trust: false` and size
 * limits, and only KaTeX's own output is inserted as markup.
 *
 * KaTeX renders MathML only: its HTML output needs inline `style` attributes, which the
 * production CSP (`style-src 'self'`) blocks, while every target webview renders MathML.
 */
import katex from "katex";
import { For, Match, Switch } from "solid-js";

export type TextPart =
  | { type: "text"; text: string }
  | { type: "code"; text: string; display: boolean }
  | { type: "math"; text: string; display: boolean };

/** Formulas longer than this are shown as code instead of being rendered. */
export const MAX_FORMULA = 20_000;

const isSpace = (char: string | undefined) => char === undefined || /\s/.test(char);

/**
 * Splits text into plain text, code spans (`` `x` ``, ```` ```x``` ````) and formulas
 * (`$x$`, `$$x$$`, `\(x\)`, `\[x\]`). Ported from the legacy `splitMath`:
 * - a backslash escapes a delimiter;
 * - `R$` is currency, not math;
 * - inline `$…$` must not start or end with whitespace and can't span lines;
 * - an unclosed delimiter stays plain text.
 */
export function splitMath(text: string): TextPart[] {
  const parts: TextPart[] = [];
  let start = 0;
  let i = 0;
  const escaped = (at: number) => {
    let backslashes = 0;
    while (at > 0 && text[--at] === "\\") backslashes++;
    return backslashes % 2 === 1;
  };
  const pushText = (end: number) => {
    if (end > start) parts.push({ type: "text", text: text.slice(start, end) });
  };

  while (i < text.length) {
    if (text[i] === "`" && !escaped(i)) {
      const marker = text.startsWith("```", i) ? "```" : "`";
      const end = text.indexOf(marker, i + marker.length);
      if (end !== -1) {
        pushText(i);
        parts.push({
          type: "code",
          text: text.slice(i + marker.length, end),
          display: marker.length === 3,
        });
        i = start = end + marker.length;
        continue;
      }
    }

    let open = "";
    let close = "";
    let display = false;
    if (text.startsWith("\\[", i) && !escaped(i)) {
      [open, close, display] = ["\\[", "\\]", true];
    } else if (text.startsWith("\\(", i) && !escaped(i)) {
      [open, close] = ["\\(", "\\)"];
    } else if (text[i] === "$" && !escaped(i) && text[i - 1] !== "R") {
      display = text.startsWith("$$", i);
      open = close = display ? "$$" : "$";
      if (!display && (isSpace(text[i + 1]) || text[i - 1] === "$")) {
        i++;
        continue;
      }
    }

    if (open) {
      let end = i + open.length;
      while ((end = text.indexOf(close, end)) !== -1) {
        const body = text.slice(i + open.length, end);
        const inlineOk = !isSpace(text[end - 1]) && !body.includes("\n");
        if (!escaped(end) && (display || inlineOk)) break;
        end += close.length;
      }
      if (end !== -1) {
        pushText(i);
        parts.push({ type: "math", text: text.slice(i + open.length, end), display });
        i = start = end + close.length;
        continue;
      }
    }
    i++;
  }
  pushText(text.length);
  return parts;
}

/** KaTeX markup for a formula, or `null` when it's too long to render. */
export function renderFormula(formula: string, display: boolean): string | null {
  if (formula.length > MAX_FORMULA) return null;
  try {
    return katex.renderToString(formula, {
      displayMode: display,
      output: "mathml",
      throwOnError: false,
      trust: false,
      strict: "ignore",
      maxExpand: 1000,
      maxSize: 20,
      macros: {},
    });
  } catch {
    return null;
  }
}

function Formula(props: { text: string; display: boolean }) {
  const html = () => renderFormula(props.text, props.display);
  return (
    <Switch>
      <Match when={html()}>
        {(markup) => (
          // Only KaTeX's output, rendered with `trust: false`, is inserted as markup.
          // eslint-disable-next-line solid/no-innerhtml
          <span class={props.display ? "math-block" : "math-inline"} innerHTML={markup()} />
        )}
      </Match>
      <Match when={!html()}>
        <code class="math-error">{props.text}</code>
      </Match>
    </Switch>
  );
}

export default function RichText(props: { text: string; class?: string }) {
  return (
    <span class={props.class ? `rich-text ${props.class}` : "rich-text"}>
      <For each={splitMath(props.text)}>
        {(part) => (
          <Switch>
            <Match when={part.type === "text"}>{part.text}</Match>
            <Match when={part.type === "code" && part}>
              {(code) => (
                <code class={code().display ? "code-block" : undefined}>{code().text}</code>
              )}
            </Match>
            <Match when={part.type === "math" && part}>
              {(math) => <Formula text={math().text} display={math().display} />}
            </Match>
          </Switch>
        )}
      </For>
    </span>
  );
}
