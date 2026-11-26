import { cleanup, render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it, vi } from "vitest";
import { api } from "../../api";
import type { AttemptView } from "../../bindings/AttemptView";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { ResponseView } from "../../bindings/ResponseView";
import type { RecheckView } from "../../bindings/RecheckView";
import type { Reveal } from "../../bindings/Reveal";
import { loadShortcuts } from "../../shortcuts";
import { focusMode } from "../../focusMode";
import { mergeExplanation } from "./mergeExplanation";
import Navigator from "./Navigator";
import Resolver from "./Resolver";

const multiple = (position: number): ResponseView => ({
  position,
  questionId: `q${position}`,
  type: "multiple",
  stem: `Enunciado ${position + 1}`,
  options: ["Alfa", "Beta", "Gama", "Delta", "Épsilon"].map((text, i) => ({
    id: `q${position}-o${i}`,
    text,
  })),
  image: null,
  imageAlt: null,
  choice: null,
  eliminated: [],
  flagged: false,
  reveal: null,
});

const boolean = (position: number): ResponseView => ({
  ...multiple(position),
  type: "boolean",
  options: [
    { id: `q${position}-c`, text: "Certo" },
    { id: `q${position}-e`, text: "Errado" },
  ],
});

/** Keys and explanations the fake backend only hands out when a question is revealed. */
const keys: Record<string, string> = { q0: "q0-o1", q1: "q1-e", q2: "q2-o4" };
/** Explanations the fake AI wrote (`q2` starts without one). */
const explanations: Record<string, string> = {};
const revealOf = (q: ResponseView): Reveal => {
  const answerId = keys[q.questionId]!;
  return {
    answerId,
    explanation:
      q.questionId === "q2" ? (explanations.q2 ?? "") : `Explicação secreta de ${q.questionId}`,
    refs: ["Lei 8.112/1990, art. 5º"],
    annulled: false,
    disputed: q.questionId === "q1",
    outcome: q.choice === null ? "blank" : q.choice === answerId ? "correct" : "wrong",
  };
};

/** What the fake AI answers to "Pedir revisão à IA" (the key proposed is option D). */
let recheckAnswer: RecheckView = {
  verdict: "wrong",
  correctedKey: "q0-o3",
  reasoning: "O art. 37 estabelece outra regra.",
  sources: ["CF/88, art. 37"],
};

/** Lets a test hold the AI's answer back; by default it answers at once. */
let recheckGate: Promise<void> = Promise.resolve();

/** Same for the backend's reply to "answer". */
let answerGate: Promise<void> = Promise.resolve();

/** What the fake backend answers to "keyboard_shortcuts". */
let shortcutsSaved = true;

function attemptView(mode: FeedbackMode): AttemptView {
  return {
    id: "att",
    sourceKind: "collection",
    sourceRef: "c1",
    title: "Caderno de teste",
    feedbackMode: mode,
    startedAt: "2026-11-20T10:00:00.000Z",
    finishedAt: null,
    revision: 0,
    questions: [multiple(0), boolean(1), multiple(2)],
    score: null,
  };
}

