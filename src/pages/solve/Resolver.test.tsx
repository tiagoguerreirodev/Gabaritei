import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it, vi } from "vitest";
import type { AttemptView } from "../../bindings/AttemptView";
import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { ResponseView } from "../../bindings/ResponseView";
import type { Reveal } from "../../bindings/Reveal";
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
const revealOf = (q: ResponseView): Reveal => {
  const answerId = keys[q.questionId]!;
  return {
    answerId,
    explanation: `Explicação secreta de ${q.questionId}`,
    refs: ["Lei 8.112/1990, art. 5º"],
    annulled: false,
    disputed: q.questionId === "q1",
    outcome: q.choice === null ? "blank" : q.choice === answerId ? "correct" : "wrong",
  };
};

/** A fake backend that applies the same rules as `attempts.rs`, recording each command. */
function backend(mode: FeedbackMode) {
  let view: AttemptView = {
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
    calls.push({ command, args });
    const position = args.position as number;
    switch (command) {
      case "attempt":
        return view;
      case "answer":
        return update(position, (q) => {
          const next = {
            ...q,
            choice: args.optionId as string | null,
            eliminated: q.eliminated.filter((e) => e !== args.optionId),
          };
          return mode === "study" && next.choice ? { ...next, reveal: revealOf(next) } : next;
        });
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
const option = (letter: string) =>
  within(card())
    .getAllByRole("button", { pressed: false })
    .concat(within(card()).queryAllByRole("button", { pressed: true }))
    .find((b) => b.querySelector(".option-letter")?.textContent === letter)!;

/** Asserts nothing about the answer is on screen. */
function expectNoKey() {
  expect(screen.queryByText("Gabarito")).not.toBeInTheDocument();
  expect(screen.queryByText(/Gabarito:/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Explicação secreta/)).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Gabarito e explicação" })).not.toBeInTheDocument();
  expect(document.querySelector(".option-key, .option-wrong")).toBeNull();
}

describe("Resolver", () => {
  it("renders no key before reveal, and reveals inline after answering in Estudo", async () => {
    const { user } = await open("study");
    expectNoKey();

    await user.click(option("A"));

    const reveal = await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(reveal).toHaveTextContent("Você errou. Gabarito: B.");
    expect(reveal).toHaveTextContent("Explicação secreta de q0");
    expect(reveal).toHaveTextContent("Lei 8.112/1990, art. 5º");
    // A revealed choice is locked.
    expect(option("C")).toBeDisabled();

    // The next question is still hidden.
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
    const reveal = await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(reveal).toHaveTextContent("Gabarito contestado na conferência");

    // A question the checker agreed with has no badge.
    await user.keyboard("{ArrowLeft}");
    await screen.findByRole("article", { name: "Questão 1 de 3" });
    await user.click(option("A"));
    const other = await screen.findByRole("region", { name: "Gabarito e explicação" });
    expect(other).not.toHaveTextContent("Gabarito contestado");
  });

  it("keeps everything hidden in Simulado until finishing, then shows the result", async () => {
    const { user, calls } = await open("exam");

    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    expectNoKey();
    // Choosing the selected option again clears it; choose it back.
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "false"));
    await user.keyboard("b");
    await waitFor(() => expect(option("B")).toHaveAttribute("aria-pressed", "true"));
    expectNoKey();

    await user.click(screen.getByRole("button", { name: "Finalizar" }));
    const dialog = await screen.findByRole("dialog", { name: "Finalizar tentativa?" });
    expect(within(dialog).getByTestId("finish-summary")).toHaveTextContent(
      "1 respondidas e 2 em branco.",
    );
    await user.click(within(dialog).getByRole("button", { name: "Finalizar" }));

    const result = await screen.findByRole("region", { name: "Resultado" });
    expect(result).toHaveTextContent("Acertos1");
    expect(result).toHaveTextContent("33,3%");
    expect(screen.getByRole("region", { name: "Gabarito e explicação" })).toHaveTextContent(
      "Você acertou. Gabarito: B.",
    );
    const finish = calls.find((c) => c.command === "finish")!;
    expect(finish.args).toEqual({ attemptId: "att", revision: 3 });
    // Finished: shortcuts no longer answer.
    await user.keyboard("a");
    expect(calls.filter((c) => c.command === "answer")).toHaveLength(3);
  });

  it("answers, eliminates, flags and moves with the keyboard", async () => {
    const { user, calls } = await open("exam");

    await user.keyboard("x");
    expect(screen.getByRole("status")).toHaveTextContent("Escolha a letra");
    await user.keyboard("c");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Restaurar alternativa C" })).toBeVisible(),
    );
    await user.keyboard("f");
    await waitFor(() => expect(screen.getByRole("button", { name: /Marcada/ })).toBeVisible());

    await user.keyboard("{ArrowRight}");
    await screen.findByRole("article", { name: "Questão 2 de 3" });
    // Certo/Errado: C and E.
    await user.keyboard("e");
    await waitFor(() => expect(option("E")).toHaveAttribute("aria-pressed", "true"));
    await user.keyboard("{ArrowLeft}");
    await screen.findByRole("article", { name: "Questão 1 de 3" });

    expect(calls.filter((c) => c.command !== "attempt")).toEqual([
      { command: "eliminate", args: { attemptId: "att", position: 0, optionId: "q0-o2" } },
      { command: "flag", args: { attemptId: "att", position: 0, flagged: true } },
      { command: "answer", args: { attemptId: "att", position: 1, optionId: "q1-e" } },
    ]);
    expect(screen.getByRole("button", { name: /Questão 1, marcada/ })).toBeVisible();
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

  it("annuls a revealed question and undoes it", async () => {
    const { user, calls } = await open("study");
    expect(screen.queryByRole("button", { name: "Anular questão" })).not.toBeInTheDocument();

    await user.click(option("B"));
    await user.click(await screen.findByRole("button", { name: "Anular questão" }));

    const reveal = await screen.findByRole("region", { name: "Gabarito e explicação" });
    await waitFor(() => expect(reveal).toHaveTextContent("Questão anulada"));
    expect(screen.getByRole("button", { name: /Questão 1, respondida, anulada/ })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Desfazer anulação" }));
    await waitFor(() => expect(reveal).toHaveTextContent("Você acertou."));

    expect(calls.filter((c) => ["annul", "unannul"].includes(c.command))).toEqual([
      { command: "annul", args: { attemptId: "att", position: 0 } },
      { command: "unannul", args: { attemptId: "att", position: 0 } },
    ]);
  });
});
