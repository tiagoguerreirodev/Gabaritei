import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Toaster, dismissToast } from "../../components/toasts";
import DataFileSection from "./DataFileSection";
import { createAction } from "../../actions";

function backend(results: { export?: boolean; import?: boolean | { message: string } } = {}) {
  const calls: string[] = [];
  mockIPC((command) => {
    calls.push(command);
    if (command === "export_data") return results.export ?? true;
    if (command === "import_data") {
      const outcome = results.import ?? true;
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the backend rejects with a plain { message }
      if (typeof outcome === "object") throw outcome;
      return outcome;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return calls;
}

const view = (onImported?: () => void) =>
  render(() => (
    <>
      <DataFileSection busy={createAction()} onImported={onImported} />
      <Toaster />
    </>
  ));

describe("DataFileSection", () => {
  // Toasts live in a module-level store, so one test's toast would show up in the next.
  afterEach(() => {
    for (let id = 0; id < 50; id++) dismissToast(id);
  });

  it("warns about using one computer at a time", () => {
    backend();
    view();
    expect(screen.getByText("use um computador de cada vez")).toBeInTheDocument();
  });

  it("exports and confirms; a cancelled dialog says nothing", async () => {
    const calls = backend();
    const user = userEvent.setup();
    view();
    await user.click(screen.getByRole("button", { name: "Exportar dados…" }));
    expect(await screen.findByText("Arquivo de dados salvo.")).toBeInTheDocument();
    expect(calls).toEqual(["export_data"]);
  });

  it("does not report anything when the export dialog is cancelled", async () => {
    backend({ export: false });
    const user = userEvent.setup();
    view();
    await user.click(screen.getByRole("button", { name: "Exportar dados…" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Exportar dados…" })).toBeEnabled(),
    );
    expect(screen.queryByText("Arquivo de dados salvo.")).not.toBeInTheDocument();
  });

  it("asks before importing, and imports only after the confirmation", async () => {
    const calls = backend();
    const onImported = vi.fn();
    const user = userEvent.setup();
    view(onImported);

    await user.click(screen.getByRole("button", { name: "Importar dados…" }));
    const dialog = await screen.findByRole("dialog");
    expect(calls).toEqual([]);
    expect(within(dialog).getByText(/serão substituídos/)).toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: "Escolher arquivo e importar" }));

    expect(await screen.findByText("Dados importados.")).toBeInTheDocument();
    expect(calls).toEqual(["import_data"]);
    expect(onImported).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does nothing when the confirmation is cancelled", async () => {
    const calls = backend();
    const user = userEvent.setup();
    view();
    await user.click(screen.getByRole("button", { name: "Importar dados…" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancelar" }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("shows why an invalid file was rejected and does not reload", async () => {
    backend({
      import: { message: "Este não é um arquivo de dados do Gabaritei. Nada foi alterado." },
    });
    const onImported = vi.fn();
    const user = userEvent.setup();
    view(onImported);
    await user.click(screen.getByRole("button", { name: "Importar dados…" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Escolher arquivo e importar",
      }),
    );
    expect(await screen.findByText(/Nada foi alterado/)).toBeInTheDocument();
    expect(onImported).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes quietly when the file dialog is cancelled", async () => {
    backend({ import: false });
    const onImported = vi.fn();
    const user = userEvent.setup();
    view(onImported);
    await user.click(screen.getByRole("button", { name: "Importar dados…" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Escolher arquivo e importar",
      }),
    );
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(onImported).not.toHaveBeenCalled();
    expect(screen.queryByText("Dados importados.")).not.toBeInTheDocument();
  });
});