/** A fake backend that applies the same rules as `attempts.rs`, recording each command. */
function backend(mode: FeedbackMode, load: (view: AttemptView) => AttemptView = (view) => view) {
  let view = attemptView(mode);
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  const update = (position: number, change: (q: ResponseView) => ResponseView) => {
    view = {
      ...view,
      revision: view.revision + 1,
      questions: view.questions.map((q) => (q.position === position ? change(q) : q)),
    };
    return view;
  };
  mockIPC((command, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    // The shortcut setting is not part of the attempt; keep it out of the recorded calls.
    if (command === "keyboard_shortcuts") return shortcutsSaved;
    calls.push({ command, args });
    const position = args.position as number;
    switch (command) {
      case "attempt":
        return load(view);
      case "answer":
        return answerGate.then(() =>
          update(position, (q) => {
            const next = {
              ...q,
              choice: args.optionId as string | null,
              eliminated: q.eliminated.filter((e) => e !== args.optionId),
            };
            return mode === "study" && next.choice ? { ...next, reveal: revealOf(next) } : next;
          }),
        );
      case "eliminate":
        return update(position, (q) => {
          const id = args.optionId as string;
          return q.eliminated.includes(id)
            ? { ...q, eliminated: q.eliminated.filter((e) => e !== id) }
            : {
                ...q,
                eliminated: [...q.eliminated, id],
                choice: q.choice === id ? null : q.choice,
              };
        });
      case "flag":
        return update(position, (q) => ({ ...q, flagged: args.flagged as boolean }));
      case "finish":
        if (args.revision !== view.revision) {
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return Promise.reject({ message: "A tentativa mudou." });
        }
        view = {
          ...view,
          finishedAt: "2026-11-20T11:00:00.000Z",
          revision: view.revision + 1,
          questions: view.questions.map((q) => ({ ...q, reveal: revealOf(q) })),
          score: {
            correct: 1,
            wrong: 0,
            blank: 2,
            annulled: 0,
            evaluated: 3,
            achievement: 1 / 3,
            accuracy: 1,
          },
        };
        return view;
      case "annul":
      case "unannul":
        return update(position, (q) => {
          const annulled = command === "annul";
          const reveal = revealOf(q);
          return {
            ...q,
            reveal: { ...reveal, annulled, outcome: annulled ? "annulled" : reveal.outcome },
          };
        });
      case "explain_ask":
        explanations[view.questions[position]!.questionId] = "Explicação escrita pela IA.";
        return update(position, (q) => ({ ...q, reveal: revealOf(q) }));
      case "recheck_ask":
        return recheckGate.then(() => recheckAnswer);
      case "recheck_apply":
        keys[view.questions[position]!.questionId] = recheckAnswer.correctedKey!;
        return update(position, (q) => ({ ...q, reveal: revealOf(q) }));
      case "recheck_dismiss":
        return null;
      case "abandon":
        return null;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return calls;
}

async function open(mode: FeedbackMode, onLeave = () => {}) {
  const calls = backend(mode);
  const user = userEvent.setup();
  render(() => <Resolver attemptId="att" onLeave={onLeave} />);
  await screen.findByRole("article", { name: "Questão 1 de 3" });
  return { calls, user };
}

const card = () => screen.getByRole("article");

/** The question rail, always on screen beside the sheet. */
const rail = () => screen.getByRole("navigation", { name: "Questões" });
const option = (letter: string) =>
  within(card())
    .getAllByRole("button")
    .find((b) => b.matches(".option") && b.querySelector(".bubble")?.textContent === letter)!;

/** Asserts nothing about the answer is on screen. */
function expectNoKey() {
  expect(screen.queryByText("Gabarito")).not.toBeInTheDocument();
  expect(screen.queryByText(/Gabarito:/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Explicação secreta/)).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Gabarito e explicação" })).not.toBeInTheDocument();
  expect(card().querySelector(".is-correct, .is-wrong, .is-key")).toBeNull();
}

describe("Resolver", () => {
  it("reveals the key, explanation and references when an Estudo answer is confirmed", async () => {
    const { user } = await open("study");
    expectNoKey();

    await user.click(option("A"));
    await user.click(screen.getByRole("button", { name: /Confirmar resposta/ }));

    const reveal = await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(reveal).toHaveTextContent("Você errou. Gabarito: B.");
    expect(reveal).toHaveTextContent("Explicação secreta de q0");
    expect(reveal).toHaveTextContent("Lei 8.112/1990, art. 5º");
    // A revealed choice is locked.
    expect(option("C")).toBeDisabled();
  });

  it("keeps the next Estudo question hidden after revealing the previous answer", async () => {
    const { user } = await open("study");
    await user.click(option("A"));
    await user.click(screen.getByRole("button", { name: /Confirmar resposta/ }));
    await screen.findByRole("region", { name: "Gabarito e explicação" });

    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    expectNoKey();
  });

  it("flags a disputed generated question only after it is revealed", async () => {
    const { user } = await open("study");
    // Question 2 (q1) was disputed in the blind check.
    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    expect(screen.queryByText(/Gabarito contestado/)).not.toBeInTheDocument();

    // Question 2 is Certo/Errado.
    await user.click(option("C"));
    await user.keyboard("{Enter}");
    const reveal = await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(reveal).toHaveTextContent("Gabarito contestado na conferência");

    // A question the checker agreed with has no badge.
    await user.keyboard("{ArrowLeft}");
    await screen.findByRole("article", { name: "Questão 1 de 3" });
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    const other = await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(other).not.toHaveTextContent("Gabarito contestado");
  });

  it("asks the AI to re-check a revealed question and applies the key only after confirming", async () => {
    const { user, calls } = await open("study");
    expect(screen.queryByRole("button", { name: "Pedir revisão à IA" })).not.toBeInTheDocument();
    await user.click(option("A"));
    await user.keyboard("{Enter}");

    await user.click(await screen.findByRole("button", { name: "Pedir revisão à IA" }));
    const review = await screen.findByRole("region", { name: "Revisão da IA" });
    expect(review).toHaveTextContent("A IA discorda do gabarito.");
    expect(review).toHaveTextContent("Gabarito sugerido: D.");
    expect(review).toHaveTextContent("O art. 37 estabelece outra regra.");
    expect(review).toHaveTextContent("CF/88, art. 37");
    // Nothing changed yet.
    expect(calls.some((c) => c.command === "recheck_apply")).toBe(false);
    expect(screen.getByRole("region", { name: "Gabarito e explicação" })).toHaveTextContent(
      "Gabarito: B.",
    );

    await user.click(screen.getByRole("button", { name: "Aplicar novo gabarito (D)" }));
    await waitFor(() =>
      expect(screen.getByRole("region", { name: "Gabarito e explicação" })).toHaveTextContent(
        "Gabarito: D.",
      ),
    );
    expect(calls.filter((c) => c.command === "recheck_apply")).toHaveLength(1);
    expect(screen.queryByRole("region", { name: "Revisão da IA" })).not.toBeInTheDocument();
    keys.q0 = "q0-o1";
  });

  it("keeps the current key when the user dismisses the AI's proposal", async () => {
    const { user, calls } = await open("study");
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await user.click(await screen.findByRole("button", { name: "Pedir revisão à IA" }));
    await user.click(await screen.findByRole("button", { name: "Manter o gabarito atual" }));

    await waitFor(() => expect(calls.some((c) => c.command === "recheck_dismiss")).toBe(true));
    expect(calls.some((c) => c.command === "recheck_apply")).toBe(false);
    expect(screen.getByRole("region", { name: "Gabarito e explicação" })).toHaveTextContent(
      "Gabarito: B.",
    );
  });

  it("does not show a late re-check under another question", async () => {
    let release = () => {};
    const { user } = await open("study");
    // Reveal questions 1 and 2, so the card stays mounted when moving between them.
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    await user.click(option("C"));
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    await user.keyboard("{ArrowLeft}");
    await screen.findByRole("article", { name: "Questão 1 de 3" });

    recheckGate = new Promise<void>((resolve) => (release = resolve));
    await user.click(await screen.findByRole("button", { name: "Pedir revisão à IA" }));
    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    release();
    await new Promise((resolve) => setTimeout(resolve, 20));
    // Question 1's verdict must not appear under question 2.
    expect(screen.queryByRole("region", { name: "Revisão da IA" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pedir revisão à IA" })).toBeVisible();
    recheckGate = Promise.resolve();
  });

  it("offers no key change when the AI confirms the key", async () => {
    const { user } = await open("study");
    recheckAnswer = {
      verdict: "correct",
      correctedKey: null,
      reasoning: "Está certo.",
      sources: [],
    };
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await user.click(await screen.findByRole("button", { name: "Pedir revisão à IA" }));
    expect(await screen.findByText("A IA confirmou o gabarito.")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Aplicar novo gabarito/ })).not.toBeInTheDocument();
    recheckAnswer = {
      verdict: "wrong",
      correctedKey: "q0-o3",
      reasoning: "O art. 37 estabelece outra regra.",
      sources: ["CF/88, art. 37"],
    };
  });

  it("offers Explicar only on a revealed question without an explanation, and fills it in", async () => {
    const { user, calls } = await open("study");
    // Question 1 has an explanation: no button, revealed or not.
    expect(screen.queryByRole("button", { name: "Explicar" })).not.toBeInTheDocument();
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(screen.queryByRole("button", { name: "Explicar" })).not.toBeInTheDocument();

    // Question 3 has none, but it is hidden until answered.
    await user.keyboard("{ArrowRight}{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 3 de 3" });
    expect(screen.queryByRole("button", { name: "Explicar" })).not.toBeInTheDocument();

    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await user.click(await screen.findByRole("button", { name: "Explicar" }));
    expect(await screen.findByText("Explicação escrita pela IA.")).toBeVisible();
    expect(calls.filter((c) => c.command === "explain_ask")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Explicar" })).not.toBeInTheDocument();
    delete explanations.q2;
  });

  it.each([
    ["study", "Confirmar resposta (Enter)"],
    ["exam", "Finalizar"],
  ] as const)("keeps one primary button in %s mode", async (mode, name) => {
    await open(mode);
    const primaries = document.querySelectorAll(".btn-primary");
    expect([...primaries].map((button) => button.textContent)).toEqual([name]);
  });

  it("keeps Simulado feedback hidden while toggling an answer", async () => {
    const { user } = await open("exam");

    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    expectNoKey();
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "false"));
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    expectNoKey();
  });

  it("reveals Simulado feedback and the recap on finish", async () => {
    const { user, calls } = await open("exam");
    for (const pressed of ["true", "false", "true"]) {
      await user.keyboard("b");
      await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", pressed));
    }

    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    expect(within(dialog).getByTestId("finish-summary")).toHaveTextContent(
      "1 respondidas e 2 em branco.",
    );
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));

    const result = await screen.findByRole("region", { name: "Resultado" });
    expect(result).toHaveTextContent("Acerto entre respondidas");
    expect(result).toHaveTextContent("33,3%");
    expect(screen.getByRole("region", { name: "Gabarito e explicação" })).toHaveTextContent(
      "Você acertou. Gabarito: B.",
    );
    expect(calls.find((c) => c.command === "finish")?.args).toEqual({
      attemptId: "att",
      revision: 3,
    });
  });

  it("ignores answer shortcuts after finishing Simulado", async () => {
    const { user, calls } = await open("exam");
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));
    await screen.findByRole("region", { name: "Resultado" });

    await user.keyboard("a");

    expect(calls.filter((c) => c.command === "answer")).toHaveLength(1);
  });

  it("only selects an option in Estudo; Enter or the button confirms it", async () => {
    const { user, calls } = await open("study");
    const answers = () => calls.filter((c) => c.command === "answer");
    const confirm = () => screen.getByRole("button", { name: /Confirmar resposta/ });
    expect(confirm()).toBeDisabled();

    // Clicking and the letter key only mark the option.
    await user.click(option("A"));
    expect(option("A")).toHaveAttribute("aria-pressed", "true");
    expect(confirm()).toBeEnabled();
    await user.keyboard("c");
    expect(option("A")).toHaveAttribute("aria-pressed", "false");
    expect(option("C")).toHaveAttribute("aria-pressed", "true");
    expect(answers()).toHaveLength(0);
    expectNoKey();

    // Enter sends the marked option, exactly once.
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(answers()).toEqual([
      { command: "answer", args: { attemptId: "att", position: 0, optionId: "q0-o2" } },
    ]);
    expect(screen.queryByRole("button", { name: /Confirmar resposta/ })).not.toBeInTheDocument();
    await user.keyboard("{Enter}");
    expect(answers()).toHaveLength(1);
  });

  it("ignores Enter in Estudo while nothing is selected", async () => {
    const { user, calls } = await open("study");
    await user.keyboard("{Enter}");
    expect(calls.some((c) => c.command === "answer")).toBe(false);
  });

  it("drops an unconfirmed selection when moving to another question", async () => {
    const { user, calls } = await open("study");
    await user.keyboard("b");
    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    await user.keyboard("{ArrowLeft}");
    await screen.findByRole("article", { name: "Questão 1 de 3" });
    expect(option("B")).toHaveAttribute("aria-pressed", "false");
    await user.keyboard("{Enter}");
    expect(calls.some((c) => c.command === "answer")).toBe(false);
  });

  it("does not answer twice when Enter is pressed on a clicked option", async () => {
    const { user, calls } = await open("study");
    await user.click(option("B"));
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(calls.filter((c) => c.command === "answer")).toHaveLength(1);
  });

  it("does not show an unconfirmed mark as an answer after finishing", async () => {
    const { user } = await open("study");
    await user.keyboard("b");
    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));
    await screen.findByRole("region", { name: "Resultado" });
    expect(document.querySelector(".option.is-wrong")).toBeNull();
    expect(option("B")).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps Enter for the focused button unless it is the marked option", async () => {
    const { user, calls } = await open("study");
    const answers = () => calls.filter((c) => c.command === "answer");
    await user.keyboard("b");
    // Enter on another option or on a ✕ button only does what that button does.
    option("C").focus();
    await user.keyboard("{Enter}");
    expect(answers()).toHaveLength(0);
    expect(option("C")).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Eliminar alternativa D" }));
    await user.keyboard("{Enter}");
    await screen.findByRole("button", { name: "Eliminar alternativa D" });
    expect(answers()).toHaveLength(0);
    // Enter on the Finalizar button opens its dialog instead of answering.
    await user.click(option("C"));
    screen.getByRole("button", { name: "Finalizar" }).focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    expect(answers()).toHaveLength(0);
  });

  it("drops the mark of an option that gets eliminated", async () => {
    const { user, calls } = await open("study");
    await user.keyboard("b");
    await user.keyboard("x");
    await user.keyboard("b");
    await screen.findByRole("button", { name: "Restaurar alternativa B" });
    await user.keyboard("{Enter}");
    expect(calls.some((c) => c.command === "answer")).toBe(false);
  });

  it("ignores letters and Enter while the confirmed answer is in flight", async () => {
    const { user, calls } = await open("study");
    let release = () => {};
    answerGate = new Promise<void>((resolve) => (release = resolve));
    await user.keyboard("b");
    await user.keyboard("{Enter}c{Enter}");
    release();
    answerGate = Promise.resolve();
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(calls.filter((c) => c.command === "answer")).toHaveLength(1);
    expect(option("C")).toHaveAttribute("aria-pressed", "false");
    expect(option("B")).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps Enter from toggling an option's answer in Simulado", async () => {
    const { user, calls } = await open("exam");
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    await user.keyboard("{Enter}");
    expect(calls.filter((c) => c.command === "answer")).toHaveLength(1);
    expect(option("B")).toHaveAttribute("aria-pressed", "true");
  });

  it("eliminates an option with X and its letter", async () => {
    const { user, calls } = await open("exam");

    await user.keyboard("x");
    expect(screen.getByRole("status")).toHaveTextContent("Escolha a letra");
    await user.keyboard("c");

    expect(await screen.findByRole("button", { name: "Restaurar alternativa C" })).toBeVisible();
    expect(calls.filter((c) => c.command !== "attempt")).toEqual([
      { command: "eliminate", args: { attemptId: "att", position: 0, optionId: "q0-o2" } },
    ]);
  });

  it("marks a question with F", async () => {
    const { user, calls } = await open("exam");

    await user.keyboard("f");

    expect(await screen.findByRole("button", { name: /Questão 1, marcada/ })).toBeVisible();
    expect(screen.getByRole("button", { name: "Marcada" })).toBeVisible();
    expect(calls.filter((c) => c.command !== "attempt")).toEqual([
      { command: "flag", args: { attemptId: "att", position: 0, flagged: true } },
    ]);
  });

  it("moves between questions with the arrows", async () => {
    const { user, calls } = await open("exam");

    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    await user.keyboard("{ArrowLeft}");

    await screen.findByRole("article", { name: "Questão 1 de 3" });
    expect(calls.filter((c) => c.command !== "attempt")).toEqual([]);
  });

  it("answers a Certo/Errado question with its option letter", async () => {
    const { user, calls } = await open("exam");
    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });

    await user.keyboard("e");

    await waitFor(() => expect(option("E")).toHaveAttribute("aria-pressed", "true"));
    expect(calls.filter((c) => c.command !== "attempt")).toEqual([
      { command: "answer", args: { attemptId: "att", position: 1, optionId: "q1-e" } },
    ]);
    expect(screen.getByRole("button", { name: /Questão 2, respondida/ })).toBeVisible();
    expect(screen.getByTestId("progress")).toHaveTextContent("1 de 3 respondidas");
  });

  it("cancels a pending elimination when the user does something else", async () => {
    const { user, calls } = await open("exam");

    // X, then the question map: the next letter answers on the new question.
    await user.keyboard("x");
    expect(screen.getByRole("status")).toHaveTextContent("Escolha a letra");
    await user.click(screen.getByRole("button", { name: "Questão 3" }));
    await screen.findByRole("article", { name: "Questão 3 de 3" });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    await user.keyboard("a");

    // X, then a click on an option: that answers, and the next letter answers too.
    await user.keyboard("x");
    await user.click(option("B"));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    await user.keyboard("c");

    await waitFor(() => expect(calls.filter((c) => c.command !== "attempt")).toHaveLength(3));
    expect(calls.map((c) => c.command).filter((c) => c !== "attempt")).toEqual([
      "answer",
      "answer",
      "answer",
    ]);
  });

  it("ignores a held key", async () => {
    const { user, calls } = await open("exam");
    await user.keyboard("{b>4/}");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    await user.keyboard("{f>4/}");
    await waitFor(() => expect(screen.getByRole("button", { name: /Marcada/ })).toBeVisible());
    expect(calls.filter((c) => c.command !== "attempt").map((c) => c.command)).toEqual([
      "answer",
      "flag",
    ]);
  });

  it("ignores shortcuts while a dialog is open", async () => {
    const { user, calls } = await open("exam");
    await user.click(screen.getByRole("button", { name: "Abandonar" }));
    await screen.findByRole("dialog", { name: "Abandonar tentativa?" });
    await user.keyboard("a");
    expect(calls.some((c) => c.command === "answer")).toBe(false);
  });

  it("abandons after confirmation and leaves the screen", async () => {
    const onLeave = vi.fn();
    const { user, calls } = await open("study", onLeave);

    await user.click(screen.getByRole("button", { name: "Abandonar" }));
    const dialog = await screen.findByRole("dialog", { name: "Abandonar tentativa?" });
    expect(dialog).toHaveTextContent("não entram no histórico");
    await user.click(within(dialog).getByRole("button", { name: "Abandonar" }));

    await waitFor(() => expect(onLeave).toHaveBeenCalled());
    expect(calls.at(-1)).toEqual({ command: "abandon", args: { attemptId: "att" } });
  });

  it("hides annulment before reveal", async () => {
    await open("study");
    expect(screen.queryByRole("button", { name: "Anular questão" })).not.toBeInTheDocument();
  });

  async function annulFirstQuestion(user: ReturnType<typeof userEvent.setup>) {
    await user.click(option("B"));
    await user.keyboard("{Enter}");
    await user.click(await screen.findByRole("button", { name: "Anular questão" }));
    await screen.findByRole("button", { name: "Desfazer anulação" });
  }

  it("annuls a revealed question", async () => {
    const { user, calls } = await open("study");
    await annulFirstQuestion(user);

    expect(screen.getByRole("region", { name: "Gabarito e explicação" })).toHaveTextContent(
      "Questão anulada",
    );
    expect(screen.getByRole("button", { name: /Questão 1, respondida, anulada/ })).toBeVisible();
    expect(calls.at(-1)).toEqual({ command: "annul", args: { attemptId: "att", position: 0 } });
  });

  it("restores an annulled question", async () => {
    const { user, calls } = await open("study");
    await annulFirstQuestion(user);

    const reveal = screen.getByRole("region", { name: "Gabarito e explicação" });
    await user.click(screen.getByRole("button", { name: "Desfazer anulação" }));
    await waitFor(() => expect(reveal).toHaveTextContent("Você acertou."));
    expect(calls.at(-1)).toEqual({ command: "unannul", args: { attemptId: "att", position: 0 } });
  });
});

