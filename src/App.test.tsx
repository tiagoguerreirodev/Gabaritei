import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import App from "./App";
import { routes } from "./routes";

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

    for (const route of routes.filter((r) => r.menu !== "none")) {
      await user.click(screen.getByRole("link", { name: route.label }));
      expect(await screen.findByRole("heading", { level: 1, name: route.label })).toBeVisible();
      expect(screen.getByRole("link", { name: route.label })).toHaveClass("active");
    }
  });

  it("reaches the contextual pages from the page that opens them", async () => {
    const user = userEvent.setup();
    render(() => <App />);

    await user.click(screen.getByRole("link", { name: "Gerar" }));
    await user.click(screen.getByRole("link", { name: "Abrir a tela de revisão de pacote" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Revisar pacote" })).toBeVisible();

    await user.click(screen.getByRole("link", { name: "Banco" }));
    await user.click(screen.getByRole("link", { name: "Abrir a tela de resolução" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Resolver" })).toBeVisible();
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
