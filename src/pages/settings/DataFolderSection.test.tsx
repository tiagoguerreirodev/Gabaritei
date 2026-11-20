import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import { Toaster, dismissToast } from "../../components/toasts";
import DataFolderSection from "./DataFolderSection";

function backend(openFails?: string) {
  const calls: { command: string; args: unknown }[] = [];
  mockIPC((command, args) => {
    calls.push({ command, args });
    if (command === "data_folder") return "/home/dados/gabaritei";
    if (command === "open_folder") {
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the backend rejects with a plain { message }
      if (openFails) throw { message: openFails };
      return null;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return calls;
}

describe("DataFolderSection", () => {
  // Toasts live in a module-level store, so one test's toast would show up in the next.
  afterEach(() => {
    for (let id = 0; id < 50; id++) dismissToast(id);
  });

  it("shows the data folder and opens it by name, never by path", async () => {
    const calls = backend();
    const user = userEvent.setup();
    render(() => <DataFolderSection />);

    expect(await screen.findByTestId("data-folder")).toHaveTextContent("/home/dados/gabaritei");
    await user.click(screen.getByRole("button", { name: "Abrir pasta de dados" }));

    const open = calls.find((call) => call.command === "open_folder");
    expect(open?.args).toEqual({ which: "data" });
  });

  it("tells the user when the folder cannot be opened", async () => {
    backend("Não foi possível abrir a pasta.");
    const user = userEvent.setup();
    render(() => (
      <>
        <DataFolderSection />
        <Toaster />
      </>
    ));

    await user.click(await screen.findByRole("button", { name: "Abrir pasta de dados" }));

    expect(await screen.findByText("Não foi possível abrir a pasta.")).toBeInTheDocument();
  });
});