describe("Resolver: sheet outcomes", () => {
  const outcomeMarks = () => document.querySelectorAll(".is-correct, .is-wrong, .is-key");

  it("a_selected_option_shows_no_outcome_before_confirming", async () => {
    const { user } = await open("study");
    await user.click(option("B"));
    expect(option("B")).toHaveClass("is-selected");
    expect(outcomeMarks()).toHaveLength(0);
    expect(screen.queryByRole("region", { name: "Gabarito e explicação" })).toBeNull();
  });

  it("exam_mode_shows_no_outcomes_until_finish", async () => {
    const { user } = await open("exam");
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    expect(outcomeMarks()).toHaveLength(0);
    expectNoKey();

    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(option("B")).toHaveClass("is-correct");
  });

  it("the_missed_key_is_marked_gabarito_after_reveal", async () => {
    const { user } = await open("study");
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });

    expect(option("B")).toHaveClass("is-correct");
    expect(within(option("B")).getByText("Gabarito", { exact: false })).toHaveClass("option-note");
    expect(option("A")).toHaveClass("is-wrong");
    expect(within(option("A")).getByText("Sua resposta", { exact: false })).toHaveClass(
      "option-note",
    );
  });

  it("an_annulled_question_shows_no_outcome_marks", async () => {
    const { user } = await open("study");
    await user.click(option("A"));
    await user.keyboard("{Enter}");
    await user.click(await screen.findByRole("button", { name: "Anular questão" }));
    await screen.findByRole("button", { name: "Desfazer anulação" });

    expect(card().querySelectorAll(".is-correct, .is-wrong, .is-key, .option-note")).toHaveLength(
      0,
    );
    expect(screen.getByRole("button", { name: /Questão 1.*anulada/ })).toHaveClass("is-annulled");
  });
});

