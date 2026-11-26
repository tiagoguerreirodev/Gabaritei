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
  refreshAt: null,
  dueInOpenAttempt: 0,
  upcoming: 1,
  errors: 1,
  practice: 2,
  practiceErrors: 1,
  items: [
    {
      questionId: "q-secret-id",
      topics: [path],
      review: { at: past, state: "overdue", intervalDays: 7 },
      isError: true,
      lastAttemptId: "att-3",
      lastAnsweredAt: past,
      accuracy: 0.4,
    },
    {
      questionId: "q2",
      topics: [{ area: "Português", matter: "Gramática", topic: "Crase" }],
      review: { at: future, state: "upcoming", intervalDays: 1 },
      isError: false,
      lastAttemptId: "att-4",
      lastAnsweredAt: past,
      accuracy: 0.9,
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
  it("opens on the first tab with rows and counts every tab", async () => {
    backend();
    const { container } = render(() => <Learning />);

    const tabs = await screen.findByRole("tablist");
    expect(within(tabs).getByRole("tab", { name: /Para hoje\s*0/ })).toBeVisible();
    expect(within(tabs).getByRole("tab", { name: /Atrasadas\s*1/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(tabs).getByRole("tab", { name: /Próximas\s*1/ })).toBeVisible();
    expect(within(tabs).getByRole("tab", { name: /Caderno de erros\s*1/ })).toBeVisible();

    const rows = within(screen.getByRole("list", { name: "Questões" })).getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("Controle");
    expect(rows[0]).toHaveTextContent("Direito › Constitucional");
    expect(rows[0]).toHaveTextContent("revisão de 7 dias");
    expect(rows[0]).toHaveTextContent(/Venceu em/);
    expect(rows[0]).toHaveTextContent("40% de acerto");
    expect(rows[0]).toHaveTextContent("Erro");
    expect(within(rows[0]!).getAllByRole("button")).toHaveLength(1);

    // Nothing that could give an answer away, not even in attributes.
    for (const secret of ["gabarito", "answer", "explica", "q-secret-id"]) {
      expect(container.innerHTML.toLowerCase()).not.toContain(secret);
    }
  });

  it("shows upcoming reviews with the singular interval and a good accuracy", async () => {
    backend();
    const user = userEvent.setup();
    render(() => <Learning />);

    await user.click(await screen.findByRole("tab", { name: /Próximas/ }));
    const row = within(screen.getByRole("list", { name: "Questões" })).getByRole("listitem");
    expect(row).toHaveTextContent(/Próxima revisão em/);
    expect(row).toHaveTextContent("revisão de 1 dia");
    expect(row).toHaveTextContent("90% de acerto");
    expect(row).not.toHaveTextContent("Erro");
  });

  it("says a review due later today is due today", async () => {
    const soon = {
      ...overview.items[1]!,
      review: { at: past, state: "today" as const, intervalDays: 3 },
    };
    mockIPC(() => ({ ...overview, items: [soon] }));
    render(() => <Learning />);
    expect(await screen.findByRole("tab", { name: /Para hoje\s*1/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
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

    await user.click(await screen.findByRole("tab", { name: /Caderno de erros/ }));
    await user.click(screen.getByLabelText("Incluir questões em branco"));
    await screen.findByRole("tab", { name: /Caderno de erros/ });
    const overviewCalls = calls.filter(([c]) => c === "learning_overview").map(([, a]) => a);
    expect(overviewCalls).toEqual([{ includeBlanks: false }, { includeBlanks: true }]);
  });

  it("starts a practice session from the due reviews with a count on the button", async () => {
    const calls: unknown[][] = [];
    backend(calls);
    const user = userEvent.setup();
    render(() => <Learning />);

    await user.click(await screen.findByRole("button", { name: "Revisar tudo de hoje (2)" }));
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

    await user.click(await screen.findByRole("tab", { name: /Caderno de erros/ }));
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
    const user = userEvent.setup();
    render(() => <Learning />);

    await user.click(await screen.findByRole("tab", { name: /Caderno de erros/ }));
    expect(screen.getByRole("button", { name: "Praticar erros (0)" })).toBeDisabled();
  });

  it("says how many due reviews are in an open attempt", async () => {
    mockIPC(() => ({ ...overview, due: 3, dueInOpenAttempt: 2, practice: 1 }));
    render(() => <Learning />);

    expect(await screen.findByText(/2 em tentativa aberta/)).toBeVisible();
  });

  it("shows no open-attempt note when none are held", async () => {
    mockIPC(() => overview);
    render(() => <Learning />);

    await screen.findByRole("tablist");
    expect(screen.queryByText(/em tentativa aberta/)).toBeNull();
  });

  it("has a pt-BR empty state and disabled practice buttons with nothing to review", async () => {
    mockIPC(() => ({
      due: 0,
      dueInOpenAttempt: 0,
      upcoming: 0,
      errors: 0,
      practice: 0,
      practiceErrors: 0,
      items: [],
    }));
    render(() => <Learning />);

    expect(await screen.findByText("Nenhuma revisão para hoje.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Revisar tudo de hoje (0)" })).toBeDisabled();
  });

  it("shows the backend's message when loading fails", async () => {
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "Falha ao ler o banco." }));
    render(() => <Learning />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao ler o banco.");
  });
});
