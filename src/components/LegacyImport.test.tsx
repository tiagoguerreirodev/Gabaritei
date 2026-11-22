import { HashRouter, Route } from "@solidjs/router";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it, vi } from "vitest";
import type { LegacyImportReport } from "../bindings/LegacyImportReport";
import type { LegacyImportStatus } from "../bindings/LegacyImportStatus";
import Home from "../pages/Home";
import { emptyHome } from "../test/fixtures";
import Settings from "../pages/Settings";
import { createBusy } from "../pages/settings/busy";
import LegacyImport from "./LegacyImport";

const report: LegacyImportReport = {
  importedAt: "2026-11-19T12:00:00.000Z",
  questions: 9368,
  collections: 352,
  collectionItems: 9757,
  topics: 344,
  images: 52,
};
/** Lets pending promises (the mocked IPC call and the resource) resolve. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const summary = "Importado em 19/11/2026: 9.368 questões, 352 cadernos, 344 assuntos e 52 imagens.";

function backend(status: LegacyImportStatus, onImport: () => unknown = () => null) {
  const calls: string[] = [];
  mockIPC((command) => {
    calls.push(command);
    if (command === "legacy_import_status") return status;
    if (command === "home") return emptyHome;
    if (command === "import_legacy_bank") return onImport();
    throw new Error(`Unexpected command: ${command}`);
  });
  return calls;
}

describe("legacy import", () => {
  it("is offered on Início while the bank is empty, and shows the summary after importing", async () => {
    const calls = backend({ report: null, bankEmpty: true }, () => ({
      report,
      alreadyImported: false,
    }));
    const user = userEvent.setup();
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));

    await user.click(await screen.findByRole("button", { name: "Escolher pasta e importar" }));

    expect(await screen.findByTestId("legacy-import-summary")).toHaveTextContent(summary);
    expect(calls).toContain("import_legacy_bank");
    expect(screen.queryByRole("button", { name: /importar/ })).not.toBeInTheDocument();
  });

  it("is not offered on Início once the bank has questions or the import ran", async () => {
    for (const status of [
      { report: null, bankEmpty: false },
      { report, bankEmpty: false },
    ]) {
      const calls = backend(status);
      const { unmount } = render(() => (
        <HashRouter>
          <Route path="/" component={Home} />
        </HashRouter>
      ));
      await vi.waitFor(() => expect(calls).toContain("legacy_import_status"));
      await settle();
      expect(screen.queryByText("Banco do Caderno Local")).not.toBeInTheDocument();
      unmount();
    }
  });

  it("keeps the button when the folder picker is cancelled", async () => {
    backend({ report: null, bankEmpty: true }, () => null);
    const user = userEvent.setup();
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));

    await user.click(await screen.findByRole("button", { name: "Escolher pasta e importar" }));

    expect(await screen.findByRole("button", { name: "Escolher pasta e importar" })).toBeEnabled();
    expect(screen.queryByTestId("legacy-import-summary")).not.toBeInTheDocument();
  });

  it("shows the backend's pt-BR error and lets the user try again", async () => {
    // The backend rejects with a plain `ErrorPayload` object, not an Error instance.
    backend({ report: null, bankEmpty: true }, () =>
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      Promise.reject({
        message: "A imagem x.png não confere com o manifesto. Nenhum dado foi alterado.",
      }),
    );
    const user = userEvent.setup();
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));

    await user.click(await screen.findByRole("button", { name: "Escolher pasta e importar" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A imagem x.png não confere com o manifesto. Nenhum dado foi alterado.",
    );
    expect(screen.getByRole("button", { name: "Escolher pasta e importar" })).toBeEnabled();
  });

  it("shows the import status under Configurações → Dados", async () => {
    backend({ report, bankEmpty: false });
    render(() => <Settings />);

    expect(await screen.findByTestId("legacy-import-summary")).toHaveTextContent(summary);
  });

  describe("shared busy state", () => {
    it("cannot start while another Dados action runs", async () => {
      const calls = backend({ report: null, bankEmpty: true });
      const busy = createBusy();
      let release!: () => void;
      void busy.run(() => new Promise<void>((resolve) => (release = resolve)));
      render(() => <LegacyImport initialReport={null} busy={busy} />);

      expect(screen.getByRole("button", { name: "Escolher pasta e importar" })).toBeDisabled();
      release();
      await vi.waitFor(() =>
        expect(screen.getByRole("button", { name: "Escolher pasta e importar" })).toBeEnabled(),
      );
      expect(calls).not.toContain("import_legacy_bank");
    });

    it("does nothing when clicked while another action runs", async () => {
      const calls = backend({ report: null, bankEmpty: true });
      const busy = createBusy();
      let release!: () => void;
      void busy.run(() => new Promise<void>((resolve) => (release = resolve)));
      const user = userEvent.setup();
      render(() => <LegacyImport initialReport={null} busy={busy} />);

      await user.click(screen.getByRole("button", { name: "Escolher pasta e importar" }));

      expect(calls).not.toContain("import_legacy_bank");
      release();
    });

    it("holds the shared flag while it runs, so other actions are blocked", async () => {
      let finish!: (value: unknown) => void;
      backend(
        { report: null, bankEmpty: true },
        () => new Promise((resolve) => (finish = resolve)),
      );
      const busy = createBusy();
      const user = userEvent.setup();
      render(() => <LegacyImport initialReport={null} busy={busy} />);

      await user.click(screen.getByRole("button", { name: "Escolher pasta e importar" }));
      await vi.waitFor(() => expect(busy.active()).toBe(true));
      expect(await busy.run(() => Promise.resolve())).toBe(false);

      finish(null);
      await vi.waitFor(() => expect(busy.active()).toBe(false));
    });
  });
});