describe("Resolver: answer-sheet UI", () => {
  const live = () => screen.getByTestId("live-region");

  it("shows a progressbar with a text label that follows the answers", async () => {
    const { user } = await open("exam");
    const bar = screen.getByRole("progressbar", { name: "Progresso da tentativa" });
    expect(bar).toHaveAttribute("aria-valuemin", "0");
    expect(bar).toHaveAttribute("aria-valuemax", "3");
    expect(bar).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getByTestId("progress")).toHaveTextContent("0 de 3 respondidas");

    await user.keyboard("b");
    await waitFor(() => expect(bar).toHaveAttribute("aria-valuenow", "1"));
    expect(bar).toHaveAttribute("aria-valuetext", "1 de 3 respondidas");
    expect(screen.getByTestId("progress")).toHaveTextContent("1 de 3 respondidas");
  });

  it("announces the question on navigation and the outcome after confirming, never the key", async () => {
    const { user } = await open("study");
    expect(live()).toHaveAttribute("aria-live", "polite");
    expectNoKey();
    expect(live()).not.toHaveTextContent(/Gabarito|acertou|errou/);

    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(live()).toHaveTextContent("Questão 2 de 3."));
    await user.keyboard("{ArrowLeft}");
    await waitFor(() => expect(live()).toHaveTextContent("Questão 1 de 3."));

    await user.keyboard("b");
    expect(live()).toHaveTextContent("Questão 1 de 3.");
    await user.keyboard("{Enter}");
    // Focus moves to the revealed answer, which reads the outcome; the live region stays quiet.
    const reveal = await screen.findByRole("region", { name: "Gabarito e explicação" });
    await waitFor(() => expect(reveal).toHaveFocus());
    expect(live()).toHaveTextContent("Questão 1 de 3.");
    expect(live()).not.toHaveTextContent(/Gabarito|secreta|acertou|errou/);
  });

  it("marks a wrong answer with a shape, not just a colour", async () => {
    const { user } = await open("study");
    await user.keyboard("a");
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(live()).not.toHaveTextContent(/errou/);

    const wrong = within(card()).getByText("Sua resposta", { exact: false });
    expect(wrong.querySelector("[aria-hidden=true]")).toHaveTextContent("✗");
    const key = within(card()).getByText("Gabarito", { selector: ".option-note", exact: false });
    expect(key.querySelector("[aria-hidden=true]")).toHaveTextContent("✓");
    const nav = screen.getByRole("button", { name: "Questão 1, respondida, errada" });
    expect(nav.querySelector(".bubble-mark")).toHaveTextContent("✗");
  });

  it("marks flagged and annulled questions in the navigator with shapes", async () => {
    const { user } = await open("study");
    await user.keyboard("f");
    const flagged = await screen.findByRole("button", { name: "Questão 1, marcada" });
    expect(flagged.querySelector(".bubble-mark.is-flag")).toHaveTextContent("⚑");

    await user.keyboard("b");
    await user.keyboard("{Enter}");
    await user.click(await screen.findByRole("button", { name: "Anular questão" }));
    const annulled = await screen.findByRole("button", { name: /Questão 1.*anulada/ });
    expect(annulled).toHaveClass("is-annulled");
  });

  it("ignores letter shortcuts with modifier keys and while typing in a field", async () => {
    const { user, calls } = await open("exam");
    const answers = () => calls.filter((c) => c.command === "answer");

    await user.keyboard("{Control>}a{/Control}");
    await user.keyboard("{Meta>}b{/Meta}");
    await user.keyboard("{Alt>}c{/Alt}");
    await user.keyboard("{Shift>}d{/Shift}");
    expect(answers()).toHaveLength(0);

    const field = document.createElement("input");
    document.body.append(field);
    field.focus();
    await user.keyboard("a");
    field.remove();
    expect(answers()).toHaveLength(0);

    await user.keyboard("a");
    await waitFor(() => expect(answers()).toHaveLength(1));
  });

  it("turns the letter shortcuts off with the setting but keeps the arrows", async () => {
    shortcutsSaved = false;
    try {
      const { user, calls } = await open("exam");
      await user.click(screen.getByRole("button", { name: "Atalhos" }));
      await screen.findByText(/desativados em Configurações/);
      await user.keyboard("a");
      await user.keyboard("f");
      expect(calls.some((c) => ["answer", "flag"].includes(c.command))).toBe(false);
      await user.keyboard("{ArrowRight}");
      await screen.findByRole("article", { name: "Questão 2 de 3" });
    } finally {
      shortcutsSaved = true;
      await loadShortcuts();
    }
  });

  it("shows the shortcut help in a toggle button instead of a long hint line", async () => {
    const { user } = await open("study");
    expect(screen.queryByText(/Atalhos:/)).not.toBeInTheDocument();
    const toggle = screen.getByRole("button", { name: "Atalhos" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const help = screen.getByRole("group", { name: "Atalhos de teclado" });
    expect(help).toHaveTextContent("Enter confirma");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(help).not.toBeVisible();
    expect(toggle).toHaveAttribute("aria-controls", "shortcuts-help");
    expect(document.getElementById("shortcuts-help")).toBe(help);
  });

  it("closes the shortcut help with Escape and returns the focus from inside the panel", async () => {
    const { user } = await open("study");
    const toggle = screen.getByRole("button", { name: "Atalhos" });
    expect(toggle).toHaveAttribute("aria-controls", "shortcuts-help");
    expect(document.getElementById("shortcuts-help")).not.toBeNull();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const help = screen.getByRole("group", { name: "Atalhos de teclado" });
    help.setAttribute("tabindex", "-1");
    help.focus();
    expect(help).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveFocus();
  });

  it("closes the shortcut help with Escape without stealing the focus from outside it", async () => {
    const { user } = await open("study");
    const toggle = screen.getByRole("button", { name: "Atalhos" });
    await user.click(toggle);
    const outside = screen.getByRole("button", { name: "Finalizar" });
    outside.focus();
    await user.keyboard("{Escape}");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(outside).toHaveFocus();
  });

  it("closes the shortcut help on a click outside it, but not inside", async () => {
    const { user } = await open("study");
    const toggle = screen.getByRole("button", { name: "Atalhos" });
    await user.click(toggle);
    await user.click(screen.getByRole("group", { name: "Atalhos de teclado" }));
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(document.body);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  it("plays the right-answer pop only right after confirming, not when navigating back", async () => {
    const { user } = await open("study");
    await user.keyboard("b");
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Gabarito e explicação" });
    await waitFor(() =>
      expect(document.querySelector(".bubble.is-correct.is-popping")).not.toBeNull(),
    );
    await user.keyboard("{ArrowRight}");
    await user.keyboard("{ArrowLeft}");
    expect(document.querySelector(".option.is-correct")).not.toBeNull();
    expect(document.querySelector(".is-popping")).toBeNull();
  });

  it("does not hint at outcomes in the navigator before reveal, in Estudo or Simulado", async () => {
    const words = /certa|errada|em branco|anulada|correct|wrong|blank|annulled|✓|✗/i;
    for (const mode of ["study", "exam"] as const) {
      const { user } = await open(mode);
      await user.keyboard("a");
      if (mode === "study") {
        // Marked but not confirmed: still nothing to reveal.
        expect(screen.getByRole("button", { name: "Questão 1" })).toBeInTheDocument();
      } else {
        await waitFor(() =>
          expect(screen.getByRole("button", { name: "Questão 1, respondida" })).toBeInTheDocument(),
        );
      }
      for (const bubble of within(rail()).getAllByRole("button")) {
        expect(bubble.getAttribute("aria-label")).not.toMatch(words);
        expect(bubble.className).not.toMatch(words);
        expect(bubble.textContent).not.toMatch(words);
        expect(bubble.className).not.toMatch(/is-correct|is-wrong|is-annulled/);
      }
      cleanup();
    }
  });

  it("shows a recap on finishing, without a comparison", async () => {
    const { user } = await open("exam");
    await user.keyboard("b");
    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));

    const recap = await screen.findByRole("region", { name: "Resultado" });
    expect(screen.getByTestId("recap-points")).toHaveTextContent("1 de 3 pontos");
    expect(recap).toHaveTextContent("33,3% de aproveitamento");
    const counts = within(recap).getByRole("list", { name: "Contagem por resultado" });
    expect(counts).toHaveTextContent("1 certa");
    expect(counts).toHaveTextContent("0 erradas");
    expect(counts).toHaveTextContent("2 em branco");
    expect(counts).toHaveTextContent("0 anuladas");
    expect(recap).toHaveFocus();
    await waitFor(() => expect(live()).toHaveTextContent("Tentativa finalizada."));
  });

  it("keeps announcing the question number while reviewing a finished attempt", async () => {
    const { user } = await open("exam");
    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));
    await screen.findByRole("region", { name: "Resultado" });
    expect(live()).toHaveTextContent("Tentativa finalizada.");
    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(live()).toHaveTextContent("Questão 2 de 3."));
  });
});

