import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import App from "./App";
import { routes } from "./routes";
import { emptyHome, emptyPerformance } from "./test/fixtures";

function linkLabels(navName: string) {
  const nav = screen.getByRole("navigation", { name: navName });
  return within(nav)
    .getAllByRole("link")
    .map((link) => link.textContent);
}

describe("App shell", () => {
  beforeEach(() => {
    mockIPC((command) => {
      if (command === "app_info") return { name: "Gabaritei", version: "9.9.9" };
      if (command === "legacy_import_status") return { report: null, bankEmpty: false };
      if (command === "home") return emptyHome;
      if (command === "performance") return emptyPerformance;
      if (command === "topics_tree") return { areas: [], questions: 0 };
      if (command === "generation_status")
        return {
          revision: 0,
          running: false,
          startedAt: null,
          kind: null,
          stage: "preparing",
          stages: [],
          cancelDiscardsDraft: false,
          canCancel: false,
          progress: null,
          outcome: null,
          outcomeKind: null,
        };
      if (command === "drafts_waiting") return [];
      if (command === "collections")
        return { items: [], total: 0, page: 0, pageSize: 20, contests: [], boards: [] };
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it("lists the main destinations and settings in the sidebar, in pt-BR", () => {
    render(() => <App />);

    expect(linkLabels("Menu principal")).toEqual([
      "Início",
      "Banco",
      "Gerar",
      "Desempenho",
      "Erros e revisões",
    ]);
    expect(linkLabels("Menu secundário")).toEqual(["Configurações"]);
  });

  it("opens every menu page with its own heading", async () => {
    const user = userEvent.setup();
    render(() => <App />);

    for (const route of routes.filter((r) => r.menu === "main" || r.menu === "footer")) {
      await user.click(screen.getByRole("link", { name: route.label }));
      expect(await screen.findByRole("heading", { level: 1, name: route.label })).toBeVisible();
      expect(screen.getByRole("link", { name: route.label })).toHaveClass("active");
    }
  });

  it("reaches the contextual pages by their routes", async () => {
    const user = userEvent.setup();
    render(() => <App />);

    // Revisar pacote opens from the "pacote pronto" notice and the waiting list.
    window.location.hash = "#/revisar-pacote";
    expect(await screen.findByRole("heading", { level: 1, name: "Revisar pacote" })).toBeVisible();
    await user.click(screen.getByRole("link", { name: "Gerar" }));

    window.location.hash = "#/resolver";
    expect(await screen.findByRole("heading", { level: 1, name: "Resolver" })).toBeVisible();
  });

  it("offers a skip link that moves focus to the page content", async () => {
    const user = userEvent.setup();
    render(() => <App />);

    await user.tab();
    const skip = screen.getByRole("button", { name: "Pular para o conteúdo" });
    expect(skip).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("main")).toHaveFocus();
  });

  it("moves focus to the page content when the route changes", async () => {
    const user = userEvent.setup();
    render(() => <App />);
    expect(screen.getByRole("main")).not.toHaveFocus();

    await user.click(screen.getByRole("link", { name: "Banco" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Banco" })).toBeVisible();
    expect(screen.getByRole("main")).toHaveFocus();
  });

  it("shows the name and version reported by the backend", async () => {
    render(() => <App />);

    expect(await screen.findByTestId("app-version")).toHaveTextContent("Gabaritei 9.9.9");
  });

  it("shows a not-found page for unknown addresses", async () => {
    window.location.hash = "#/nao-existe";
    render(() => <App />);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Página não encontrada" }),
    ).toBeVisible();
  });
});
