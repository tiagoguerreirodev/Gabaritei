import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import type { LearningOverview } from "../bindings/LearningOverview";
import Learning from "./Learning";

const path = { area: "Direito", matter: "Constitucional", topic: "Controle" };
const future = new Date(Date.now() + 5 * 86_400_000).toISOString();
const past = new Date(Date.now() - 3 * 86_400_000).toISOString();

const overview: LearningOverview = {
  due: 1,
  upcoming: 1,
  errors: 1,
  practice: 2,
  practiceErrors: 1,
  items: [
    {
      questionId: "q-secret-id",
      topics: [path],
      dueAt: past,
      isError: true,
      lastAttemptId: "att-3",
      lastAnsweredAt: past,
    },
    {
      questionId: "q2",
      topics: [{ area: "Português", matter: "Gramática", topic: "Crase" }],
      dueAt: future,
      isError: false,
      lastAttemptId: "att-4",
      lastAnsweredAt: past,
    },
  ],
};

function backend(calls: unknown[][] = []) {
  mockIPC((command, args) => {
    calls.push([command, args]);
    if (command === "learning_overview") return overview;
    if (command === "session_start") return { id: "att-new" };
    throw new Error(`Unexpected command: ${command}`);
  });
}

describe("Erros e revisões", () => {
  it("shows the counts and the list with topic and due date, and no answer key", async () => {
    backend();
    const { container } = render(() => <Learning />);

    const cards = await screen.findByRole("list", { name: "Resumo" });
    expect(within(cards).getByText("Para revisar agora").nextElementSibling).toHaveTextContent("1");
    expect(within(cards).getByText("Próximas").nextElementSibling).toHaveTextContent("1");
    expect(within(cards).getByText("Caderno de erros").nextElementSibling).toHaveTextContent("1");

    const rows = within(screen.getByRole("list", { name: "Questões" })).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("Direito › Constitucional › Controle");
    expect(rows[0]).toHaveTextContent(/Venceu em/);
    expect(rows[0]).toHaveTextContent("Erro");
    expect(rows[1]).toHaveTextContent(/Próxima revisão em/);
    expect(rows[1]).not.toHaveTextContent("Erro");

    // Nothing that could give an answer away, not even in attributes.
    for (const secret of ["gabarito", "answer", "explica", "q-secret-id"]) {
      expect(container.innerHTML.toLowerCase()).not.toContain(secret);
    }
  });

  it("says a review due later today is due today", async () => {
    const later = new Date();
    later.setHours(23, 59, 0, 0);
    const soon = { ...overview.items[1]!, dueAt: later.toISOString() };
    mockIPC(() => ({ ...overview, items: [soon] }));
    render(() => <Learning />);
    expect(await screen.findByText(/Vence hoje/)).toBeVisible();
  });

  it("links each row to the past attempt result", async () => {
    backend();
    const user = userEvent.setup();
    render(() => <Learning />);

    await user.click(
      await screen.findByRole("button", {
        name: "Ver resultado: Direito › Constitucional › Controle",
      }),
    );
    expect(window.location.hash).toBe("#/resolver?tentativa=att-3");
  });

  it("asks again with blanks included when the toggle is on", async () => {
    const calls: unknown[][] = [];
    backend(calls);
    const user = userEvent.setup();
    render(() => <Learning />);

    await screen.findByRole("list", { name: "Resumo" });
    await user.click(screen.getByLabelText("Incluir questões em branco"));
    await screen.findByRole("list", { name: "Resumo" });
    const overviewCalls = calls.filter(([c]) => c === "learning_overview").map(([, a]) => a);
    expect(overviewCalls).toEqual([{ includeBlanks: false }, { includeBlanks: true }]);
  });

  it("starts a practice session from the due reviews with a count on the button", async () => {
    const calls: unknown[][] = [];
    backend(calls);
    const user = userEvent.setup();
    render(() => <Learning />);

    await user.click(await screen.findByRole("button", { name: "Praticar agora (2)" }));
    await user.click(await screen.findByRole("button", { name: "Começar" }));
    const started = calls.find(([c]) => c === "session_start");
    expect(started?.[1]).toEqual({
      request: { source: { kind: "reviews" }, feedbackMode: "study" },
    });
    expect(window.location.hash).toBe("#/resolver?tentativa=att-new");
  });

  it("practices the error notebook with the blanks choice", async () => {
    const calls: unknown[][] = [];
    backend(calls);
    const user = userEvent.setup();
    render(() => <Learning />);

    await screen.findByRole("list", { name: "Resumo" });
    await user.click(screen.getByLabelText("Incluir questões em branco"));
    await user.click(await screen.findByRole("button", { name: "Praticar erros (1)" }));
    await user.click(await screen.findByRole("button", { name: "Começar" }));
    const started = calls.find(([c]) => c === "session_start");
    expect(started?.[1]).toMatchObject({
      request: { source: { kind: "errors", includeBlanks: true } },
    });
  });

  it("disables and counts error practice by what a session can take, not by the notebook", async () => {
    mockIPC(() => ({ ...overview, errors: 3, practiceErrors: 0 }));
    render(() => <Learning />);

    await screen.findByRole("list", { name: "Resumo" });
    expect(screen.getByRole("button", { name: "Praticar erros (0)" })).toBeDisabled();
  });

  it("has a pt-BR empty state and disabled practice buttons with nothing to review", async () => {
    mockIPC(() => ({ due: 0, upcoming: 0, errors: 0, practice: 0, practiceErrors: 0, items: [] }));
    render(() => <Learning />);

    expect(await screen.findByText("Nada para revisar por enquanto.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Praticar agora (0)" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Praticar erros (0)" })).toBeDisabled();
  });

  it("shows the backend's message when loading fails", async () => {
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "Falha ao ler o banco." }));
    render(() => <Learning />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao ler o banco.");
  });
});