describe("Resolver: headings", () => {
  it("has one h1 while loading", () => {
    backend("study");
    render(() => <Resolver attemptId="att" onLeave={() => {}} />);
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Resolver");
  });

  it("has one h1 when loaded", async () => {
    backend("study");
    render(() => <Resolver attemptId="att" onLeave={() => {}} />);
    await screen.findByRole("article", { name: "Questão 1 de 3" });
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Caderno de teste");
  });

  it("keeps the h1 when the attempt fails to load", async () => {
    mockIPC(() =>
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      Promise.reject({ message: "Tentativa não encontrada." }),
    );
    render(() => <Resolver attemptId="x" onLeave={() => {}} />);
    await screen.findByText(/Tentativa não encontrada/);
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Resolver");
  });
});

it("retries an attempt load and resumes at the first unanswered question", async () => {
  let loads = 0;
  backend("exam", (view) => {
    if (loads++ === 0) throw new Error("Falha temporária.");
    return {
      ...view,
      questions: [{ ...multiple(0), choice: "q0-o1" }, boolean(1), multiple(2)],
    };
  });
  const user = userEvent.setup();
  render(() => <Resolver attemptId="att" onLeave={() => {}} />);

  await user.click(await screen.findByRole("button", { name: "Tentar novamente" }));

  expect(await screen.findByRole("article", { name: "Questão 2 de 3" })).toBeVisible();
  expect(loads).toBe(2);
});

