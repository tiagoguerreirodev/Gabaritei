import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { ActiveAttempt } from "../bindings/ActiveAttempt";
import type { HistoryItem } from "../bindings/HistoryItem";
import type { HistoryPage } from "../bindings/HistoryPage";
import type { HistoryQuery } from "../bindings/HistoryQuery";
import History from "./History";

const open: ActiveAttempt = {
  id: "att-open",
  title: "Direito Administrativo + 1",
  sourceKind: "filter",
  feedbackMode: "study",
  startedAt: "2026-11-25T14:02:00.000Z",
  answered: 8,
  total: 20,
};

function item(overrides: Partial<HistoryItem> = {}): HistoryItem {
  return {
    id: "att-done",
    title: "Caderno TRF5 2024",
    sourceKind: "collection",
    collectionId: "c-trf5",
    feedbackMode: "exam",
    startedAt: "2026-11-24T20:00:00.000Z",
    finishedAt: "2026-11-24T21:10:00.000Z",
    total: 40,
    score: {
      correct: 34,
      wrong: 4,
      blank: 2,
      annulled: 0,
      evaluated: 40,
      achievement: 0.85,
      accuracy: 0.9,
    },
    ...overrides,
  };
}

function page(items: HistoryItem[], total = items.length): HistoryPage {
  return { items, total, page: 0, pageSize: 20 };
}

const queries: HistoryQuery[] = [];

function backend(options: {
  open?: ActiveAttempt[];
  history?: (query: HistoryQuery) => HistoryPage;
  fail?: boolean;
}) {
  mockIPC((command, args) => {
    if (options.fail) {
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      return Promise.reject({ message: "Falhou." });
    }
    if (command === "active_attempts") return options.open ?? [];
    if (command === "attempt_history") {
      const { query } = args as { query: HistoryQuery };
      queries.push(query);
      return (options.history ?? (() => page([])))(query);
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

function renderPage() {
  return render(() => (
    <HashRouter>
      <Route path="/" component={History} />
    </HashRouter>
  ));
}

describe("Histórico", () => {
  beforeEach(() => {
    queries.length = 0;
    window.location.hash = "#/";
  });

  it("lists open attempts before finished ones", async () => {
    backend({ open: [open], history: () => page([item()]) });
    renderPage();
    const headings = await screen.findAllByRole("heading", { level: 2 });
    expect(headings.map((heading) => heading.textContent)).toEqual(["Em andamento", "Finalizadas"]);
    expect(screen.getByText("8/20 respondidas")).toBeVisible();
    expect(screen.getByText("34/40")).toBeVisible();
    expect(screen.getByText("85%")).toBeVisible();
  });

  it("opens a finished attempt", async () => {
    const user = userEvent.setup();
    backend({ history: () => page([item()]) });
    renderPage();
    await user.click(await screen.findByRole("button", { name: "Abrir Caderno TRF5 2024" }));
    expect(window.location.hash).toBe("#/resolver?tentativa=att-done");
  });

  it("continues an open attempt", async () => {
    const user = userEvent.setup();
    backend({ open: [open], history: () => page([item()]) });
    renderPage();
    await user.click(await screen.findByRole("button", { name: /Continuar/ }));
    expect(window.location.hash).toBe("#/resolver?tentativa=att-open");
  });

  it("refazer a caderno attempt links to the caderno", async () => {
    backend({ history: () => page([item()]) });
    renderPage();
    const link = await screen.findByRole("link", { name: /Refazer/ });
    expect(link).toHaveAttribute("href", "#/banco?caderno=c-trf5");
  });

  it("refazer uses the attempt questions for old kinds", async () => {
    const older = item({ sourceKind: "errors", collectionId: null, id: "att-err" });
    backend({ history: () => page([older]) });
    renderPage();
    const link = await screen.findByRole("link", { name: /Refazer/ });
    expect(link).toHaveAttribute("href", "#/banco?tentativa=att-err");
  });

  it("filters send the query and go back to the first page", async () => {
    const user = userEvent.setup();
    backend({ history: () => page([item()], 45) });
    renderPage();
    await screen.findByText("Caderno TRF5 2024");
    await user.click(screen.getByRole("button", { name: "Próxima" }));
    await user.selectOptions(screen.getByLabelText("Modo"), "Simulado");
    await waitFor(() =>
      expect(queries.at(-1)).toEqual({
        source: null,
        mode: "exam",
        page: 0,
        pageSize: 20,
      }),
    );
  });

  it("pages through finished attempts", async () => {
    const user = userEvent.setup();
    backend({ history: () => page([item()], 45) });
    renderPage();
    expect(await screen.findByText("Página 1 de 3")).toBeVisible();
    expect(screen.getByRole("button", { name: "Anterior" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Próxima" }));
    expect(await screen.findByText("Página 2 de 3")).toBeVisible();
    expect(queries.at(-1)?.page).toBe(1);
  });

  it("shows a dash without a score", async () => {
    const annulled = item({
      score: {
        correct: 0,
        wrong: 0,
        blank: 0,
        annulled: 10,
        evaluated: 0,
        achievement: null,
        accuracy: null,
      },
    });
    backend({ history: () => page([annulled]) });
    renderPage();
    const row = (await screen.findByText("Caderno TRF5 2024")).closest("li")!;
    expect(within(row).getByText("—")).toBeVisible();
  });

  it("shows an empty state that links to Banco", async () => {
    backend({});
    renderPage();
    expect(await screen.findByText("Nenhuma tentativa ainda.")).toBeVisible();
    expect(screen.getByRole("link", { name: "Ir para o Banco" })).toHaveAttribute(
      "href",
      "#/banco",
    );
  });

  it("shows a retry when loading fails", async () => {
    backend({ fail: true });
    renderPage();
    expect((await screen.findAllByRole("alert"))[0]).toHaveTextContent("Falhou.");
    expect(screen.getAllByRole("button", { name: "Tentar novamente" }).length).toBeGreaterThan(0);
  });
});
