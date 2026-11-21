import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import type { HomeSummary } from "../bindings/HomeSummary";
import { emptyHome, stats } from "../test/fixtures";
import Home from "./Home";

const summary: HomeSummary = {
  questions: 9368,
  overall: stats(30, 10, 10, 100),
  areas: [
    { name: "Português", stats: stats(2, 8, 0, 30) },
    { name: "Direito", stats: stats(28, 2, 10, 40) },
    { name: "Informática", stats: stats(0, 0, 0, 30) },
  ],
  activeAttempts: [
    {
      id: "att-7",
      title: "Direito › Constitucional › Controle",
      sourceKind: "topic",
      feedbackMode: "exam",
      startedAt: "2026-11-19T12:00:00.000Z",
      answered: 4,
      total: 10,
    },
  ],
  dueReviews: 3,
  dueInOpenAttempt: 2,
  draftsWaiting: 2,
};

function backend(home: HomeSummary | { error: string }) {
  mockIPC((command) => {
    if (command === "legacy_import_status") return { report: null, bankEmpty: false };
    if (command === "home") {
      if ("error" in home) {
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ message: home.error });
      }
      return home;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

describe("Início", () => {
  it("shows the summary cards, achievement per area, attempts to resume and what waits", async () => {
    backend(summary);
    render(() => <Home />);

    const cards = await screen.findByRole("list", { name: "Resumo" });
    expect(within(cards).getByText("Aproveitamento").nextElementSibling).toHaveTextContent("60%");
    expect(screen.getByText("9.368 questões no banco.")).toBeVisible();

    const areas = within(
      screen.getByRole("heading", { name: "Aproveitamento por área" }).parentElement!,
    );
    const names = areas.getAllByRole("listitem").map((li) => li.textContent);
    expect(names).toEqual(["Português20%", "Direito70%"]);

    const waiting = screen.getByRole("heading", { name: "Aguardando você" }).parentElement!;
    expect(within(waiting).getByRole("link", { name: "Revisões pendentes" })).toHaveAttribute(
      "href",
      "#/erros-e-revisoes",
    );
    expect(
      within(waiting).getByRole("link", { name: "Revisões pendentes" }).parentElement,
    ).toHaveTextContent("3 · 2 em tentativa aberta");
    expect(
      within(waiting).getByRole("link", { name: "Pacotes para revisar" }).parentElement,
    ).toHaveTextContent("2");
  });

  it("resumes an active attempt in the Resolver", async () => {
    backend(summary);
    const user = userEvent.setup();
    render(() => <Home />);

    expect(await screen.findByText(/Simulado · 4 de 10 respondidas/)).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Retomar Direito › Constitucional › Controle" }),
    );
    expect(window.location.hash).toBe("#/resolver?tentativa=att-7");
  });

  it("refreshes after the first-run legacy import", async () => {
    let imported = false;
    mockIPC((command) => {
      if (command === "legacy_import_status") return { report: null, bankEmpty: !imported };
      if (command === "import_legacy_bank") {
        imported = true;
        return {
          report: {
            importedAt: "2026-11-19T12:00:00.000Z",
            questions: 9368,
            collections: 1,
            collectionItems: 1,
            topics: 1,
            images: 0,
          },
          alreadyImported: false,
        };
      }
      if (command === "home") return imported ? summary : emptyHome;
      throw new Error(`Unexpected command: ${command}`);
    });
    const user = userEvent.setup();
    render(() => <Home />);

    expect(await screen.findByText("0 questões no banco.")).toBeVisible();
    await user.click(await screen.findByRole("button", { name: "Escolher pasta e importar" }));

    expect(await screen.findByText("9.368 questões no banco.")).toBeVisible();
    // The import card keeps its summary even though the bank is no longer empty.
    expect(screen.getByTestId("legacy-import-summary")).toBeVisible();
  });

  it("has pt-BR empty states for a fresh install", async () => {
    backend(emptyHome);
    render(() => <Home />);

    expect(await screen.findByText("Você ainda não finalizou nenhuma tentativa.")).toBeVisible();
    expect(screen.getByText("Nenhuma tentativa em andamento.")).toBeVisible();
    expect(screen.queryByRole("list", { name: "Resumo" })).not.toBeInTheDocument();
  });

  it("shows the backend's message when loading fails", async () => {
    backend({ error: "Falha ao ler o banco." });
    render(() => <Home />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao ler o banco.");
  });
});