it("merges an explanation while preserving newer answers and key changes", () => {
  const first = multiple(0);
  const outdatedReveal = revealOf(first);
  const currentReveal = { ...outdatedReveal, answerId: "q0-o3", explanation: "", refs: [] };
  const answeredFirst = { ...first, choice: "q0-o0", reveal: currentReveal };
  const answeredSecond = { ...boolean(1), choice: "q1-e" };
  const current: AttemptView = {
    ...attemptView("study"),
    revision: 4,
    questions: [answeredFirst, answeredSecond],
  };
  const explanation = "Explicação escrita pela IA.";
  const refs = ["Fonte nova"];
  const explained: AttemptView = {
    ...current,
    revision: 2,
    questions: [{ ...first, reveal: { ...outdatedReveal, explanation, refs } }, boolean(1)],
  };

  expect(mergeExplanation(current, explained, first.position)).toEqual({
    ...current,
    questions: [
      { ...answeredFirst, reveal: { ...currentReveal, explanation, refs } },
      answeredSecond,
    ],
  });
});

it("retries the attempt after a load failure with unrevealed questions still hidden", async () => {
  backend("study");
  const fetchAttempt = vi.spyOn(api, "attempt").mockRejectedValueOnce(new Error("Falhou."));
  const user = userEvent.setup();
  render(() => <Resolver attemptId="att" onLeave={() => {}} />);

  try {
    expect(await screen.findByRole("alert")).toHaveTextContent("Falhou.");
    await user.click(screen.getByRole("button", { name: "Tentar novamente" }));
    await screen.findByRole("article", { name: "Questão 1 de 3" });
    expect(fetchAttempt).toHaveBeenCalledTimes(2);
    expectNoKey();
  } finally {
    fetchAttempt.mockRestore();
  }
});

