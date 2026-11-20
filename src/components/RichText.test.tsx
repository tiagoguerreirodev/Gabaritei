import { render } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import RichText, { MAX_FORMULA, splitMath } from "./RichText";

const kinds = (text: string) => splitMath(text).map((p) => p.type);

describe("splitMath", () => {
  it("finds inline and display formulas in every delimiter style", () => {
    expect(splitMath("Seja $x^2$ e $$y$$ e \\(z\\) e \\[w\\].")).toEqual([
      { type: "text", text: "Seja " },
      { type: "math", text: "x^2", display: false },
      { type: "text", text: " e " },
      { type: "math", text: "y", display: true },
      { type: "text", text: " e " },
      { type: "math", text: "z", display: false },
      { type: "text", text: " e " },
      { type: "math", text: "w", display: true },
      { type: "text", text: "." },
    ]);
  });

  it("leaves currency alone", () => {
    expect(kinds("Custa R$ 10,00 e R$ 20,00.")).toEqual(["text"]);
    expect(kinds("De R$10 a R$20")).toEqual(["text"]);
    expect(kinds("Entre $ 5 e $ 6")).toEqual(["text"]);
  });

  it("keeps escaped, unclosed and multi-line inline delimiters as text", () => {
    expect(kinds("\\$x\\$")).toEqual(["text"]);
    expect(kinds("só $abre")).toEqual(["text"]);
    expect(kinds("$a\nb$")).toEqual(["text"]);
  });

  it("reads code spans and blocks, and doesn't look for math inside them", () => {
    expect(splitMath("`$x$` e \\(y\\)")).toEqual([
      { type: "code", text: "$x$", display: false },
      { type: "text", text: " e " },
      { type: "math", text: "y", display: false },
    ]);
    expect(splitMath("```\nSELECT 1;\n```")).toEqual([
      { type: "code", text: "\nSELECT 1;\n", display: true },
    ]);
  });
});

describe("RichText", () => {
  it("renders formulas with KaTeX as MathML", () => {
    const { container } = render(() => (
      <RichText text={"Frações $\\frac{1}{2}$ e $$\\begin{pmatrix}1&2\\\\3&4\\end{pmatrix}$$"} />
    ));
    expect(container.querySelectorAll("math")).toHaveLength(2);
    expect(container.querySelector("mfrac")).not.toBeNull();
    expect(container.querySelector("mtable")).not.toBeNull();
    expect(container.querySelector(".math-block")).not.toBeNull();
    // MathML only: no inline styles that the production CSP would block.
    expect(container.querySelector("[style]")).toBeNull();
  });

  it("renders HTML in the text as inert text", () => {
    const text = "<script>alert(1)</script> <img src=x onerror=alert(1)> `<b>`";
    const { container } = render(() => <RichText text={text} />);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
    expect(container.textContent).toBe(
      "<script>alert(1)</script> <img src=x onerror=alert(1)> <b>",
    );
  });

  it("doesn't trust links or HTML inside formulas", () => {
    const { container } = render(() => (
      <RichText text={"$\\href{javascript:alert(1)}{x}$ $\\htmlId{a}{b}$ $\\comandoInvalido{y}$"} />
    ));
    expect(container.querySelector("a")).toBeNull();
    expect(container.querySelector("#a")).toBeNull();
    expect(container.querySelector("[href]")).toBeNull();
    expect(container.textContent).toContain("comandoInvalido");
  });

  it("shows currency as plain text", () => {
    const { container } = render(() => <RichText text="Valor: R$ 10,00 por R$ 5,00" />);
    expect(container.querySelector("math")).toBeNull();
    expect(container.textContent).toBe("Valor: R$ 10,00 por R$ 5,00");
  });

  it("shows formulas over the size limit as code", () => {
    const huge = "x".repeat(MAX_FORMULA + 1);
    const { container } = render(() => <RichText text={`$${huge}$`} />);
    expect(container.querySelector("math")).toBeNull();
    expect(container.querySelector("code.math-error")?.textContent).toBe(huge);
  });
});
