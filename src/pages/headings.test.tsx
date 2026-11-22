import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import Bank from "./Bank";
import Home from "./Home";
import Learning from "./Learning";
import Performance from "./Performance";
import Settings from "./Settings";

// Every command fails, so each page shows its error state: the heading structure must not depend on data.
const pages = [
  ["Início", Home],
  ["Banco", Bank],
  ["Desempenho", Performance],
  ["Erros e revisões", Learning],
  ["Configurações", Settings],
] as const;

describe("page headings", () => {
  it.each(pages)("%s has exactly one h1, with the page name", async (name, Page) => {
    mockIPC((command) =>
      command === "legacy_import_status"
        ? { report: null, bankEmpty: false }
        : // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({ message: "Falhou." }),
    );
    render(() => (
      <HashRouter>
        <Route path="/" component={Page} />
      </HashRouter>
    ));
    await waitFor(() => expect(screen.getAllByRole("alert").length).toBeGreaterThan(0));
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);
    expect(h1s[0]).toHaveTextContent(name);
  });
});