describe("Navigator", () => {
  const many = (count: number): ResponseView[] =>
    Array.from({ length: count }, (_, position) => multiple(position));

  it("the_rail_groups_questions_by_twenty, every bubble keyboard-reachable", async () => {
    const onSelect = vi.fn();
    const user = userEvent.setup();
    render(() => <Navigator questions={many(45)} current={0} onSelect={onSelect} />);

    for (const label of ["1–20", "21–40", "41–45"]) {
      expect(screen.getByRole("group", { name: `Questões ${label}` })).toHaveTextContent(
        `Q ${label}`,
      );
    }
    expect(screen.getAllByRole("button")).toHaveLength(45);
    const last = screen.getByRole("button", { name: "Questão 45" });
    last.focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith(44);
  });

  it("the_rail_stays_a_labelled_nav with its progress and a legend in words", () => {
    render(() => <Navigator questions={many(8)} current={0} onSelect={() => {}} />);
    const nav = screen.getByRole("navigation", { name: "Questões" });
    expect(within(nav).getByRole("progressbar")).toHaveAttribute("aria-valuemax", "8");
    expect(nav).toHaveTextContent("Marcada para revisar");
    expect(screen.getAllByRole("group")).toHaveLength(1);
  });

  it("rail_bubbles_name_their_state", () => {
    const questions = many(5);
    questions[3] = { ...questions[3]!, choice: "q3-o0", flagged: true };
    render(() => <Navigator questions={questions} current={3} onSelect={() => {}} />);
    const bubble = screen.getByRole("button", { name: "Questão 4, respondida, marcada" });
    expect(bubble).toHaveAttribute("aria-current", "step");
    expect(bubble).toHaveClass("is-filled", "is-current");
  });
});

describe("Resolver: focus mode", () => {
  it("is on while the attempt is open, and the Modo foco button turns it off and on", async () => {
    const { user } = await open("exam");
    const button = screen.getByRole("button", { name: "Modo foco" });
    expect(focusMode()).toBe(true);
    expect(button).toHaveAttribute("aria-pressed", "true");

    await user.click(button);
    expect(focusMode()).toBe(false);
    expect(button).toHaveAttribute("aria-pressed", "false");

    await user.click(button);
    expect(focusMode()).toBe(true);
  });

  it("ends when the attempt is finished", async () => {
    const { user } = await open("exam");
    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));
    await screen.findByRole("button", { name: "Voltar ao Banco" });
    expect(focusMode()).toBe(false);
    expect(screen.queryByRole("button", { name: "Modo foco" })).not.toBeInTheDocument();
  });

  it("ends when the screen closes, and the next attempt starts focused again", async () => {
    const { user } = await open("exam");
    await user.click(screen.getByRole("button", { name: "Modo foco" }));
    cleanup();
    expect(focusMode()).toBe(false);

    await open("exam");
    expect(focusMode()).toBe(true);
  });
});
